//! 统一的 provider 错误。各厂商适配层把自家 `XxxError` 映射到这里,App 只认这一种。

use thiserror::Error;

/// 与厂商无关的存储错误。
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ProviderError {
    /// 目标对象 / 路径不存在。
    #[error("not found: {0}")]
    NotFound(String),

    /// 权限不足。
    #[error("access denied: {0}")]
    AccessDenied(String),

    /// 传入的路径不合法(如空路径、缺少 bucket)。
    #[error("invalid path: {0}")]
    InvalidPath(String),

    /// 该 provider 不支持此操作。
    #[error("unsupported: {0}")]
    Unsupported(String),

    /// 其他来自后端 SDK 的失败(携带原始信息)。
    #[error("backend error: {0}")]
    Backend(String),
}

/// provider 层的 Result 别名。
pub type Result<T> = std::result::Result<T, ProviderError>;
