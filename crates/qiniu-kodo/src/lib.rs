//! # qiniu-kodo
//!
//! 七牛云 Kodo(对象存储)异步 Rust SDK,走七牛的 **S3 兼容端点**
//! (`s3.{region}.qiniucs.com`),用 AWS Signature V4 签名。
//!
//! 通用的 S3 逻辑(对象 / 桶 / 列举 / 分片 / 签名)复用共享 crate [`s3_core`];本 crate 只是
//! 带七牛品牌的门面,把 [`S3Client`](此处别名 [`KodoClient`])直接透出。用法与其它厂商 SDK 一致:
//!
//! ```no_run
//! use qiniu_kodo::KodoClient;
//! let client = KodoClient::new("ak", "sk", "s3.cn-east-1.qiniucs.com");
//! ```

pub use s3_core::{bucket, client, error, multipart, object};
pub use s3_core::{BucketSummary, ListEntry, ObjectMeta, ObjectSummary, Result, MIN_PART_SIZE};

/// 七牛云 Kodo 客户端(即通用 [`s3_core::S3Client`])。
pub use s3_core::S3Client as KodoClient;
/// 七牛云 Kodo 错误类型(即通用 [`s3_core::S3Error`])。
pub use s3_core::S3Error as KodoError;
