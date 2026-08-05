//! 把 [`upyun_uss::S3Client`] 适配成统一的 [`StorageProvider`]。
//!
//! App 私有适配层(不发布)。又拍云走 S3 兼容 API + SigV4,路径遵循 `bucket/key` 约定,与
//! AWS/R2/MinIO/B2/Wasabi/DigitalOcean Spaces/Scaleway/US3/京东云 等 S3 系适配层同构,但
//! **region 值未经证实**(见 `upyun-uss` crate 文档,硬编码 `us-east-1` 是推断值,需真实
//! 账号验证)。
//!
//! 能力矩阵按官方"S3 API"文档给出的**完整**支持操作列表收窄——这份列表明确只有文件相关的
//! 一部分(ListBuckets/ListObjectsV2/PutObject/GetObject/HeadObject/DeleteObject/
//! CopyObject/CreateMultipartUpload/UploadPart/UploadPartCopy/CompleteMultipartUpload/
//! AbortMultipartUpload/ListParts),**没有** CreateBucket/DeleteBucket、ListMultipartUploads
//! (bucket 级枚举未完成分片,和文档里的 ListParts 是不同接口)、任何 bucket 级管理接口
//! (CORS/生命周期/网站托管/版本控制)、对象标签、对象 ACL。这是目前接入的厂商里限制最多的
//! 一个,只保留最基础的对象读写与分片上传;`bucket_ops` 也标不支持,不提供新建/删除 bucket。

use async_trait::async_trait;
use bytes::Bytes;
use futures::StreamExt;

use nebula_provider::{
    path, ByteStream, Capabilities, Entry, ProgressFn, ProviderError, Result, StorageProvider,
};
use upyun_uss::{ListEntry, S3Client, S3Error};

const MULTIPART_THRESHOLD: usize = 16 * 1024 * 1024;
const MULTIPART_PART_SIZE: usize = 8 * 1024 * 1024;

fn should_multipart(len: usize) -> bool {
    len > MULTIPART_THRESHOLD
}

/// 又拍云的 provider 适配器。一个实例 = 一个账号。
pub struct UpyunProvider {
    id: String,
    client: S3Client,
}

impl UpyunProvider {
    /// 用账号别名与凭证创建。region 由 [`upyun_uss::new_client`] 硬编码为 `us-east-1`
    /// (未经证实的推断值)。
    pub fn new(
        id: impl Into<String>,
        access_key: impl Into<String>,
        secret_key: impl Into<String>,
        endpoint: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            client: upyun_uss::new_client(access_key, secret_key, endpoint),
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
impl StorageProvider for UpyunProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            // 官方文档确认 CreateMultipartUpload/UploadPart/CompleteMultipartUpload/
            // AbortMultipartUpload 全套支持。
            multipart_upload: true,
            resumable_upload: true,
            storage_class_ops: false,
            // 官方支持操作列表里没有 CreateBucket/DeleteBucket。
            bucket_ops: false,
            // CopyObject 支持,足够实现"自我复制换元数据"这条路径。
            metadata_ops: true,
            object_tagging: false,
            // 官方列表只有 ListParts(单个已知上传内枚举分片),没有 ListMultipartUploads
            // (bucket 级枚举所有未完成上传)——这是两个不同接口,后者才是这个能力位依赖的。
            multipart_cleanup: false,
            object_acl: false,
            presign: true,
            // 官方文档确认 CopyObject 支持。
            server_side_copy: true,
            hierarchical: false,
            bucket_lifecycle: false,
            versioning: false,
            bucket_cors: false,
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

    async fn set_content_type(&self, path: &str, content_type: &str) -> Result<()> {
        let (bucket, key) = require_object(path)?;
        self.client
            .set_content_type(bucket, key, content_type)
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

    fn provider() -> UpyunProvider {
        UpyunProvider::new("test", "ak", "sk", "s3.api.upyun.com")
    }

    #[test]
    fn region_hardcoded_since_endpoint_has_none() {
        let p = provider();
        assert_eq!(p.client.region(), "us-east-1");
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
        // 又拍云是目前限制最多的厂商:只保留最基础的对象读写与分片上传,其它全关。
        assert!(caps.multipart_upload);
        assert!(caps.server_side_copy);
        assert!(caps.presign);
        assert!(caps.metadata_ops);
        assert!(!caps.bucket_ops);
        assert!(!caps.object_tagging);
        assert!(!caps.object_acl);
        assert!(!caps.bucket_lifecycle);
        assert!(!caps.bucket_cors);
        assert!(!caps.bucket_website);
        assert!(!caps.versioning);
        assert!(!caps.storage_class_ops);
        assert!(!caps.multipart_cleanup);
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
