//! 把 [`digitalocean_spaces::S3Client`] 适配成统一的 [`StorageProvider`]。
//!
//! App 私有适配层(不发布)。DigitalOcean Spaces 走 S3 兼容 API + SigV4,路径遵循
//! `bucket/key` 约定,与 AWS/R2/MinIO/B2/Wasabi 等 S3 系适配层同构。Spaces 的 endpoint
//! (`{region}.digitaloceanspaces.com`)不带 `s3.` 前缀,构造函数必须走
//! [`digitalocean_spaces::new_client`](不是 `S3Client::new`),和 R2/MinIO 一样。
//!
//! 能力矩阵已对照 DigitalOcean 官方文档逐项核实:生命周期规则、CORS、预签名、版本控制
//! (标准启用/暂停语义)都支持;静态网站托管支持,但官方文档明确只对 Standard Storage 桶
//! 生效,Cold Storage 桶不支持,Nebula 无法提前判断桶类型,遇到会把后端报错透传;对象标签
//! 官方文档 / 第三方资料均未找到支持证据,标不支持;对象 ACL 官方文档明确只有 `private`/
//! `public-read` 两种 canned 值,正好和 Nebula 现有的 `set_object_acl(path, public: bool)`
//! 完全对上;存储类型转换不支持——Spaces 的"存储类型"是建桶时选 Standard 还是 Cold Storage
//! 的桶级属性,不是对象级可转换的 `x-amz-storage-class`,而且官方文档明确 CopyObject 在这
//! 两种桶之间不通,Nebula 的 `set_storage_class` 正是靠自我复制实现的,这条路走不通。

use async_trait::async_trait;
use bytes::Bytes;
use futures::StreamExt;

use digitalocean_spaces::{ListEntry, S3Client, S3Error};
use nebula_provider::{
    path, ByteStream, Capabilities, CorsRule, Entry, IncompleteUpload, LifecycleRule,
    ObjectVersion, ProgressFn, ProviderError, Result, StorageProvider, WebsiteConfig,
};

const MULTIPART_THRESHOLD: usize = 16 * 1024 * 1024;
const MULTIPART_PART_SIZE: usize = 8 * 1024 * 1024;

fn should_multipart(len: usize) -> bool {
    len > MULTIPART_THRESHOLD
}

/// DigitalOcean Spaces 的 provider 适配器。一个实例 = 一个账号。
pub struct DoSpacesProvider {
    id: String,
    client: S3Client,
}

impl DoSpacesProvider {
    /// 用账号别名与凭证创建。region 由 [`digitalocean_spaces::new_client`] 从 endpoint 推导。
    pub fn new(
        id: impl Into<String>,
        access_key: impl Into<String>,
        secret_key: impl Into<String>,
        endpoint: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            client: digitalocean_spaces::new_client(access_key, secret_key, endpoint),
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
fn rule_from_sdk(r: digitalocean_spaces::LifecycleRule) -> LifecycleRule {
    LifecycleRule {
        id: r.id,
        prefix: r.prefix,
        enabled: r.enabled,
        expiration_days: r.expiration_days,
        transitions: r.transitions,
    }
}

/// 把统一 provider 模型映射回 SDK 的生命周期规则。
fn rule_to_sdk(r: &LifecycleRule) -> digitalocean_spaces::LifecycleRule {
    digitalocean_spaces::LifecycleRule {
        id: r.id.clone(),
        prefix: r.prefix.clone(),
        enabled: r.enabled,
        expiration_days: r.expiration_days,
        transitions: r.transitions.clone(),
    }
}

/// 把 SDK 的 CORS 规则映射到统一 provider 模型(字段一一对应)。
fn cors_from_sdk(r: digitalocean_spaces::CorsRule) -> CorsRule {
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
fn cors_to_sdk(r: &CorsRule) -> digitalocean_spaces::CorsRule {
    digitalocean_spaces::CorsRule {
        id: r.id.clone(),
        allowed_origins: r.allowed_origins.clone(),
        allowed_methods: r.allowed_methods.clone(),
        allowed_headers: r.allowed_headers.clone(),
        expose_headers: r.expose_headers.clone(),
        max_age_seconds: r.max_age_seconds,
    }
}

/// 把 SDK 的静态网站托管配置映射到统一 provider 模型(字段一一对应)。
fn website_from_sdk(c: digitalocean_spaces::WebsiteConfig) -> WebsiteConfig {
    WebsiteConfig {
        index_document: c.index_document,
        error_document: c.error_document,
    }
}

/// 把统一 provider 模型映射回 SDK 的静态网站托管配置。
fn website_to_sdk(c: &WebsiteConfig) -> digitalocean_spaces::WebsiteConfig {
    digitalocean_spaces::WebsiteConfig {
        index_document: c.index_document.clone(),
        error_document: c.error_document.clone(),
    }
}

/// 把 SDK 的历史版本映射到统一 provider 模型(字段一一对应)。
fn version_from_sdk(v: digitalocean_spaces::ObjectVersion) -> ObjectVersion {
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
impl StorageProvider for DoSpacesProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            multipart_upload: true,
            resumable_upload: true,
            // Spaces 的"存储类型"是建桶时选 Standard/Cold 的桶级属性,不是对象级可转换的
            // x-amz-storage-class;官方文档明确 CopyObject 在两种桶之间不通,Nebula 的
            // set_storage_class 正是靠自我复制实现的,这条路走不通。
            storage_class_ops: false,
            bucket_ops: true,
            metadata_ops: true,
            // 官方文档 / 第三方资料均未找到 PutObjectTagging 支持的证据。
            object_tagging: false,
            multipart_cleanup: true,
            // 官方文档明确只有 private / public-read 两种 canned ACL,正好是 Nebula
            // set_object_acl(path, public: bool) 现在发送的两个值,没有额外限制。
            object_acl: true,
            // 只有 AWS S3 / 华为云 OBS 真支持按账号 ID 授权的对象级 ACL(已在各自 provider 里单独开启),其它厂商不建模。
            fine_grained_acl: false,
            presign: true,
            server_side_copy: true,
            hierarchical: false,
            // 官方文档确认支持(过期对象、清理未完成分片上传)。
            bucket_lifecycle: true,
            // 官方文档确认标准启用/暂停模型,语义和 Nebula 现有实现匹配。
            versioning: true,
            // 官方文档明确支持给 Space 配置 CORS,走标准接口。
            bucket_cors: true,
            // 官方文档确认支持静态网站托管,但只对 Standard Storage 桶生效——Cold Storage
            // 桶不支持,Nebula 无法提前判断桶类型,配置失败时把后端报错透传给用户。
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

    async fn bucket_website(&self, bucket: &str) -> Result<Option<WebsiteConfig>> {
        let config = self
            .client
            .get_bucket_website(bucket)
            .await
            .map_err(map_err)?;
        Ok(config.map(website_from_sdk))
    }

    async fn set_bucket_website(&self, bucket: &str, config: Option<&WebsiteConfig>) -> Result<()> {
        let config = config.map(website_to_sdk);
        self.client
            .set_bucket_website(bucket, config.as_ref())
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

    fn provider() -> DoSpacesProvider {
        DoSpacesProvider::new("test", "ak", "sk", "nyc3.digitaloceanspaces.com")
    }

    #[test]
    fn region_derived_from_endpoint_without_s3_prefix() {
        let p = provider();
        assert_eq!(p.client.region(), "nyc3");
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
        assert!(caps.bucket_website);
        assert!(caps.object_acl);
        assert!(caps.versioning);
        assert!(!caps.object_tagging);
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
