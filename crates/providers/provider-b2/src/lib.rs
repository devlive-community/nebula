//! 把 [`backblaze_b2::S3Client`] 适配成统一的 [`StorageProvider`]。
//!
//! App 私有适配层(不发布)。B2 走 S3 兼容 API + SigV4,路径遵循 `bucket/key` 约定,与
//! AWS/R2/MinIO 等 S3 系适配层同构。B2 的 endpoint 形如 `s3.{region}.backblazeb2.com`,
//! 和 AWS 同形状,`S3Client::new` 直接解析 region,不需要专门的构造函数。
//!
//! 能力矩阵已对照 Backblaze 官方文档逐项核实(不是从 R2/MinIO 抄近似值):对象标签、
//! 对象级 ACL 写入、静态网站托管官方文档明确写不支持;版本控制是"桶默认永远开启、没有
//! 开关"这种和 Nebula `versioning` 能力位(开关 + 历史合一)不匹配的语义,故也标不支持;
//! 存储类型转换未在文档中找到对应的头,保守不支持。生命周期规则、CORS、预签名、服务端
//! 复制、分片上传均有文档明确支持。

use async_trait::async_trait;
use bytes::Bytes;
use futures::StreamExt;

use backblaze_b2::{ListEntry, S3Client, S3Error};
use nebula_provider::{
    path, ByteStream, Capabilities, CorsRule, Entry, IncompleteUpload, LifecycleRule, ProgressFn,
    ProviderError, Result, StorageProvider,
};

const MULTIPART_THRESHOLD: usize = 16 * 1024 * 1024;
const MULTIPART_PART_SIZE: usize = 8 * 1024 * 1024;

fn should_multipart(len: usize) -> bool {
    len > MULTIPART_THRESHOLD
}

/// Backblaze B2 的 provider 适配器。一个实例 = 一个账号。
pub struct B2Provider {
    id: String,
    client: S3Client,
}

impl B2Provider {
    /// 用账号别名与凭证创建。
    pub fn new(
        id: impl Into<String>,
        access_key: impl Into<String>,
        secret_key: impl Into<String>,
        endpoint: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            client: S3Client::new(access_key, secret_key, endpoint),
        }
    }

    /// 复用已构造好的 [`S3Client`]。
    pub fn from_client(id: impl Into<String>, client: S3Client) -> Self {
        Self {
            id: id.into(),
            client,
        }
    }
}

fn require_object(path: &str) -> Result<(&str, &str)> {
    match path::split(path) {
        (Some(bucket), key) if !key.is_empty() => Ok((bucket, key)),
        _ => Err(ProviderError::InvalidPath(path.to_string())),
    }
}

/// 把 SDK 的生命周期规则映射到统一 provider 模型(字段一一对应)。
fn rule_from_sdk(r: backblaze_b2::LifecycleRule) -> LifecycleRule {
    LifecycleRule {
        id: r.id,
        prefix: r.prefix,
        enabled: r.enabled,
        expiration_days: r.expiration_days,
        transitions: r.transitions,
    }
}

/// 把统一 provider 模型映射回 SDK 的生命周期规则。
fn rule_to_sdk(r: &LifecycleRule) -> backblaze_b2::LifecycleRule {
    backblaze_b2::LifecycleRule {
        id: r.id.clone(),
        prefix: r.prefix.clone(),
        enabled: r.enabled,
        expiration_days: r.expiration_days,
        transitions: r.transitions.clone(),
    }
}

/// 把 SDK 的 CORS 规则映射到统一 provider 模型(字段一一对应)。
fn cors_from_sdk(r: backblaze_b2::CorsRule) -> CorsRule {
    CorsRule {
        id: r.id,
        allowed_origins: r.allowed_origins,
        allowed_methods: r.allowed_methods,
        allowed_headers: r.allowed_headers,
        expose_headers: r.expose_headers,
        max_age_seconds: r.max_age_seconds,
    }
}

/// 把统一 provider 模型映射回 SDK 的 CORS 规则。
fn cors_to_sdk(r: &CorsRule) -> backblaze_b2::CorsRule {
    backblaze_b2::CorsRule {
        id: r.id.clone(),
        allowed_origins: r.allowed_origins.clone(),
        allowed_methods: r.allowed_methods.clone(),
        allowed_headers: r.allowed_headers.clone(),
        expose_headers: r.expose_headers.clone(),
        max_age_seconds: r.max_age_seconds,
    }
}

fn map_err(err: S3Error) -> ProviderError {
    match err {
        S3Error::Api { code, message, .. } => match code.as_str() {
            "NoSuchKey" | "NoSuchBucket" => ProviderError::NotFound(message),
            "AccessDenied" | "InvalidAccessKeyId" | "SignatureDoesNotMatch" => {
                ProviderError::AccessDenied(message)
            }
            _ => ProviderError::Backend(format!("{code}: {message}")),
        },
        other => ProviderError::Backend(other.to_string()),
    }
}

#[async_trait]
impl StorageProvider for B2Provider {
    fn id(&self) -> &str {
        &self.id
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            multipart_upload: true,
            resumable_upload: true,
            // 官方文档没有找到类似 x-amz-storage-class 的头,不确定就不假装支持。
            storage_class_ops: false,
            bucket_ops: true,
            metadata_ops: true,
            // 官方文档明确写 Object Tagging 目前不支持。
            object_tagging: false,
            multipart_cleanup: true,
            // 官方文档:Get Object ACL 能用,但对象级 ACL 写入不支持——set_object_acl /
            // public_url 都依赖这个能力位,一起关掉。
            object_acl: false,
            presign: true,
            server_side_copy: true,
            hierarchical: false,
            // 官方博客确认 S3 兼容生命周期规则 API(Put/Get/Delete)已支持。
            bucket_lifecycle: true,
            // B2 的桶默认永远开启版本、没有"启用/暂停"开关,和 Nebula 这个能力位"开关 +
            // 历史"合一的语义不匹配,宁可标不支持也不做半个功能。
            versioning: false,
            // 官方文档有专门的"如何用 S3 兼容 API 启用 CORS"页面。
            bucket_cors: true,
            // 官方文档明确写 Website configuration 目前不支持。
            bucket_website: false,
        }
    }

    async fn list(&self, path: &str) -> Result<Vec<Entry>> {
        match path::split(path) {
            (None, _) => {
                let stream = self.client.list_buckets();
                futures::pin_mut!(stream);
                let mut entries = Vec::new();
                while let Some(item) = stream.next().await {
                    let bucket = item.map_err(map_err)?;
                    entries.push(Entry::directory(bucket.name));
                }
                Ok(entries)
            }
            (Some(bucket), prefix) => {
                let prefix_opt = (!prefix.is_empty()).then_some(prefix);
                let stream = self.client.list_dir(bucket, prefix_opt);
                futures::pin_mut!(stream);
                let mut entries = Vec::new();
                while let Some(item) = stream.next().await {
                    match item.map_err(map_err)? {
                        ListEntry::Prefix(prefix) => {
                            entries.push(Entry::directory(format!("{bucket}/{prefix}")));
                        }
                        ListEntry::Object(obj) => {
                            entries.push(
                                Entry::file(format!("{bucket}/{}", obj.key), obj.size)
                                    .with_etag(obj.etag)
                                    .with_last_modified(obj.last_modified)
                                    .with_storage_class(obj.storage_class),
                            );
                        }
                    }
                }
                Ok(entries)
            }
        }
    }

    async fn list_page(&self, path: &str, cursor: Option<String>) -> Result<nebula_provider::Page> {
        match path::split(path) {
            // 根:桶数量少,一次列全,无分页。
            (None, _) => Ok(nebula_provider::Page {
                entries: self.list(path).await?,
                cursor: None,
            }),
            (Some(bucket), prefix) => {
                let prefix_opt = (!prefix.is_empty()).then_some(prefix);
                let page = self
                    .client
                    .list_dir_page(bucket, prefix_opt, cursor.unwrap_or_default())
                    .await
                    .map_err(map_err)?;
                let entries = page
                    .items
                    .into_iter()
                    .map(|item| match item {
                        ListEntry::Prefix(prefix) => Entry::directory(format!("{bucket}/{prefix}")),
                        ListEntry::Object(obj) => {
                            Entry::file(format!("{bucket}/{}", obj.key), obj.size)
                                .with_etag(obj.etag)
                                .with_last_modified(obj.last_modified)
                                .with_storage_class(obj.storage_class)
                        }
                    })
                    .collect();
                Ok(nebula_provider::Page {
                    entries,
                    cursor: page.next,
                })
            }
        }
    }

    async fn stat(&self, path: &str) -> Result<Entry> {
        match path::split(path) {
            (Some(bucket), "") => Ok(Entry::directory(bucket.to_string())),
            (Some(bucket), key) => {
                let meta = self
                    .client
                    .head_object(bucket, key)
                    .await
                    .map_err(map_err)?;
                let mut entry = Entry::file(format!("{bucket}/{key}"), meta.content_length);
                if let Some(etag) = meta.etag {
                    entry = entry.with_etag(etag);
                }
                if let Some(lm) = meta.last_modified {
                    entry = entry.with_last_modified(lm);
                }
                if let Some(ct) = meta.content_type {
                    entry = entry.with_content_type(ct);
                }
                Ok(entry)
            }
            (None, _) => Err(ProviderError::InvalidPath(path.to_string())),
        }
    }

    async fn read(&self, path: &str) -> Result<Bytes> {
        let (bucket, key) = require_object(path)?;
        self.client.get_object(bucket, key).await.map_err(map_err)
    }

    async fn read_stream(&self, path: &str) -> Result<(Option<u64>, ByteStream)> {
        let (bucket, key) = require_object(path)?;
        let (len, stream) = self
            .client
            .get_object_stream(bucket, key)
            .await
            .map_err(map_err)?;
        Ok((len, Box::pin(stream.map(|r| r.map_err(map_err)))))
    }

    async fn read_range(&self, path: &str, offset: u64) -> Result<(Option<u64>, ByteStream)> {
        let (bucket, key) = require_object(path)?;
        let (total, stream) = self
            .client
            .get_object_range(bucket, key, offset)
            .await
            .map_err(map_err)?;
        Ok((total, Box::pin(stream.map(|r| r.map_err(map_err)))))
    }

    async fn write(&self, path: &str, data: Bytes, content_type: Option<&str>) -> Result<()> {
        let (bucket, key) = require_object(path)?;
        if should_multipart(data.len()) {
            self.client
                .upload_multipart(bucket, key, data, MULTIPART_PART_SIZE, content_type)
                .await
                .map_err(map_err)
        } else {
            self.client
                .put_object(bucket, key, data, content_type)
                .await
                .map_err(map_err)
        }
    }

    async fn write_with_progress(
        &self,
        path: &str,
        data: Bytes,
        content_type: Option<&str>,
        progress: ProgressFn<'_>,
    ) -> Result<()> {
        let (bucket, key) = require_object(path)?;
        let total = data.len() as u64;
        if should_multipart(data.len()) {
            self.client
                .upload_multipart_progress(
                    bucket,
                    key,
                    data,
                    MULTIPART_PART_SIZE,
                    content_type,
                    progress,
                )
                .await
                .map_err(map_err)
        } else {
            progress(0, total);
            self.client
                .put_object(bucket, key, data, content_type)
                .await
                .map_err(map_err)?;
            progress(total, total);
            Ok(())
        }
    }

    async fn write_stream(
        &self,
        path: &str,
        len: Option<u64>,
        stream: ByteStream,
        content_type: Option<&str>,
        progress: ProgressFn<'_>,
    ) -> Result<()> {
        let (bucket, key) = require_object(path)?;
        // 已知且不大的对象:收集后简单 PUT,省去分片握手。
        if let Some(l) = len {
            if !should_multipart(l as usize) {
                let data = nebula_provider::collect_stream(stream).await?;
                progress(0, l);
                self.client
                    .put_object(bucket, key, data, content_type)
                    .await
                    .map_err(map_err)?;
                progress(l, l);
                return Ok(());
            }
        }
        // 大文件 / 未知大小:流式分片,内存受控。
        self.client
            .upload_multipart_stream(
                bucket,
                key,
                stream,
                MULTIPART_PART_SIZE,
                content_type,
                len.unwrap_or(0),
                progress,
            )
            .await
            .map_err(map_err)
    }

    async fn begin_multipart(&self, path: &str, content_type: Option<&str>) -> Result<String> {
        let (bucket, key) = require_object(path)?;
        self.client
            .initiate_multipart_upload(bucket, key, content_type)
            .await
            .map_err(map_err)
    }

    async fn upload_part(
        &self,
        path: &str,
        upload_id: &str,
        part_number: u32,
        data: Bytes,
    ) -> Result<String> {
        let (bucket, key) = require_object(path)?;
        self.client
            .upload_part(bucket, key, upload_id, part_number, data)
            .await
            .map_err(map_err)
    }

    async fn complete_multipart(
        &self,
        path: &str,
        upload_id: &str,
        parts: &[(u32, String)],
    ) -> Result<()> {
        let (bucket, key) = require_object(path)?;
        self.client
            .complete_multipart_upload(bucket, key, upload_id, parts)
            .await
            .map_err(map_err)
    }

    async fn abort_multipart(&self, path: &str, upload_id: &str) -> Result<()> {
        let (bucket, key) = require_object(path)?;
        self.client
            .abort_multipart_upload(bucket, key, upload_id)
            .await
            .map_err(map_err)
    }

    async fn create_bucket(&self, bucket: &str) -> Result<()> {
        self.client.create_bucket(bucket).await.map_err(map_err)
    }

    async fn delete_bucket(&self, bucket: &str) -> Result<()> {
        self.client.delete_bucket(bucket).await.map_err(map_err)
    }

    async fn bucket_lifecycle(&self, bucket: &str) -> Result<Vec<LifecycleRule>> {
        let rules = self
            .client
            .get_bucket_lifecycle(bucket)
            .await
            .map_err(map_err)?;
        Ok(rules.into_iter().map(rule_from_sdk).collect())
    }

    async fn set_bucket_lifecycle(&self, bucket: &str, rules: &[LifecycleRule]) -> Result<()> {
        let rules: Vec<_> = rules.iter().map(rule_to_sdk).collect();
        self.client
            .set_bucket_lifecycle(bucket, &rules)
            .await
            .map_err(map_err)
    }

    async fn bucket_cors(&self, bucket: &str) -> Result<Vec<CorsRule>> {
        let rules = self.client.get_bucket_cors(bucket).await.map_err(map_err)?;
        Ok(rules.into_iter().map(cors_from_sdk).collect())
    }

    async fn set_bucket_cors(&self, bucket: &str, rules: &[CorsRule]) -> Result<()> {
        let rules: Vec<_> = rules.iter().map(cors_to_sdk).collect();
        self.client
            .set_bucket_cors(bucket, &rules)
            .await
            .map_err(map_err)
    }

    async fn set_content_type(&self, path: &str, content_type: &str) -> Result<()> {
        let (bucket, key) = require_object(path)?;
        self.client
            .set_content_type(bucket, key, content_type)
            .await
            .map_err(map_err)
    }

    async fn list_incomplete_uploads(&self, bucket: &str) -> Result<Vec<IncompleteUpload>> {
        let uploads = self
            .client
            .list_multipart_uploads(bucket)
            .await
            .map_err(map_err)?;
        Ok(uploads
            .into_iter()
            .map(|u| IncompleteUpload {
                key: u.key,
                upload_id: u.upload_id,
                initiated: u.initiated,
            })
            .collect())
    }

    async fn delete(&self, path: &str) -> Result<()> {
        let (bucket, key) = require_object(path)?;
        self.client
            .delete_object(bucket, key)
            .await
            .map_err(map_err)
    }

    async fn copy(&self, from: &str, to: &str) -> Result<()> {
        let (src_bucket, src_key) = require_object(from)?;
        let (dst_bucket, dst_key) = require_object(to)?;
        self.client
            .copy_object(src_bucket, src_key, dst_bucket, dst_key)
            .await
            .map_err(map_err)
    }

    async fn presign(&self, path: &str, expires_secs: u64) -> Result<String> {
        let (bucket, key) = require_object(path)?;
        self.client
            .presign_get(bucket, key, expires_secs)
            .map_err(map_err)
    }

    async fn presign_put(&self, path: &str, expires_secs: u64) -> Result<String> {
        let (bucket, key) = require_object(path)?;
        self.client
            .presign_put(bucket, key, expires_secs)
            .map_err(map_err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider() -> B2Provider {
        B2Provider::new("test", "ak", "sk", "s3.us-west-002.backblazeb2.com")
    }

    #[test]
    fn region_parsed_from_endpoint() {
        let p = provider();
        assert_eq!(p.client.region(), "us-west-002");
    }

    #[test]
    fn multipart_threshold_decision() {
        assert!(!should_multipart(MULTIPART_THRESHOLD));
        assert!(should_multipart(MULTIPART_THRESHOLD + 1));
    }

    #[test]
    fn id_and_capabilities() {
        let p = provider();
        assert_eq!(p.id(), "test");
        let caps = p.capabilities();
        assert!(caps.presign);
        assert!(caps.bucket_lifecycle);
        assert!(caps.bucket_cors);
        // 逐项核实过不支持的能力,确认没有被误开。
        assert!(!caps.bucket_website);
        assert!(!caps.object_tagging);
        assert!(!caps.object_acl);
        assert!(!caps.versioning);
        assert!(!caps.storage_class_ops);
        assert!(!caps.hierarchical);
    }

    #[test]
    fn map_err_classifies_codes() {
        assert!(matches!(
            map_err(S3Error::Api {
                status: 404,
                code: "NoSuchBucket".into(),
                message: "m".into(),
                request_id: None,
            }),
            ProviderError::NotFound(_)
        ));
    }

    #[tokio::test]
    async fn rejects_rootless_paths() {
        let p = provider();
        assert!(matches!(
            p.read("bucket-only").await,
            Err(ProviderError::InvalidPath(_))
        ));
    }
}
