//! 把 [`jdcloud_oss::S3Client`] 适配成统一的 [`StorageProvider`]。
//!
//! App 私有适配层(不发布)。京东云 OSS 走 S3 兼容 API + SigV4,路径遵循 `bucket/key` 约定,
//! 与 AWS/R2/MinIO/B2/Wasabi/DigitalOcean Spaces/Scaleway/US3 等 S3 系适配层同构。
//!
//! 官方文档没有直接断言签名版本,但第三方 PHP 客户端(`jsuphp/jdcloud-oss`)源码证实其内部
//! 直接用官方 `Aws\S3\S3Client` 配 `signature_version=v4` 指向京东云 endpoint——真实代码跑
//! 通过,比文档描述更硬的证据。endpoint(`s3.{region}.jdcloud-oss.com`)和 AWS 同形状,
//! `S3Client::new` 直接解析 region,不需要专用构造函数。
//!
//! 能力矩阵对照京东云官方产品功能文档逐项核实:生命周期管理、跨域访问设置(CORS)、静态网站
//! 托管、对象标签、存储类型转换、分片拷贝均有独立的官方文档页面,标支持;对象 ACL 官方
//! 三态(private/public-read/public-read-write)确认支持,是 Nebula 现有二态实现的超集
//! ("PutBucket 不支持 x-amz-acl 头"这条限制针对的是**建桶时**指定 ACL,和这里用的
//! `PutObjectAcl`(`?acl` 子资源,对象级)是不同的接口,不受影响)。**版本控制没有查到独立
//! 的官方文档页面确认**(产品功能列表里"跨区域复制"通常隐含依赖版本控制,但这是推断不是
//! 直接证据),保守标不支持——比"没查到证据就不敢开"的既有原则(B2/DO Spaces 对象标签
//! 就是这么处理的)更进一步,不能因为一个可能相关的功能存在就代入未直接证实的能力位。

use async_trait::async_trait;
use bytes::Bytes;
use futures::StreamExt;

use jdcloud_oss::{ListEntry, S3Client, S3Error};
use nebula_provider::{
    path, ByteStream, Capabilities, CorsRule, Entry, IncompleteUpload, LifecycleRule, ProgressFn,
    ProviderError, Result, StorageProvider, WebsiteConfig,
};

const MULTIPART_THRESHOLD: usize = 16 * 1024 * 1024;
const MULTIPART_PART_SIZE: usize = 8 * 1024 * 1024;

fn should_multipart(len: usize) -> bool {
    len > MULTIPART_THRESHOLD
}

/// 京东云 OSS 的 provider 适配器。一个实例 = 一个账号。
pub struct JdCloudProvider {
    id: String,
    client: S3Client,
}

impl JdCloudProvider {
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
fn rule_from_sdk(r: jdcloud_oss::LifecycleRule) -> LifecycleRule {
    LifecycleRule {
        id: r.id,
        prefix: r.prefix,
        enabled: r.enabled,
        expiration_days: r.expiration_days,
        transitions: r.transitions,
    }
}

/// 把统一 provider 模型映射回 SDK 的生命周期规则。
fn rule_to_sdk(r: &LifecycleRule) -> jdcloud_oss::LifecycleRule {
    jdcloud_oss::LifecycleRule {
        id: r.id.clone(),
        prefix: r.prefix.clone(),
        enabled: r.enabled,
        expiration_days: r.expiration_days,
        transitions: r.transitions.clone(),
    }
}

/// 把 SDK 的 CORS 规则映射到统一 provider 模型(字段一一对应)。
fn cors_from_sdk(r: jdcloud_oss::CorsRule) -> CorsRule {
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
fn cors_to_sdk(r: &CorsRule) -> jdcloud_oss::CorsRule {
    jdcloud_oss::CorsRule {
        id: r.id.clone(),
        allowed_origins: r.allowed_origins.clone(),
        allowed_methods: r.allowed_methods.clone(),
        allowed_headers: r.allowed_headers.clone(),
        expose_headers: r.expose_headers.clone(),
        max_age_seconds: r.max_age_seconds,
    }
}

/// 把 SDK 的静态网站托管配置映射到统一 provider 模型(字段一一对应)。
fn website_from_sdk(c: jdcloud_oss::WebsiteConfig) -> WebsiteConfig {
    WebsiteConfig {
        index_document: c.index_document,
        error_document: c.error_document,
    }
}

/// 把统一 provider 模型映射回 SDK 的静态网站托管配置。
fn website_to_sdk(c: &WebsiteConfig) -> jdcloud_oss::WebsiteConfig {
    jdcloud_oss::WebsiteConfig {
        index_document: c.index_document.clone(),
        error_document: c.error_document.clone(),
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
impl StorageProvider for JdCloudProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            multipart_upload: true,
            resumable_upload: true,
            // 官方产品功能文档有独立的"转换存储类型"页面。
            storage_class_ops: true,
            bucket_ops: true,
            metadata_ops: true,
            // 官方产品功能文档有独立的"对象标签"页面。
            object_tagging: true,
            multipart_cleanup: true,
            // 官方文档确认三态(private/public-read/public-read-write),是 Nebula 现有
            // set_object_acl(path, public: bool) 二态实现的超集。"PutBucket 不支持
            // x-amz-acl"的限制只影响建桶时指定 ACL,不影响这里用的 PutObjectAcl。
            object_acl: true,
            // 只有 AWS S3 / 华为云 OBS 真支持按账号 ID 授权的对象级 ACL(已在各自 provider 里单独开启),其它厂商不建模。
            fine_grained_acl: false,
            presign: true,
            // 官方产品功能文档有独立的"分片拷贝"页面,单次 CopyObject 是更基础的操作。
            server_side_copy: true,
            hierarchical: false,
            // 官方产品功能文档有独立的"生命周期管理"页面。
            bucket_lifecycle: true,
            // 没有查到独立的官方文档页面确认版本控制,保守不开。
            versioning: false,
            // 官方产品功能文档有独立的"跨域访问设置"页面。
            bucket_cors: true,
            // 官方产品功能文档有独立的"静态网站托管设置"页面。
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

    fn provider() -> JdCloudProvider {
        JdCloudProvider::new("test", "ak", "sk", "s3.cn-south-1.jdcloud-oss.com")
    }

    #[test]
    fn region_parsed_from_endpoint() {
        let p = provider();
        assert_eq!(p.client.region(), "cn-south-1");
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
        assert!(caps.object_tagging);
        assert!(caps.object_acl);
        assert!(caps.bucket_cors);
        assert!(caps.bucket_website);
        assert!(caps.storage_class_ops);
        // 没查到独立官方文档确认,和其它大多支持的能力位相反,单独断言避免复制粘贴时漏改。
        assert!(!caps.versioning);
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
