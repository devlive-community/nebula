//! # aliyun-oss
//!
//! 手写的阿里云 OSS(对象存储)异步 Rust SDK,不依赖任何聚合库。
//!
//! 当前增量提供:
//!
//! - [`client::OssClient`] — 客户端构造与 endpoint 拼接
//! - [`sign`] — OSS 专有签名(HMAC-SHA1,Header 方式)
//! - [`object`] — 对象操作:put / get / delete / head
//! - [`error::OssError`] — 错误类型
//!
//! 桶管理、分片上传等操作在后续增量加入。
//!
//! 本 crate 只依赖 [`cloud_core`] 与 reqwest,不感知任何上层应用。

pub mod client;
pub mod error;
pub mod object;
pub mod sign;

pub use client::OssClient;
pub use error::{OssError, Result};
pub use object::ObjectMeta;
