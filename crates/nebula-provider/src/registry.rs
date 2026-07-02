//! provider 注册表:App 持有它,按 id 统一管理多个账号 / 多家云的 provider。

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use crate::provider::StorageProvider;

/// 线程安全、可 clone(共享同一份内部表)的 provider 注册表。
#[derive(Clone, Default)]
pub struct ProviderRegistry {
    inner: Arc<RwLock<HashMap<String, Arc<dyn StorageProvider>>>>,
}

impl ProviderRegistry {
    /// 新建空注册表。
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册(或按 id 覆盖)一个 provider。
    pub fn register(&self, provider: Arc<dyn StorageProvider>) {
        let id = provider.id().to_string();
        self.inner.write().unwrap().insert(id, provider);
    }

    /// 按 id 取出 provider。
    pub fn get(&self, id: &str) -> Option<Arc<dyn StorageProvider>> {
        self.inner.read().unwrap().get(id).cloned()
    }

    /// 按 id 移除 provider,返回是否存在过。
    pub fn remove(&self, id: &str) -> bool {
        self.inner.write().unwrap().remove(id).is_some()
    }

    /// 已注册的所有 id(字典序)。
    pub fn ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.inner.read().unwrap().keys().cloned().collect();
        ids.sort();
        ids
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capabilities::Capabilities;
    use crate::entry::Entry;
    use crate::error::{ProviderError, Result};
    use async_trait::async_trait;
    use bytes::Bytes;

    struct DummyProvider {
        id: String,
    }

    #[async_trait]
    impl StorageProvider for DummyProvider {
        fn id(&self) -> &str {
            &self.id
        }
        fn capabilities(&self) -> Capabilities {
            Capabilities::default()
        }
        async fn list(&self, _path: &str) -> Result<Vec<Entry>> {
            Ok(vec![])
        }
        async fn stat(&self, path: &str) -> Result<Entry> {
            Err(ProviderError::NotFound(path.into()))
        }
        async fn read(&self, path: &str) -> Result<Bytes> {
            Err(ProviderError::NotFound(path.into()))
        }
        async fn write(&self, _path: &str, _data: Bytes, _ct: Option<&str>) -> Result<()> {
            Ok(())
        }
        async fn delete(&self, _path: &str) -> Result<()> {
            Ok(())
        }
    }

    fn provider(id: &str) -> Arc<dyn StorageProvider> {
        Arc::new(DummyProvider { id: id.to_string() })
    }

    #[test]
    fn register_get_and_ids() {
        let reg = ProviderRegistry::new();
        reg.register(provider("aliyun-main"));
        reg.register(provider("tencent-backup"));

        assert!(reg.get("aliyun-main").is_some());
        assert!(reg.get("missing").is_none());
        assert_eq!(reg.ids(), vec!["aliyun-main", "tencent-backup"]);
    }

    #[test]
    fn register_overwrites_same_id() {
        let reg = ProviderRegistry::new();
        reg.register(provider("dup"));
        reg.register(provider("dup"));
        assert_eq!(reg.ids(), vec!["dup"]);
    }

    #[test]
    fn remove_reports_presence() {
        let reg = ProviderRegistry::new();
        reg.register(provider("x"));
        assert!(reg.remove("x"));
        assert!(!reg.remove("x"));
        assert!(reg.get("x").is_none());
    }
}
