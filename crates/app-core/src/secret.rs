//! 敏感凭证存储抽象。
//!
//! AccessKeySecret 不落 SQLite,统一经 [`SecretStore`] 存取。真实运行用系统钥匙串
//! ([`KeyringSecrets`]);测试用内存实现([`MemorySecrets`]),避免依赖无头环境里
//! 不存在的钥匙串服务。
//!
//! ## 单条目存储(减少钥匙串弹窗)
//!
//! 所有账号的密钥合并成一个 JSON map,存进**同一条**钥匙串条目([`BLOB_KEY`]),并在首次
//! 读取后缓存到内存。这样启动加载 N 个账号只读钥匙串 1 次 → macOS 只弹 1 次确认(旧版按
//! 账号各存一条,启动会弹 N 次)。旧格式(每账号一条)在读取时自动迁移进合并条目。
//!
//! 注意:macOS 上"始终允许"是按 App 代码签名记忆的;未签名 / 每次重新构建的开发版每次启动
//! 仍会弹一次,需用固定 Developer ID 签名 + 公证后才不再弹。

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

/// 合并存储所有账号密钥的那条钥匙串条目名。
const BLOB_KEY: &str = "__nebula_secrets__";

/// 系统钥匙串实现(macOS Keychain / Windows Credential / Linux Secret Service)。
///
/// 所有密钥合并存进一条条目 [`BLOB_KEY`],首次读取后缓存,减少弹窗。
pub struct KeyringSecrets {
    service: String,
    /// 内存缓存;`None` 表示尚未从钥匙串加载。
    cache: Mutex<Option<HashMap<String, String>>>,
}

impl KeyringSecrets {
    pub fn new(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
            cache: Mutex::new(None),
        }
    }

    fn blob_entry(&self) -> Result<keyring::Entry> {
        Ok(keyring::Entry::new(&self.service, BLOB_KEY)?)
    }

    /// 旧格式:某账号单独一条条目(用于迁移)。
    fn legacy_entry(&self, account: &str) -> Result<keyring::Entry> {
        Ok(keyring::Entry::new(&self.service, account)?)
    }

    /// 确保合并条目已加载进缓存,返回缓存锁。整个进程只真正读钥匙串一次。
    fn load(&self) -> Result<std::sync::MutexGuard<'_, Option<HashMap<String, String>>>> {
        let mut guard = self.cache.lock().unwrap();
        if guard.is_none() {
            let map = match self.blob_entry()?.get_password() {
                Ok(json) => serde_json::from_str(&json).unwrap_or_default(),
                Err(keyring::Error::NoEntry) => HashMap::new(),
                Err(e) => return Err(e.into()),
            };
            *guard = Some(map);
        }
        Ok(guard)
    }

    /// 把缓存写回合并条目。
    fn persist(&self, map: &HashMap<String, String>) -> Result<()> {
        let json = serde_json::to_string(map).expect("string map is always serializable");
        self.blob_entry()?.set_password(&json)?;
        Ok(())
    }
}

impl SecretStore for KeyringSecrets {
    fn set(&self, account: &str, secret: &str) -> Result<()> {
        let mut guard = self.load()?;
        let map = guard.as_mut().unwrap();
        map.insert(account.to_string(), secret.to_string());
        self.persist(map)
    }

    fn get(&self, account: &str) -> Result<String> {
        let mut guard = self.load()?;
        let map = guard.as_mut().unwrap();
        if let Some(s) = map.get(account) {
            return Ok(s.clone());
        }
        // 迁移:合并条目里没有 → 回退读旧的"每账号一条",读到就搬进合并条目并删掉旧条目。
        match self.legacy_entry(account)?.get_password() {
            Ok(secret) => {
                map.insert(account.to_string(), secret.clone());
                self.persist(map)?;
                let _ = self.legacy_entry(account)?.delete_credential();
                Ok(secret)
            }
            Err(keyring::Error::NoEntry) => Err(keyring::Error::NoEntry.into()),
            Err(e) => Err(e.into()),
        }
    }

    fn delete(&self, account: &str) -> Result<()> {
        let mut guard = self.load()?;
        let map = guard.as_mut().unwrap();
        if map.remove(account).is_some() {
            self.persist(map)?;
        }
        // 也清掉可能残留的旧条目。
        let _ = self.legacy_entry(account)?.delete_credential();
        Ok(())
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
