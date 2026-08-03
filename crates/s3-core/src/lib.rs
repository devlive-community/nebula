//! # s3-core
//!
//! 通用的 **S3 兼容对象存储**异步客户端。用 AWS SigV4(复用 [`s3_sigv4`])签名、路径风格访问,
//! 覆盖对象读写 / 桶管理 / 列举分页 / 分片上传。
//!
//! 各家"S3 兼容"云只是 endpoint 域名不同(七牛 `qiniucs.com`、AWS `amazonaws.com`、
//! Cloudflare R2、MinIO 自建域名……),因此都能复用本 crate:厂商 crate(`qiniu-kodo`、
//! `aws-s3` 等)只做一层带品牌的门面,把 endpoint / region 传进来即可。
//!
//! - [`client::S3Client`] — 客户端构造(region 从 `s3.{region}.*` 解析)
//! - [`object`] — put / get / delete / head / copy / 预签名
//! - [`bucket`] — 列举对象、列举 / 创建 / 删除 bucket(自动翻页)
//! - [`multipart`] — 分片上传
//! - [`error::S3Error`] — 错误类型

pub mod bucket;
pub mod client;
pub mod error;
pub mod multipart;
pub mod object;

pub use bucket::{BucketSummary, LifecycleRule, ListEntry, ObjectSummary};
pub use client::S3Client;
pub use error::{Result, S3Error};
pub use multipart::MIN_PART_SIZE;
pub use object::ObjectMeta;
