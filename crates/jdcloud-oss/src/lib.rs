//! # jdcloud-oss
//!
//! 京东云对象存储(JD Cloud OSS)的异步 Rust SDK,走京东云的 S3 兼容 API + AWS Signature V4,
//! 是共享 crate [`s3_core`] 的品牌门面。
//!
//! 签名版本没有官方文档直接给出明确断言,但第三方 PHP 客户端(`jsuphp/jdcloud-oss`)的源码
//! 证实它内部直接实例化官方 `Aws\S3\S3Client` 并显式传 `'signature_version' => 'v4'` 指向
//! 京东云 endpoint——这是比"文档写了什么"更硬的证据:有人真的拿标准 AWS SDK 跑通过。
//!
//! endpoint 形如 `s3.{region}.jdcloud-oss.com`(比如 `s3.cn-south-1.jdcloud-oss.com`),和
//! AWS 的 `s3.{region}.amazonaws.com` 同形状,`S3Client::new` 能直接从中解析出 region,不
//! 需要专用构造函数。
//!
//! ```no_run
//! use jdcloud_oss::S3Client;
//! let client = S3Client::new("ak", "sk", "s3.cn-south-1.jdcloud-oss.com");
//! ```

pub use s3_core::{bucket, client, error, multipart, object};
pub use s3_core::{
    BucketSummary, CorsRule, LifecycleRule, ListEntry, ObjectMeta, ObjectSummary, ObjectVersion,
    Result, S3Client, S3Error, WebsiteConfig, MIN_PART_SIZE,
};
