//! # huawei-obs
//!
//! 手写的华为云 OBS(对象存储)异步 Rust SDK,不依赖任何聚合库。
//!
//! OBS 的签名体系与阿里云 OSS 高度同构(HMAC-SHA1 + base64 的 V2 风格 Header 签名),
//! 主要差异是 canonical 头前缀 `x-obs-`、授权词 `OBS`,以及
//! `obs.{region}.myhuaweicloud.com` 的 endpoint 域名。
//!
//! 当前增量提供:
//!
//! - [`client::ObsClient`] — 客户端构造与 endpoint 拼接
//! - [`sign`] — OBS 专有签名(HMAC-SHA1,Header 方式)
//! - [`object`] — 对象操作:put / get / delete / head / copy / 预签名
//! - [`error::ObsError`] — 错误类型
//!
//! 本 crate 只依赖 [`cloud_core`] 与 reqwest,不感知任何上层应用。

pub mod client;
pub mod error;
pub mod object;
pub mod sign;

pub use client::ObsClient;
pub use error::{ObsError, Result};
pub use object::ObjectMeta;
