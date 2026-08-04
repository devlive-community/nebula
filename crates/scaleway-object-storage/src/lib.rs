//! # scaleway-object-storage
//!
//! Scaleway Object Storage 的异步 Rust SDK,走标准 S3 API + AWS Signature V4,路径风格访问。
//!
//! 通用 S3 逻辑复用共享 crate [`s3_core`];本 crate 是带 Scaleway 品牌的门面。Scaleway 的
//! endpoint 形如 `s3.{region}.scw.cloud`(region 形如 `fr-par`、`nl-ams`、`pl-waw`、
//! `it-mil`),和 AWS 的 `s3.{region}.amazonaws.com` 是同一种形状,`S3Client::new` 能直接从中
//! 解析出 region,不需要像 R2/MinIO 那样另写一个强制 region 的构造函数。
//!
//! ```no_run
//! use scaleway_object_storage::S3Client;
//! let client = S3Client::new("ak", "sk", "s3.fr-par.scw.cloud");
//! ```

pub use s3_core::{bucket, client, error, multipart, object};
pub use s3_core::{
    BucketSummary, CorsRule, LifecycleRule, ListEntry, ObjectMeta, ObjectSummary, ObjectVersion,
    Result, S3Client, S3Error, WebsiteConfig, MIN_PART_SIZE,
};
