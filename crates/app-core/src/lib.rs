//! # app-core
//!
//! Nebula 的**框架无关**业务逻辑层。它持有一个 [`ProviderRegistry`],对上暴露以
//! "账号 id + 路径"为参数的高层操作(浏览 / 上传 / 下载 / 删除),对下依赖统一的
//! [`StorageProvider`] 抽象。
//!
//! Tauri 外壳(`app/src-tauri`)只是把这里的方法包成 `#[tauri::command]`,因此这些
//! 逻辑可以完全用 `cargo test` 覆盖,无需启动 GUI。

mod error;
mod store;

use std::sync::Arc;

use bytes::Bytes;
use nebula_provider::{Entry, ProviderRegistry, StorageProvider};
use provider_aliyun::AliyunProvider;

pub use error::{AppError, Result};
pub use nebula_provider::{Capabilities, EntryKind};
pub use store::{AccountRecord, AccountStore};

const VENDOR_ALIYUN: &str = "aliyun";

/// App 的核心状态与操作入口。可低成本 clone(共享注册表与存储)。
#[derive(Clone, Default)]
pub struct App {
    registry: ProviderRegistry,
    /// 持久化存储;`None` 时账号仅存在内存(用于测试)。
    store: Option<Arc<AccountStore>>,
}

impl App {
    /// 新建一个不持久化的空 App(账号仅存内存)。
    pub fn new() -> Self {
        Self::default()
    }

    /// 用 SQLite 库路径创建 App,并把库里已存的账号加载注册。
    pub fn with_store(db_path: impl AsRef<std::path::Path>) -> Result<Self> {
        let store = AccountStore::open(db_path)?;
        let app = App {
            registry: ProviderRegistry::new(),
            store: Some(Arc::new(store)),
        };
        app.load_persisted()?;
        Ok(app)
    }

    /// 把存储里的账号构造成 provider 并注册。
    fn load_persisted(&self) -> Result<()> {
        let Some(store) = &self.store else {
            return Ok(());
        };
        for rec in store.list()? {
            self.register_record(rec);
        }
        Ok(())
    }

    /// 按厂商把一条记录注册为 provider(未知厂商忽略)。
    fn register_record(&self, rec: AccountRecord) {
        if rec.vendor == VENDOR_ALIYUN {
            self.registry.register(Arc::new(AliyunProvider::new(
                rec.id,
                rec.access_key_id,
                rec.access_key_secret,
                rec.endpoint,
            )));
        }
    }

    /// 注册一个账号 / provider(以 `provider.id()` 为键)。不持久化。
    pub fn add_account(&self, provider: Arc<dyn StorageProvider>) {
        self.registry.register(provider);
    }

    /// 便捷:新增一个阿里云 OSS 账号(注册并持久化)。
    pub fn add_aliyun_account(
        &self,
        id: impl Into<String>,
        access_key_id: impl Into<String>,
        access_key_secret: impl Into<String>,
        endpoint: impl Into<String>,
    ) -> Result<()> {
        let rec = AccountRecord {
            id: id.into(),
            vendor: VENDOR_ALIYUN.to_string(),
            access_key_id: access_key_id.into(),
            access_key_secret: access_key_secret.into(),
            endpoint: endpoint.into(),
        };
        if let Some(store) = &self.store {
            store.upsert(&rec)?;
        }
        self.register_record(rec);
        Ok(())
    }

    /// 移除一个账号(注册表 + 存储),返回它是否存在过。
    pub fn remove_account(&self, id: &str) -> Result<bool> {
        if let Some(store) = &self.store {
            store.delete(id)?;
        }
        Ok(self.registry.remove(id))
    }

    /// 列出已注册的账号 id(字典序)。
    pub fn accounts(&self) -> Vec<String> {
        self.registry.ids()
    }

    /// 浏览某账号下某路径(桶 / 前缀)的条目。
    pub async fn browse(&self, account: &str, path: &str) -> Result<Vec<Entry>> {
        Ok(self.provider(account)?.list(path).await?)
    }

    /// 读取某路径的元信息。
    pub async fn stat(&self, account: &str, path: &str) -> Result<Entry> {
        Ok(self.provider(account)?.stat(path).await?)
    }

    /// 下载对象内容。
    pub async fn download(&self, account: &str, path: &str) -> Result<Bytes> {
        Ok(self.provider(account)?.read(path).await?)
    }

    /// 上传 / 覆盖对象。
    pub async fn upload(
        &self,
        account: &str,
        path: &str,
        data: Bytes,
        content_type: Option<&str>,
    ) -> Result<()> {
        Ok(self
            .provider(account)?
            .write(path, data, content_type)
            .await?)
    }

    /// 删除对象。
    pub async fn delete(&self, account: &str, path: &str) -> Result<()> {
        Ok(self.provider(account)?.delete(path).await?)
    }

    /// 按 id 解析 provider,未注册则报 [`AppError::NoSuchProvider`]。
    fn provider(&self, account: &str) -> Result<Arc<dyn StorageProvider>> {
        self.registry
            .get(account)
            .ok_or_else(|| AppError::NoSuchProvider(account.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use nebula_provider::{Capabilities, EntryKind, ProviderError};
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// 内存版 provider,用于离线端到端测试 App 逻辑。路径即完整 key。
    struct MemoryProvider {
        id: String,
        store: Mutex<HashMap<String, Bytes>>,
    }

    impl MemoryProvider {
        fn new(id: &str) -> Self {
            Self {
                id: id.to_string(),
                store: Mutex::new(HashMap::new()),
            }
        }
    }

    #[async_trait]
    impl StorageProvider for MemoryProvider {
        fn id(&self) -> &str {
            &self.id
        }
        fn capabilities(&self) -> Capabilities {
            Capabilities::default()
        }
        async fn list(&self, _path: &str) -> nebula_provider::Result<Vec<Entry>> {
            let store = self.store.lock().unwrap();
            Ok(store
                .iter()
                .map(|(k, v)| Entry::file(k.clone(), v.len() as u64))
                .collect())
        }
        async fn stat(&self, path: &str) -> nebula_provider::Result<Entry> {
            let store = self.store.lock().unwrap();
            store
                .get(path)
                .map(|v| Entry::file(path.to_string(), v.len() as u64))
                .ok_or_else(|| ProviderError::NotFound(path.to_string()))
        }
        async fn read(&self, path: &str) -> nebula_provider::Result<Bytes> {
            let store = self.store.lock().unwrap();
            store
                .get(path)
                .cloned()
                .ok_or_else(|| ProviderError::NotFound(path.to_string()))
        }
        async fn write(
            &self,
            path: &str,
            data: Bytes,
            _ct: Option<&str>,
        ) -> nebula_provider::Result<()> {
            self.store.lock().unwrap().insert(path.to_string(), data);
            Ok(())
        }
        async fn delete(&self, path: &str) -> nebula_provider::Result<()> {
            self.store.lock().unwrap().remove(path);
            Ok(())
        }
    }

    fn app_with_memory() -> App {
        let app = App::new();
        app.add_account(Arc::new(MemoryProvider::new("mem")));
        app
    }

    #[tokio::test]
    async fn upload_download_roundtrip() {
        let app = app_with_memory();
        let data = Bytes::from_static(b"hello app-core");
        app.upload("mem", "b/k.txt", data.clone(), Some("text/plain"))
            .await
            .unwrap();

        let got = app.download("mem", "b/k.txt").await.unwrap();
        assert_eq!(got, data);

        let meta = app.stat("mem", "b/k.txt").await.unwrap();
        assert_eq!(meta.kind, EntryKind::File);
        assert_eq!(meta.size, data.len() as u64);
    }

    #[tokio::test]
    async fn browse_lists_uploaded_entries() {
        let app = app_with_memory();
        app.upload("mem", "a", Bytes::from_static(b"1"), None)
            .await
            .unwrap();
        app.upload("mem", "b", Bytes::from_static(b"22"), None)
            .await
            .unwrap();
        let entries = app.browse("mem", "").await.unwrap();
        assert_eq!(entries.len(), 2);
    }

    #[tokio::test]
    async fn delete_removes_object() {
        let app = app_with_memory();
        app.upload("mem", "x", Bytes::from_static(b"1"), None)
            .await
            .unwrap();
        app.delete("mem", "x").await.unwrap();
        assert!(matches!(
            app.stat("mem", "x").await,
            Err(AppError::Provider(ProviderError::NotFound(_)))
        ));
    }

    #[tokio::test]
    async fn unknown_account_errors() {
        let app = App::new();
        assert!(matches!(
            app.browse("ghost", "").await,
            Err(AppError::NoSuchProvider(_))
        ));
    }

    #[test]
    fn account_management() {
        let app = app_with_memory();
        assert_eq!(app.accounts(), vec!["mem"]);
        assert!(app.remove_account("mem").unwrap());
        assert!(app.accounts().is_empty());
    }

    #[test]
    fn store_backed_accounts_persist_across_restart() {
        let path = std::env::temp_dir().join(format!(
            "nebula-app-test-{}.db",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        {
            let app = App::with_store(&path).unwrap();
            app.add_aliyun_account("acc", "ak", "sk", "oss-cn-hangzhou.aliyuncs.com")
                .unwrap();
            assert_eq!(app.accounts(), vec!["acc"]);
        }
        {
            // 重新打开:账号应从库里加载并注册。
            let app = App::with_store(&path).unwrap();
            assert_eq!(app.accounts(), vec!["acc"]);
            assert!(app.remove_account("acc").unwrap());
        }
        {
            // 删除已持久化:再次打开为空。
            let app = App::with_store(&path).unwrap();
            assert!(app.accounts().is_empty());
        }
        let _ = std::fs::remove_file(&path);
    }
}
