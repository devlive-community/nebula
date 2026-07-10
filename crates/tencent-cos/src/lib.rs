//! # tencent-cos
//!
//! 手写的腾讯云 COS(对象存储)异步 Rust SDK,不依赖任何聚合库。
//!
//! COS 请求/响应是 S3 风格,但**认证用腾讯专有签名**(`q-sign-algorithm=sha1`,基于 HMAC-SHA1),
//! 不是 AWS SigV4,因此不复用 s3-core,自带 [`sign`]。
//!
//! 当前增量提供:
//!
//! - [`client::CosClient`] — 客户端构造 + COS 签名请求器
//! - [`sign`] — COS 专有签名(HMAC-SHA1,`q-sign` 头)
//! - [`object`] — 对象操作:put / get / delete / head / copy / 预签名
//! - [`error::CosError`] — 错误类型
//!
//! 本 crate 只依赖 [`cloud_core`] 与 reqwest,不感知任何上层应用。

pub mod client;
pub mod error;
pub mod object;
pub mod sign;

pub use client::CosClient;
pub use error::{CosError, Result};
pub use object::ObjectMeta;
