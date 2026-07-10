//! 把 [`aliyun_oss::OssClient`] 适配成统一的 [`StorageProvider`]。
//!
//! 这是 App 私有的适配层(不发布):它是唯一同时依赖 `aliyun-oss` 和
//! `nebula-provider` 的地方,从而让纯净 SDK 与 App 抽象彼此解耦。
//!
//! 路径遵循 [`StorageProvider`] 的 `bucket/key` 约定,解析复用
//! [`nebula_provider::path::split`]。

use async_trait::async_trait;
use bytes::Bytes;
use futures::StreamExt;

use aliyun_oss::{ListEntry, OssClient, OssError};
use nebula_provider::{
    path, ByteStream, Capabilities, Entry, ProgressFn, ProviderError, Result, StorageProvider,
};

/// 超过该大小的上传自动改用分片上传。
const MULTIPART_THRESHOLD: usize = 16 * 1024 * 1024;
/// 分片上传时每片大小。
const MULTIPART_PART_SIZE: usize = 8 * 1024 * 1024;

/// 是否应对该大小的数据使用分片上传。
fn should_multipart(len: usize) -> bool {
    len > MULTIPART_THRESHOLD
}

/// 阿里云 OSS 的 provider 适配器。一个实例 = 一个账号。
pub struct AliyunProvider {
    id: String,
    client: OssClient,
}

impl AliyunProvider {
    /// 用账号别名与凭证创建。`id` 是注册表里的稳定标识(如账号别名)。
    pub fn new(
        id: impl Into<String>,
        access_key_id: impl Into<String>,
        access_key_secret: impl Into<String>,
        endpoint: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            client: OssClient::new(access_key_id, access_key_secret, endpoint),
        }
    }

    /// 复用已构造好的 [`OssClient`]。
    pub fn from_client(id: impl Into<String>, client: OssClient) -> Self {
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

/// 把 OSS 错误映射到统一 provider 错误。
fn map_err(err: OssError) -> ProviderError {
    match err {
        OssError::Api { code, message, .. } => match code.as_str() {
            "NoSuchKey" | "NoSuchBucket" | "SymlinkTargetNotExist" => {
                ProviderError::NotFound(message)
            }
            "AccessDenied" | "InvalidAccessKeyId" | "SignatureDoesNotMatch" => {
                ProviderError::AccessDenied(message)
            }
            _ => ProviderError::Backend(format!("{code}: {message}")),
        },
        other => ProviderError::Backend(other.to_string()),
    }
}

#[async_trait]
impl StorageProvider for AliyunProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            multipart_upload: true,
            presign: true,
            server_side_copy: true,
            hierarchical: false,
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
                                    .with_last_modified(obj.last_modified),
                            );
                        }
                    }
                }
                Ok(entries)
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider() -> AliyunProvider {
        AliyunProvider::new("test", "ak", "sk", "oss-cn-hangzhou.aliyuncs.com")
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
        assert!(!p.capabilities().hierarchical);
    }

    #[test]
    fn map_err_classifies_codes() {
        let not_found = map_err(OssError::Api {
            status: 404,
            code: "NoSuchKey".into(),
            message: "missing".into(),
            request_id: None,
        });
        assert!(matches!(not_found, ProviderError::NotFound(_)));

        let denied = map_err(OssError::Api {
            status: 403,
            code: "AccessDenied".into(),
            message: "nope".into(),
            request_id: None,
        });
        assert!(matches!(denied, ProviderError::AccessDenied(_)));

        let other = map_err(OssError::Api {
            status: 400,
            code: "InvalidArgument".into(),
            message: "bad".into(),
            request_id: None,
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
