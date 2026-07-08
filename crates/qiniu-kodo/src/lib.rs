//! # qiniu-kodo
//!
//! 手写的七牛云 Kodo(对象存储)异步 Rust SDK,走七牛的 **S3 兼容端点**,
//! 用 **AWS Signature V4** 签名,不依赖任何聚合库。
//!
//! 与阿里云 OSS / 华为云 OBS 的差异:路径风格访问(bucket 在 path)、SigV4 签名体系
//! (SHA256 链,不是 V2 的 HMAC-SHA1)、endpoint 为 `s3.{region}.qiniucs.com`。
//!
//! 当前增量提供:
//!
//! - [`client::KodoClient`] — 客户端构造 + SigV4 请求签名
//! - [`sign`] — AWS Signature V4 签名
//! - [`object`] — 对象操作:put / get / delete / head / copy / 预签名
//! - [`error::KodoError`] — 错误类型
//!
//! 本 crate 只依赖 [`cloud_core`] 与 reqwest,不感知任何上层应用。

pub mod client;
pub mod error;
pub mod object;
pub mod sign;

pub use client::KodoClient;
pub use error::{KodoError, Result};
pub use object::ObjectMeta;
