//! 把 [`wasabi_s3::S3Client`] 适配成统一的 [`StorageProvider`]。
//!
//! App 私有适配层(不发布)。Wasabi 走 S3 兼容 API + SigV4,路径遵循 `bucket/key` 约定,与
//! AWS/R2/MinIO/B2 等 S3 系适配层同构。Wasabi 的 endpoint 形如 `s3.{region}.wasabisys.com`,
//! 和 AWS 同形状,`S3Client::new` 直接解析 region,不需要专门的构造函数。
//!
//! 能力矩阵已对照 Wasabi 官方文档逐项核实,和同为 S3 兼容的 B2/R2/MinIO **都不一样**:
//! 对象标签、对象级 ACL(canned)、版本控制(标准启用/暂停模型)官方文档确认支持;CORS 官方
//! 文档明确写不支持 `PUT`/`GET ?cors` 这套配置接口(Wasabi 是请求带 `Origin` 头时服务端自动
//! 回 CORS 响应头,不需要也不能显式配置规则,和 Nebula 现有"读写一份规则列表"的 CORS 面板模型
//! 没有对应物);静态网站托管官方明确不支持(不打算做,定位是存文件不是托管网站);存储类型
//! 转换不支持,因为 Wasabi 只有一种存储类型,没有可转换的目标。

use async_trait::async_trait;
use bytes::Bytes;
use futures::StreamExt;

use nebula_provider::{
    path, ByteStream, Capabilities, Entry, IncompleteUpload, LifecycleRule, ObjectVersion,
    ProgressFn, ProviderError, Result, StorageProvider,
};
use wasabi_s3::{ListEntry, S3Client, S3Error};

const MULTIPART_THRESHOLD: usize = 16 * 1024 * 1024;
const MULTIPART_PART_SIZE: usize = 8 * 1024 * 1024;

fn should_multipart(len: usize) -> bool {
    len > MULTIPART_THRESHOLD
}

/// Wasabi 的 provider 适配器。一个实例 = 一个账号。
pub struct WasabiProvider {
    id: String,
    client: S3Client,
}

impl WasabiProvider {
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
fn rule_from_sdk(r: wasabi_s3::LifecycleRule) -> LifecycleRule {
    LifecycleRule {
        id: r.id,
        prefix: r.prefix,
        enabled: r.enabled,
        expiration_days: r.expiration_days,
        transitions: r.transitions,
    }
}

/// 把统一 provider 模型映射回 SDK 的生命周期规则。
fn rule_to_sdk(r: &LifecycleRule) -> wasabi_s3::LifecycleRule {
    wasabi_s3::LifecycleRule {
        id: r.id.clone(),
        prefix: r.prefix.clone(),
        enabled: r.enabled,
        expiration_days: r.expiration_days,
        transitions: r.transitions.clone(),
    }
}

/// 把 SDK 的历史版本映射到统一 provider 模型(字段一一对应)。
fn version_from_sdk(v: wasabi_s3::ObjectVersion) -> ObjectVersion {
    ObjectVersion {
        version_id: v.version_id,
        is_latest: v.is_latest,
        is_delete_marker: v.is_delete_marker,
        size: v.size,
        etag: v.etag,
        last_modified: v.last_modified,
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
impl StorageProvider for WasabiProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            multipart_upload: true,
            resumable_upload: true,
            // Wasabi 只有一种存储类型,没有可转换的目标,x-amz-storage-class 在这里没有
            // 实际意义,不做这个只有一个选项的摆设功能。
            storage_class_ops: false,
            bucket_ops: true,
            metadata_ops: true,
            // 官方文档确认支持 PutObjectTagging/GetObjectTagging。
            object_tagging: true,
            multipart_cleanup: true,
            // 协议层支持 canned ACL;公开访问的实际效果可能被 Wasabi 账号等级(免费/试用 vs
            // 付费)限制,Nebula 拿不到账号等级信息,没法提前判断——已知限制,不是漏做。
            object_acl: true,
            presign: true,
            server_side_copy: true,
            hierarchical: false,
            // 官方文档确认支持生命周期策略(过期/转换/清理)。
            bucket_lifecycle: true,
            // 官方文档确认标准启用/暂停模型,和 Nebula 现有语义匹配(不像 B2 那样没有开关)。
            versioning: true,
            // 官方文档明确不支持 PUT/GET ?cors 这套配置接口——Wasabi 用的是"请求带 Origin
            // 头就自动回 CORS 响应头"的免配置机制,和 Nebula 的"读写规则列表"面板没有对应物。
            bucket_cors: false,
            // 官方明确表态不支持指定默认首页/错误页,没有计划支持。
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

    async fn bucket_versioning(&self, bucket: &str) -> Result<bool> {
        self.client
            .get_bucket_versioning(bucket)
            .await
            .map_err(map_err)
    }

    async fn set_bucket_versioning(&self, bucket: &str, enabled: bool) -> Result<()> {
        self.client
            .set_bucket_versioning(bucket, enabled)
            .await
            .map_err(map_err)
    }

    async fn list_object_versions(&self, path: &str) -> Result<Vec<ObjectVersion>> {
        let (bucket, key) = require_object(path)?;
        let versions = self
            .client
            .list_object_versions(bucket, key)
            .await
            .map_err(map_err)?;
        Ok(versions.into_iter().map(version_from_sdk).collect())
    }

    async fn restore_object_version(&self, path: &str, version_id: &str) -> Result<()> {
        let (bucket, key) = require_object(path)?;
        self.client
            .restore_object_version(bucket, key, version_id)
            .await
            .map_err(map_err)
    }

    async fn delete_object_version(&self, path: &str, version_id: &str) -> Result<()> {
        let (bucket, key) = require_object(path)?;
        self.client
            .delete_object_version(bucket, key, version_id)
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

    async fn object_tags(&self, path: &str) -> Result<Vec<(String, String)>> {
        let (bucket, key) = require_object(path)?;
        self.client
            .get_object_tags(bucket, key)
            .await
            .map_err(map_err)
    }

    async fn set_object_tags(&self, path: &str, tags: &[(String, String)]) -> Result<()> {
        let (bucket, key) = require_object(path)?;
        self.client
            .set_object_tags(bucket, key, tags)
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

    async fn set_object_acl(&self, path: &str, public: bool) -> Result<()> {
        let (bucket, key) = require_object(path)?;
        let acl = if public { "public-read" } else { "private" };
        self.client
            .set_object_acl(bucket, key, acl)
            .await
            .map_err(map_err)
    }

    fn public_url(&self, path: &str) -> Option<String> {
        let (bucket, key) = require_object(path).ok()?;
        Some(self.client.public_url(bucket, key))
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

    fn provider() -> WasabiProvider {
        WasabiProvider::new("test", "ak", "sk", "s3.us-west-1.wasabisys.com")
    }

    #[test]
    fn region_parsed_from_endpoint() {
        let p = provider();
        assert_eq!(p.client.region(), "us-west-1");
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
        // 和 B2 刚好相反的四项,容易复制粘贴时漏改,单独断言。
        assert!(caps.object_tagging);
        assert!(caps.versioning);
        assert!(caps.object_acl);
        assert!(!caps.bucket_cors);
        assert!(!caps.bucket_website);
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
