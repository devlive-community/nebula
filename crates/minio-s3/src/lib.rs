//! # minio-s3
//!
//! MinIO 的异步 Rust SDK,走 MinIO 的 S3 兼容 API + AWS Signature V4,路径风格访问。
//!
//! 通用 S3 逻辑复用共享 crate [`s3_core`];本 crate 是带 MinIO 品牌的门面。MinIO 的两点特殊:
//! 自建服务常用 **http + 自定义端口**(endpoint 带 `http://` 前缀即走明文),SigV4 的 region
//! **默认 `us-east-1`**(MinIO 默认)。因此**用 [`new_client`] 构造**。
//!
//! ```no_run
//! let client = minio_s3::new_client("ak", "sk", "http://minio.local:9000");
//! ```

pub use s3_core::{bucket, client, error, multipart, object};
pub use s3_core::{
    BucketSummary, ListEntry, ObjectMeta, ObjectSummary, Result, S3Client, S3Error, MIN_PART_SIZE,
};

/// 用 MinIO endpoint 创建客户端。region 默认 `us-east-1`;endpoint 以 `http://` 开头走明文,可带端口。
///
/// 若你的 MinIO 配了非默认 region,可再链式 [`S3Client::with_region`] 覆盖。
pub fn new_client(
    access_key: impl Into<String>,
    secret_key: impl Into<String>,
    endpoint: impl Into<String>,
) -> S3Client {
    S3Client::new(access_key, secret_key, endpoint).with_region("us-east-1")
}
