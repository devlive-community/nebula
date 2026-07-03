//! 敏感凭证存储抽象。
//!
//! AccessKeySecret 不落 SQLite,统一经 [`SecretStore`] 存取。真实运行用系统钥匙串
//! ([`KeyringSecrets`]);测试用内存实现([`MemorySecrets`]),避免依赖无头环境里
//! 不存在的钥匙串服务。

use std::collections::HashMap;
use std::sync::Mutex;

use crate::error::Result;

/// 按账号 id 存取密钥。
pub trait SecretStore: Send + Sync {
    /// 保存(或覆盖)某账号的密钥。
    fn set(&self, account: &str, secret: &str) -> Result<()>;
    /// 读取某账号的密钥;不存在时返回 [`keyring::Error::NoEntry`]。
    fn get(&self, account: &str) -> Result<String>;
    /// 删除某账号的密钥;不存在视为成功。
    fn delete(&self, account: &str) -> Result<()>;
}

/// 系统钥匙串实现(macOS Keychain / Windows Credential / Linux Secret Service)。
pub struct KeyringSecrets {
    service: String,
}

impl KeyringSecrets {
    pub fn new(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
        }
    }

    fn entry(&self, account: &str) -> Result<keyring::Entry> {
        Ok(keyring::Entry::new(&self.service, account)?)
    }
}

impl SecretStore for KeyringSecrets {
    fn set(&self, account: &str, secret: &str) -> Result<()> {
        self.entry(account)?.set_password(secret)?;
        Ok(())
    }

    fn get(&self, account: &str) -> Result<String> {
        Ok(self.entry(account)?.get_password()?)
    }

    fn delete(&self, account: &str) -> Result<()> {
        match self.entry(account)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}

/// 进程内内存实现。用于测试,或不需要持久化密钥的场景。
#[derive(Default)]
pub struct MemorySecrets {
    map: Mutex<HashMap<String, String>>,
}

impl SecretStore for MemorySecrets {
    fn set(&self, account: &str, secret: &str) -> Result<()> {
        self.map
            .lock()
            .unwrap()
            .insert(account.to_string(), secret.to_string());
        Ok(())
    }

    fn get(&self, account: &str) -> Result<String> {
        self.map
            .lock()
            .unwrap()
            .get(account)
            .cloned()
            .ok_or_else(|| keyring::Error::NoEntry.into())
    }

    fn delete(&self, account: &str) -> Result<()> {
        self.map.lock().unwrap().remove(account);
        Ok(())
    }
}
