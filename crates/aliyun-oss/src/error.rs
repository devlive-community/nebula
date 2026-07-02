//! OSS SDK 的错误类型。

use thiserror::Error;

/// 调用 OSS 时可能出现的错误。
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum OssError {
    /// 底层传输 / 编码 / 签名等公共错误。
    #[error(transparent)]
    Core(#[from] cloud_core::CoreError),

    /// OSS 服务端返回的业务错误(4xx/5xx 且响应体是 OSS 的 Error XML)。
    #[error("oss api error: {code} ({status}) - {message}")]
    Api {
        /// HTTP 状态码。
        status: u16,
        /// OSS 错误码,如 `NoSuchBucket`、`AccessDenied`。
        code: String,
        /// 可读错误信息。
        message: String,
        /// 便于排查的服务端请求 id。
        request_id: Option<String>,
    },
}

/// OSS SDK 的 Result 别名。
pub type Result<T> = std::result::Result<T, OssError>;
