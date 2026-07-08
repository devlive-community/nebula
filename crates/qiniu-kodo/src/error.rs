//! Kodo SDK 的错误类型。

use thiserror::Error;

/// 调用 Kodo(S3 兼容)时可能出现的错误。
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum KodoError {
    /// 底层传输 / 编码 / 签名等公共错误。
    #[error(transparent)]
    Core(#[from] cloud_core::CoreError),

    /// 服务端返回的业务错误(4xx/5xx 且响应体是 S3 的 Error XML)。
    #[error("kodo api error: {code} ({status}) - {message}")]
    Api {
        /// HTTP 状态码。
        status: u16,
        /// S3 错误码,如 `NoSuchKey`、`AccessDenied`。
        code: String,
        /// 可读错误信息。
        message: String,
        /// 便于排查的服务端请求 id。
        request_id: Option<String>,
    },
}

/// Kodo SDK 的 Result 别名。
pub type Result<T> = std::result::Result<T, KodoError>;
