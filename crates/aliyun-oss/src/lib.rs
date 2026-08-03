//! # aliyun-oss
//!
//! 手写的阿里云 OSS(对象存储)异步 Rust SDK,不依赖任何聚合库。
//!
//! 当前增量提供:
//!
//! - [`client::OssClient`] — 客户端构造与 endpoint 拼接
//! - [`sign`] — OSS 专有签名(HMAC-SHA1,Header 方式)
//! - [`object`] — 对象操作:put / get / delete / head
//! - [`bucket`] — 桶操作:列举对象、列举 / 创建 / 删除 bucket(自动翻页)
//! - [`multipart`] — 分片上传(大文件)
//! - [`error::OssError`] — 错误类型
//!
//! 本 crate 只依赖 [`cloud_core`] 与 reqwest,不感知任何上层应用。

pub mod bucket;
pub mod client;
pub mod error;
pub mod multipart;
pub mod object;
pub mod sign;

pub use bucket::{BucketSummary, LifecycleRule, ListEntry, ObjectSummary};
pub use client::OssClient;
pub use error::{OssError, Result};
pub use object::ObjectMeta;
