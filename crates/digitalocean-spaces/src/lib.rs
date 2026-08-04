//! # digitalocean-spaces
//!
//! DigitalOcean Spaces 的异步 Rust SDK,走 Spaces 的 S3 兼容 API + AWS Signature V4。
//!
//! 通用 S3 逻辑复用共享 crate [`s3_core`];本 crate 是带 Spaces 品牌的门面。Spaces 的 endpoint
//! 形如 `{region}.digitaloceanspaces.com`(比如 `nyc3.digitaloceanspaces.com`)——**不带**
//! `s3.` 前缀,和 AWS/B2/Wasabi 那种 `s3.{region}.xxx.com` 形状不一样,`S3Client::new` 内置的
//! region 解析(只认 `s3.` 前缀)在这里会失败。因此**用 [`new_client`] 构造**,不要直接
//! `S3Client::new`——它会从 endpoint 的第一个 `.` 之前取 region 并显式设置。
//!
//! ```no_run
//! let client = digitalocean_spaces::new_client("ak", "sk", "nyc3.digitaloceanspaces.com");
//! ```

pub use s3_core::{bucket, client, error, multipart, object};
pub use s3_core::{
    BucketSummary, CorsRule, LifecycleRule, ListEntry, ObjectMeta, ObjectSummary, ObjectVersion,
    Result, S3Client, S3Error, WebsiteConfig, MIN_PART_SIZE,
};

/// 用 Spaces endpoint 创建客户端。region 从 endpoint 第一个 `.` 之前的片段推导
/// (`nyc3.digitaloceanspaces.com` → `nyc3`),因为 Spaces 的 endpoint 不带 `s3.` 前缀,
/// `S3Client::new` 内置的 region 解析在这里解析不出来。
pub fn new_client(
    access_key: impl Into<String>,
    secret_key: impl Into<String>,
    endpoint: impl Into<String>,
) -> S3Client {
    let client = S3Client::new(access_key, secret_key, endpoint);
    let region = client
        .endpoint()
        .split('.')
        .next()
        .unwrap_or("")
        .to_string();
    client.with_region(region)
}
