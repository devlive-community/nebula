//! # aws-s3
//!
//! AWS S3 的异步 Rust SDK,走 S3 REST API + AWS Signature V4,路径风格访问。
//!
//! 通用 S3 逻辑复用共享 crate [`s3_core`];本 crate 是带 AWS 品牌的门面。endpoint 用区域型
//! `s3.{region}.amazonaws.com`(region 会从中解析);`us-east-1` 的旧全局域名 `s3.amazonaws.com`
//! 解析不出 region,请显式 [`S3Client::with_region`]。
//!
//! ```no_run
//! use aws_s3::S3Client;
//! let client = S3Client::new("ak", "sk", "s3.us-west-2.amazonaws.com");
//! ```

pub use s3_core::{bucket, client, error, multipart, object};
pub use s3_core::{
    AclGrant, BucketSummary, CorsRule, LifecycleRule, ListEntry, ObjectMeta, ObjectSummary,
    ObjectVersion, Result, S3Client, S3Error, WebsiteConfig, MIN_PART_SIZE,
};
