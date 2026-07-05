//! 应用设置(持久化到 SQLite 的 settings 表)。

use serde::{Deserialize, Serialize};

/// 用户可调的应用设置。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// 分享(预签名)链接的有效期,单位秒。
    pub share_expiry_secs: u64,
    /// 批量传输的最大并发数。
    pub concurrency: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            share_expiry_secs: 3600,
            concurrency: 3,
        }
    }
}
