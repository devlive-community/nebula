//! OBS SDK 的错误类型。

use thiserror::Error;

/// 调用 OBS 时可能出现的错误。
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ObsError {
    /// 底层传输 / 编码 / 签名等公共错误。
    #[error(transparent)]
    Core(#[from] cloud_core::CoreError),

    /// OBS 服务端返回的业务错误(4xx/5xx 且响应体是 OBS 的 Error XML)。
    #[error("obs api error: {code} ({status}) - {message}")]
    Api {
        /// HTTP 状态码。
        status: u16,
        /// OBS 错误码,如 `NoSuchBucket`、`AccessDenied`。
        code: String,
        /// 可读错误信息。
        message: String,
        /// 便于排查的服务端请求 id。
        request_id: Option<String>,
    },
}

/// OBS SDK 的 Result 别名。
pub type Result<T> = std::result::Result<T, ObsError>;
