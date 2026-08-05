//! 把 [`scaleway_object_storage::S3Client`] 适配成统一的 [`StorageProvider`]。
//!
//! App 私有适配层(不发布)。Scaleway Object Storage 走标准 S3 API + SigV4,路径遵循
//! `bucket/key` 约定,与 AWS/R2/MinIO/B2/Wasabi 等 S3 系适配层同构。Scaleway 的 endpoint 形如
//! `s3.{region}.scw.cloud`,和 AWS 同形状,`S3Client::new` 直接解析 region,不需要专门的
//! 构造函数。
//!
//! 能力矩阵已对照 Scaleway 官方文档逐项核实,是目前接入的 S3 兼容厂商里支持面最广的一个
//! (标签、ACL、版本控制、生命周期、CORS、静态网站托管、存储类型转换全部支持,和 Wasabi
//! 明确不支持 CORS/网站托管、B2 不支持标签/ACL/CORS/网站托管都不一样):对象标签、生命周期
//! 规则(转换/过期)、版本控制(标准启用/暂停模型)、CORS 配置接口、静态网站托管配置官方文档
//! 均确认支持;存储类型只有 `STANDARD`/`ONEZONE_IA`/`GLACIER` 三档(比 AWS 少,但和 AWS 同名,
//! 可以直接复用 `x-amz-storage-class` 语义),`GLACIER` 仅 `fr-par`/`nl-ams` 两个区域可用——
//! Nebula 拿不到"当前 bucket 所在区域是否支持该存储类型"这层信息,不在客户端做额外校验,
//! 交给服务端在真正设置时报错。ACL 模型比 AWS 简化(官方文档只强调 `public-read`/`private`
//! 两态的"对象可见性"概念,没有 `public-read-write`/`authenticated-read` 等更细粒度的
//! canned ACL),但协议层接受标准的 `x-amz-acl` 头,和 Nebula 现有"公开读/私有"二态模型天然匹配。

use async_trait::async_trait;
use bytes::Bytes;
use futures::StreamExt;

use nebula_provider::{
    path, ByteStream, Capabilities, Entry, IncompleteUpload, LifecycleRule, ObjectVersion,
    ProgressFn, ProviderError, Result, StorageProvider,
};
use scaleway_object_storage::{ListEntry, S3Client, S3Error};

const MULTIPART_THRESHOLD: usize = 16 * 1024 * 1024;
const MULTIPART_PART_SIZE: usize = 8 * 1024 * 1024;

fn should_multipart(len: usize) -> bool {
    len > MULTIPART_THRESHOLD
}

/// Scaleway Object Storage 的 provider 适配器。一个实例 = 一个账号。
pub struct ScalewayProvider {
    id: String,
    client: S3Client,
}

impl ScalewayProvider {
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
fn rule_from_sdk(r: scaleway_object_storage::LifecycleRule) -> LifecycleRule {
    LifecycleRule {
        id: r.id,
        prefix: r.prefix,
        enabled: r.enabled,
        expiration_days: r.expiration_days,
        transitions: r.transitions,
    }
}

/// 把统一 provider 模型映射回 SDK 的生命周期规则。
fn rule_to_sdk(r: &LifecycleRule) -> scaleway_object_storage::LifecycleRule {
    scaleway_object_storage::LifecycleRule {
        id: r.id.clone(),
        prefix: r.prefix.clone(),
        enabled: r.enabled,
        expiration_days: r.expiration_days,
        transitions: r.transitions.clone(),
    }
}

/// 把 SDK 的历史版本映射到统一 provider 模型(字段一一对应)。
fn version_from_sdk(v: scaleway_object_storage::ObjectVersion) -> ObjectVersion {
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
impl StorageProvider for ScalewayProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            multipart_upload: true,
            resumable_upload: true,
            // 官方文档确认支持 STANDARD/ONEZONE_IA/GLACIER 三档存储类型转换。
            storage_class_ops: true,
            bucket_ops: true,
            metadata_ops: true,
            // 官方文档确认支持对象标签(tags),用于分类与细粒度访问控制。
            object_tagging: true,
            multipart_cleanup: true,
            // 官方文档确认支持 canned ACL(public-read/private 二态"对象可见性"模型)。
            object_acl: true,
            // 只有 AWS S3 / 华为云 OBS 真支持按账号 ID 授权的对象级 ACL(已在各自 provider 里单独开启),其它厂商不建模。
            fine_grained_acl: false,
            presign: true,
            server_side_copy: true,
            hierarchical: false,
            // 官方文档确认支持生命周期规则(转换到冷存储 / 按期过期删除)。
            bucket_lifecycle: true,
            // 官方文档确认标准启用/暂停版本控制模型。
            versioning: true,
            // 官方文档确认支持 PUT/GET/DELETE ?cors 配置接口,和 Wasabi 的免配置模式不同。
            bucket_cors: true,
            // 官方文档确认支持 bucket 静态网站托管配置。
            bucket_website: true,
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

    fn provider() -> ScalewayProvider {
        ScalewayProvider::new("test", "ak", "sk", "s3.fr-par.scw.cloud")
    }

    #[test]
    fn region_parsed_from_endpoint() {
        let p = provider();
        assert_eq!(p.client.region(), "fr-par");
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
        // Scaleway 是目前支持面最广的 S3 兼容厂商,全部能力位都应为 true——和 Wasabi/B2
        // 那种"故意关掉几项"的矩阵刚好相反,单独断言避免复制粘贴时漏改。
        assert!(caps.presign);
        assert!(caps.bucket_lifecycle);
        assert!(caps.object_tagging);
        assert!(caps.versioning);
        assert!(caps.object_acl);
        assert!(caps.bucket_cors);
        assert!(caps.bucket_website);
        assert!(caps.storage_class_ops);
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
