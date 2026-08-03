//! # cloudflare-r2
//!
//! Cloudflare R2 的异步 Rust SDK,走 R2 的 S3 兼容 API + AWS Signature V4。
//!
//! 通用 S3 逻辑复用共享 crate [`s3_core`];本 crate 是带 R2 品牌的门面。R2 的两点特殊:
//! endpoint 形如 `{account_id}.r2.cloudflarestorage.com`(不含区域),SigV4 的 region **固定为
//! `auto`**。因此**用 [`new_client`] 构造**(它会把 region 设成 `auto`),不要直接 `S3Client::new`。
//!
//! ```no_run
//! let client = cloudflare_r2::new_client("ak", "sk", "<account_id>.r2.cloudflarestorage.com");
//! ```

pub use s3_core::{bucket, client, error, multipart, object};
pub use s3_core::{
    BucketSummary, CorsRule, LifecycleRule, ListEntry, ObjectMeta, ObjectSummary, ObjectVersion,
    Result, S3Client, S3Error, WebsiteConfig, MIN_PART_SIZE,
};

/// 用 R2 endpoint 创建客户端。R2 的 SigV4 region 固定为 `auto`,这里自动设好。
pub fn new_client(
    access_key: impl Into<String>,
    secret_key: impl Into<String>,
    endpoint: impl Into<String>,
) -> S3Client {
    S3Client::new(access_key, secret_key, endpoint).with_region("auto")
}
