//! 把 [`huawei_obs::ObsClient`] 适配成统一的 [`StorageProvider`]。
//!
//! 这是 App 私有的适配层(不发布):它是唯一同时依赖 `huawei-obs` 和
//! `nebula-provider` 的地方,从而让纯净 SDK 与 App 抽象彼此解耦。
//!
//! 路径遵循 [`StorageProvider`] 的 `bucket/key` 约定,解析复用
//! [`nebula_provider::path::split`]。

use async_trait::async_trait;
use bytes::Bytes;
use futures::StreamExt;

use huawei_obs::{ListEntry, ObsClient, ObsError};
use nebula_provider::{
    path, ByteStream, Capabilities, CorsRule, Entry, Grant, IncompleteUpload, LifecycleRule,
    ObjectVersion, Permission, ProgressFn, ProviderError, Result, StorageProvider, WebsiteConfig,
};

/// 把 SDK 的授权条目映射到统一 provider 模型;权限字符串未知(理论上不会发生,SDK 只会
/// 转发官方 XML 里出现过的值)时静默丢弃这一条,不让一条无法识别的历史授权拖垮整次读取。
fn grant_from_sdk(g: huawei_obs::multipart::AclGrant) -> Option<Grant> {
    Some(Grant {
        grantee_id: g.grantee_id,
        permission: Permission::parse_official(&g.permission)?,
    })
}

/// 把统一 provider 模型映射回 SDK 的授权条目。
fn grant_to_sdk(g: &Grant) -> huawei_obs::multipart::AclGrant {
    huawei_obs::multipart::AclGrant {
        grantee_id: g.grantee_id.clone(),
        permission: g.permission.as_str().to_string(),
    }
}

/// 超过该大小的上传自动改用分片上传。
const MULTIPART_THRESHOLD: usize = 16 * 1024 * 1024;
/// 分片上传时每片大小。
const MULTIPART_PART_SIZE: usize = 8 * 1024 * 1024;

/// 是否应对该大小的数据使用分片上传。
fn should_multipart(len: usize) -> bool {
    len > MULTIPART_THRESHOLD
}

/// 华为云 OBS 的 provider 适配器。一个实例 = 一个账号。
pub struct HuaweiProvider {
    id: String,
    client: ObsClient,
}

impl HuaweiProvider {
    /// 用账号别名与凭证创建。`id` 是注册表里的稳定标识(如账号别名)。
    pub fn new(
        id: impl Into<String>,
        access_key: impl Into<String>,
        secret_key: impl Into<String>,
        endpoint: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            client: ObsClient::new(access_key, secret_key, endpoint),
        }
    }

    /// 复用已构造好的 [`ObsClient`]。
    pub fn from_client(id: impl Into<String>, client: ObsClient) -> Self {
        Self {
            id: id.into(),
            client,
        }
    }
}

/// 要求路径能定位到一个对象(bucket + 非空 key),否则视为非法路径。
fn require_object(path: &str) -> Result<(&str, &str)> {
    match path::split(path) {
        (Some(bucket), key) if !key.is_empty() => Ok((bucket, key)),
        _ => Err(ProviderError::InvalidPath(path.to_string())),
    }
}

/// 把 SDK 的生命周期规则映射到统一 provider 模型(字段一一对应)。
fn rule_from_sdk(r: huawei_obs::LifecycleRule) -> LifecycleRule {
    LifecycleRule {
        id: r.id,
        prefix: r.prefix,
        enabled: r.enabled,
        expiration_days: r.expiration_days,
        transitions: r.transitions,
    }
}

/// 把 SDK 的历史版本映射到统一 provider 模型(字段一一对应)。
fn version_from_sdk(v: huawei_obs::ObjectVersion) -> ObjectVersion {
    ObjectVersion {
        version_id: v.version_id,
        is_latest: v.is_latest,
        is_delete_marker: v.is_delete_marker,
        size: v.size,
        etag: v.etag,
        last_modified: v.last_modified,
    }
}

/// 把统一 provider 模型映射回 SDK 的生命周期规则。
fn rule_to_sdk(r: &LifecycleRule) -> huawei_obs::LifecycleRule {
    huawei_obs::LifecycleRule {
        id: r.id.clone(),
        prefix: r.prefix.clone(),
        enabled: r.enabled,
        expiration_days: r.expiration_days,
        transitions: r.transitions.clone(),
    }
}

/// 把 SDK 的 CORS 规则映射到统一 provider 模型(字段一一对应)。
fn cors_from_sdk(r: huawei_obs::bucket::CorsRule) -> CorsRule {
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
fn cors_to_sdk(r: &CorsRule) -> huawei_obs::bucket::CorsRule {
    huawei_obs::bucket::CorsRule {
        id: r.id.clone(),
        allowed_origins: r.allowed_origins.clone(),
        allowed_methods: r.allowed_methods.clone(),
        allowed_headers: r.allowed_headers.clone(),
        expose_headers: r.expose_headers.clone(),
        max_age_seconds: r.max_age_seconds,
    }
}

/// 把 SDK 的静态网站托管配置映射到统一 provider 模型(字段一一对应)。
fn website_from_sdk(c: huawei_obs::bucket::WebsiteConfig) -> WebsiteConfig {
    WebsiteConfig {
        index_document: c.index_document,
        error_document: c.error_document,
    }
}

/// 把统一 provider 模型映射回 SDK 的静态网站托管配置。
fn website_to_sdk(c: &WebsiteConfig) -> huawei_obs::bucket::WebsiteConfig {
    huawei_obs::bucket::WebsiteConfig {
        index_document: c.index_document.clone(),
        error_document: c.error_document.clone(),
    }
}

/// 把 OBS 错误映射到统一 provider 错误。
fn map_err(err: ObsError) -> ProviderError {
    match err {
        ObsError::Api { code, message, .. } => match code.as_str() {
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
impl StorageProvider for HuaweiProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            multipart_upload: true,
            resumable_upload: true,
            storage_class_ops: true,
            bucket_ops: true,
            metadata_ops: true,
            object_tagging: true,
            multipart_cleanup: true,
            object_acl: true,
            // 华为云 OBS 官方支持按账号 ID(DomainId)授权对象级 ACL(GetObjectAcl/PutObjectAcl)。
            fine_grained_acl: true,
            presign: true,
            server_side_copy: true,
            hierarchical: false,
            bucket_lifecycle: true,
            versioning: true,
            bucket_cors: true,
            bucket_website: true,
        }
    }

    async fn list(&self, path: &str) -> Result<Vec<Entry>> {
        match path::split(path) {
            // 根:列出所有 bucket,每个作为目录。
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
            // 桶 / 前缀:按 "/" 折叠成一层目录,子前缀为目录、对象为文件。
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
            // 小文件单请求上传:开始 0%,成功后 100%。
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

    async fn set_storage_class(&self, path: &str, class: &str) -> Result<()> {
        let (bucket, key) = require_object(path)?;
        self.client
            .set_storage_class(bucket, key, class)
            .await
            .map_err(map_err)
    }

    async fn restore(&self, path: &str, days: u32) -> Result<()> {
        let (bucket, key) = require_object(path)?;
        self.client
            .restore_object(bucket, key, days)
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

    async fn object_grants(&self, path: &str) -> Result<Vec<Grant>> {
        let (bucket, key) = require_object(path)?;
        let grants = self
            .client
            .get_object_acl(bucket, key)
            .await
            .map_err(map_err)?;
        Ok(grants.into_iter().filter_map(grant_from_sdk).collect())
    }

    async fn set_object_grants(&self, path: &str, grants: &[Grant]) -> Result<()> {
        let (bucket, key) = require_object(path)?;
        let grants: Vec<_> = grants.iter().map(grant_to_sdk).collect();
        self.client
            .set_object_acl_grants(bucket, key, &grants)
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

    fn provider() -> HuaweiProvider {
        HuaweiProvider::new("test", "ak", "sk", "obs.cn-north-4.myhuaweicloud.com")
    }

    #[test]
    fn multipart_threshold_decision() {
        assert!(!should_multipart(0));
        assert!(!should_multipart(MULTIPART_THRESHOLD));
        assert!(should_multipart(MULTIPART_THRESHOLD + 1));
    }

    #[test]
    fn id_and_capabilities() {
        let p = provider();
        assert_eq!(p.id(), "test");
        assert!(p.capabilities().multipart_upload);
        assert!(p.capabilities().fine_grained_acl);
        assert!(!p.capabilities().hierarchical);
    }

    #[test]
    fn grant_round_trips_through_sdk_mapping() {
        let g = Grant {
            grantee_id: "acct-1".to_string(),
            permission: Permission::FullControl,
        };
        let sdk = grant_to_sdk(&g);
        assert_eq!(sdk.grantee_id, "acct-1");
        assert_eq!(sdk.permission, "FULL_CONTROL");
        assert_eq!(grant_from_sdk(sdk), Some(g));
    }

    #[test]
    fn grant_from_sdk_drops_unrecognized_permission_strings() {
        let sdk = huawei_obs::multipart::AclGrant {
            grantee_id: "acct-1".to_string(),
            permission: "BOGUS".to_string(),
        };
        assert_eq!(grant_from_sdk(sdk), None);
    }

    #[test]
    fn map_err_classifies_codes() {
        let not_found = map_err(ObsError::Api {
            status: 404,
            code: "NoSuchKey".into(),
            message: "missing".into(),
            request_id: None,
            endpoint: None,
        });
        assert!(matches!(not_found, ProviderError::NotFound(_)));

        let denied = map_err(ObsError::Api {
            status: 403,
            code: "AccessDenied".into(),
            message: "nope".into(),
            request_id: None,
            endpoint: None,
        });
        assert!(matches!(denied, ProviderError::AccessDenied(_)));

        let other = map_err(ObsError::Api {
            status: 400,
            code: "InvalidArgument".into(),
            message: "bad".into(),
            request_id: None,
            endpoint: None,
        });
        assert!(matches!(other, ProviderError::Backend(_)));
    }

    #[tokio::test]
    async fn object_ops_reject_rootless_paths() {
        let p = provider();
        // 无 bucket 或无 key 时,不发网络请求就应报 InvalidPath。
        assert!(matches!(
            p.read("").await,
            Err(ProviderError::InvalidPath(_))
        ));
        assert!(matches!(
            p.delete("bucket-only").await,
            Err(ProviderError::InvalidPath(_))
        ));
        assert!(matches!(
            p.write("bucket/", Bytes::new(), None).await,
            Err(ProviderError::InvalidPath(_))
        ));
    }

    #[tokio::test]
    async fn stat_on_bucket_returns_directory() {
        let p = provider();
        let entry = p.stat("mybucket").await.unwrap();
        assert!(entry.is_dir());
        assert_eq!(entry.name, "mybucket");
    }
}
