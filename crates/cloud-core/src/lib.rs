//! # cloud-core
//!
//! 各家云原生 SDK 共用的底层积木。当前提供:
//!
//! - [`crypto`] — 签名所需的 HMAC / 摘要 / 编码原语
//! - [`error`] — 与厂商无关的基础错误类型
//!
//! 后续增量会加入 HTTP 客户端封装、重试与分页迭代器。
//!
//! 本 crate 不含任何厂商专有逻辑,也不依赖 App 层。

pub mod crypto;
pub mod error;

pub use error::{CoreError, Result};
