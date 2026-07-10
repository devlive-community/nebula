//! App 层错误。

use thiserror::Error;

/// App 操作可能出现的错误。
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum AppError {
    /// 指定 id 的账号 / provider 未注册。
    #[error("no such provider: {0}")]
    NoSuchProvider(String),

    /// 来自底层 provider 的错误。
    #[error(transparent)]
    Provider(#[from] nebula_provider::ProviderError),

    /// 账号持久化(SQLite)错误。
    #[error("storage error: {0}")]
    Store(#[from] rusqlite::Error),

    /// 密钥存储(钥匙串)错误。
    #[error("secret store error: {0}")]
    Secret(#[from] keyring::Error),

    /// 本地文件读写错误(如断点续传上传时读本地分片)。
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// App 层 Result 别名。
pub type Result<T> = std::result::Result<T, AppError>;
