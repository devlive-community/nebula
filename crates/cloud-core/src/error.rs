//! `cloud-core` 层的基础错误类型。
//!
//! 各厂商 SDK 通常定义自己的 `XxxError`,并在需要时从这里的 `CoreError` 转换 /
//! 包装底层(传输、编码)失败。这里只覆盖与具体厂商无关的通用失败。

use thiserror::Error;

/// 与厂商无关的底层错误。
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum CoreError {
    /// 构造请求(URL / 头 / 签名输入)时的失败。
    #[error("invalid request: {0}")]
    InvalidRequest(String),

    /// 解析响应体失败(如预期 XML/JSON 但格式不符)。
    #[error("failed to parse response: {0}")]
    InvalidResponse(String),

    /// 签名过程本身出错。
    #[error("signature error: {0}")]
    Signature(String),

    /// 底层 HTTP 传输失败(连接、超时、TLS 等)。
    #[error("http transport error: {0}")]
    Transport(#[from] reqwest::Error),
}

/// `cloud-core` 内部便捷 Result 别名。
pub type Result<T> = std::result::Result<T, CoreError>;
