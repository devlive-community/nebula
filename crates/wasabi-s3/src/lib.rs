//! # wasabi-s3
//!
//! Wasabi 的异步 Rust SDK,走 Wasabi 的 S3 兼容 API + AWS Signature V4,路径风格访问。
//!
//! 通用 S3 逻辑复用共享 crate [`s3_core`];本 crate 是带 Wasabi 品牌的门面。Wasabi 的 endpoint
//! 形如 `s3.{region}.wasabisys.com`(region 形如 `us-west-1`),和 AWS 的
//! `s3.{region}.amazonaws.com` 是同一种形状,`S3Client::new` 能直接从中解析出 region,不需要
//! 像 R2/MinIO 那样另写一个强制 region 的构造函数。
//!
//! ```no_run
//! use wasabi_s3::S3Client;
//! let client = S3Client::new("ak", "sk", "s3.us-west-1.wasabisys.com");
//! ```

pub use s3_core::{bucket, client, error, multipart, object};
pub use s3_core::{
    BucketSummary, CorsRule, LifecycleRule, ListEntry, ObjectMeta, ObjectSummary, ObjectVersion,
    Result, S3Client, S3Error, WebsiteConfig, MIN_PART_SIZE,
};
