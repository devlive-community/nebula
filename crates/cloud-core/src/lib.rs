//! # cloud-core
//!
//! 各家云原生 SDK 共用的底层积木。当前提供:
//!
//! - [`crypto`] — 签名所需的 HMAC / 摘要 / 编码原语
//! - [`error`] — 与厂商无关的基础错误类型
//! - [`http`] — 共享的 HTTP 客户端封装
//! - [`retry`] — 带指数退避的通用异步重试
//! - [`pagination`] — 把逐页拉取展开成元素流的通用分页
//!
//! 本 crate 不含任何厂商专有逻辑,也不依赖 App 层。

pub mod crypto;
pub mod error;
pub mod http;
pub mod pagination;
pub mod retry;

pub use error::{CoreError, Result};
pub use http::HttpClient;
pub use pagination::{paginate, Page};
pub use retry::{retry, RetryPolicy};
