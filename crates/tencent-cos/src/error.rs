//! COS SDK 的错误类型。

use thiserror::Error;

/// 调用 COS 时可能出现的错误。
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum CosError {
    /// 底层传输 / 编码 / 签名等公共错误。
    #[error(transparent)]
    Core(#[from] cloud_core::CoreError),

    /// COS 服务端返回的业务错误(4xx/5xx 且响应体是 COS 的 Error XML)。
    #[error("cos api error: {code} ({status}) - {message}")]
    Api {
        /// HTTP 状态码。
        status: u16,
        /// COS 错误码,如 `NoSuchKey`、`AccessDenied`。
        code: String,
        /// 可读错误信息。
        message: String,
        /// 便于排查的服务端请求 id。
        request_id: Option<String>,
        /// 跨区域错误里服务端给出的正确 endpoint(若有,用于自动纠正)。
        endpoint: Option<String>,
    },
}

/// COS SDK 的 Result 别名。
pub type Result<T> = std::result::Result<T, CosError>;
