//! # app-core
//!
//! Nebula 的**框架无关**业务逻辑层。它持有一个 [`ProviderRegistry`],对上暴露以
//! "账号 id + 路径"为参数的高层操作(浏览 / 上传 / 下载 / 删除),对下依赖统一的
//! [`StorageProvider`] 抽象。
//!
//! Tauri 外壳(`app/src-tauri`)只是把这里的方法包成 `#[tauri::command]`,因此这些
//! 逻辑可以完全用 `cargo test` 覆盖,无需启动 GUI。

mod error;
mod secret;
mod settings;
mod store;

use std::sync::Arc;

use bytes::Bytes;
use nebula_provider::{Entry, ProviderRegistry, StorageProvider};
use provider_aliyun::AliyunProvider;

pub use error::{AppError, Result};
pub use nebula_provider::{ByteStream, Capabilities, EntryKind, ProgressFn};
pub use secret::{KeyringSecrets, MemorySecrets, SecretStore};
pub use settings::Settings;
pub use store::{AccountRecord, AccountStore};

const VENDOR_ALIYUN: &str = "aliyun";
/// 钥匙串里存储密钥用的服务名。
const KEYRING_SERVICE: &str = "org.devlive.nebula";

/// App 的核心状态与操作入口。可低成本 clone(共享注册表 / 存储 / 密钥库)。
#[derive(Clone)]
pub struct App {
    registry: ProviderRegistry,
    /// 账号元信息持久化;`None` 时仅存内存(用于测试)。
    store: Option<Arc<AccountStore>>,
    /// 敏感密钥存储(钥匙串或内存)。
    secrets: Arc<dyn SecretStore>,
}

impl Default for App {
    fn default() -> Self {
        Self {
            registry: ProviderRegistry::new(),
            store: None,
            secrets: Arc::new(MemorySecrets::default()),
        }
    }
}

impl App {
    /// 新建一个不持久化的空 App(账号仅存内存,密钥走内存)。
    pub fn new() -> Self {
        Self::default()
    }

    /// 用 SQLite 库路径创建 App,密钥走系统钥匙串,并加载已存账号。
    pub fn with_store(db_path: impl AsRef<std::path::Path>) -> Result<Self> {
        Self::with_store_and_secrets(db_path, Arc::new(KeyringSecrets::new(KEYRING_SERVICE)))
    }

    /// 同 [`Self::with_store`],但可注入自定义密钥库(便于测试)。
    pub fn with_store_and_secrets(
        db_path: impl AsRef<std::path::Path>,
        secrets: Arc<dyn SecretStore>,
    ) -> Result<Self> {
        let store = AccountStore::open(db_path)?;
        let app = App {
            registry: ProviderRegistry::new(),
            store: Some(Arc::new(store)),
            secrets,
        };
        app.load_persisted()?;
        Ok(app)
    }

    /// 把存储里的账号构造成 provider 并注册;密钥缺失的账号跳过。
    fn load_persisted(&self) -> Result<()> {
        let Some(store) = &self.store else {
            return Ok(());
        };
        for rec in store.list()? {
            if let Ok(secret) = self.secrets.get(&rec.id) {
                self.register_record(&rec, &secret);
            }
        }
        Ok(())
    }

    /// 按厂商把一条记录 + 密钥注册为 provider(未知厂商忽略)。
    fn register_record(&self, rec: &AccountRecord, secret: &str) {
        if rec.vendor == VENDOR_ALIYUN {
            self.registry.register(Arc::new(AliyunProvider::new(
                rec.id.clone(),
                rec.access_key_id.clone(),
                secret.to_string(),
                rec.endpoint.clone(),
            )));
        }
    }

    /// 注册一个账号 / provider(以 `provider.id()` 为键)。不持久化。
    pub fn add_account(&self, provider: Arc<dyn StorageProvider>) {
        self.registry.register(provider);
    }

    /// 便捷:新增一个阿里云 OSS 账号。密钥进钥匙串,元信息进 SQLite。
    pub fn add_aliyun_account(
        &self,
        id: impl Into<String>,
        access_key_id: impl Into<String>,
        access_key_secret: impl Into<String>,
        endpoint: impl Into<String>,
    ) -> Result<()> {
        let id = id.into();
        let secret = access_key_secret.into();
        self.secrets.set(&id, &secret)?;

        // access_key_secret 不落 SQLite,置空;真实密钥在钥匙串。
        let rec = AccountRecord {
            id,
            vendor: VENDOR_ALIYUN.to_string(),
            access_key_id: access_key_id.into(),
            access_key_secret: String::new(),
            endpoint: endpoint.into(),
        };
        if let Some(store) = &self.store {
            store.upsert(&rec)?;
        }
        self.register_record(&rec, &secret);
        Ok(())
    }

    /// 移除一个账号(注册表 + 存储 + 钥匙串),返回它是否存在过。
    pub fn remove_account(&self, id: &str) -> Result<bool> {
        self.secrets.delete(id)?;
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

    /// 流式下载:返回 `(内容长度, 分块流)`,供调用方边写边报进度。
    pub async fn download_stream(
        &self,
        account: &str,
        path: &str,
    ) -> Result<(Option<u64>, ByteStream)> {
        Ok(self.provider(account)?.read_stream(path).await?)
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

    /// 带进度的上传:`progress(已上传字节, 总字节)` 会在传输过程中被多次调用。
    pub async fn upload_with_progress(
        &self,
        account: &str,
        path: &str,
        data: Bytes,
        content_type: Option<&str>,
        progress: ProgressFn<'_>,
    ) -> Result<()> {
        Ok(self
            .provider(account)?
            .write_with_progress(path, data, content_type, progress)
            .await?)
    }

    /// 删除对象。
    pub async fn delete(&self, account: &str, path: &str) -> Result<()> {
        Ok(self.provider(account)?.delete(path).await?)
    }

    /// 重命名 / 移动对象。
    pub async fn rename(&self, account: &str, from: &str, to: &str) -> Result<()> {
        Ok(self.provider(account)?.rename(from, to).await?)
    }

    /// 复制对象到新路径(保留原对象)。
    pub async fn copy(&self, account: &str, from: &str, to: &str) -> Result<()> {
        Ok(self.provider(account)?.copy(from, to).await?)
    }

    /// 生成预签名下载链接,`expires_secs` 秒后失效。
    pub async fn presign(&self, account: &str, path: &str, expires_secs: u64) -> Result<String> {
        Ok(self.provider(account)?.presign(path, expires_secs).await?)
    }

    /// 读取应用设置(无存储或未设置时返回默认值)。
    pub fn settings(&self) -> Settings {
        let mut s = Settings::default();
        let Some(store) = &self.store else {
            return s;
        };
        if let Ok(Some(v)) = store.get_setting("share_expiry_secs") {
            if let Ok(n) = v.parse() {
                s.share_expiry_secs = n;
            }
        }
        if let Ok(Some(v)) = store.get_setting("concurrency") {
            if let Ok(n) = v.parse() {
                s.concurrency = n;
            }
        }
        s
    }

    /// 保存应用设置到 SQLite。
    pub fn save_settings(&self, settings: &Settings) -> Result<()> {
        if let Some(store) = &self.store {
            store.set_setting("share_expiry_secs", &settings.share_expiry_secs.to_string())?;
            store.set_setting("concurrency", &settings.concurrency.to_string())?;
        }
        Ok(())
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

    fn temp_db(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "nebula-app-{tag}-{}.db",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn store_backed_accounts_persist_across_restart() {
        let path = temp_db("persist");
        // 共享同一份密钥库,模拟钥匙串在重启后仍在。
        let secrets: Arc<dyn SecretStore> = Arc::new(MemorySecrets::default());
        {
            let app = App::with_store_and_secrets(&path, secrets.clone()).unwrap();
            app.add_aliyun_account("acc", "ak", "sk", "oss-cn-hangzhou.aliyuncs.com")
                .unwrap();
            assert_eq!(app.accounts(), vec!["acc"]);
        }
        {
            // 重新打开:账号应从库 + 密钥库加载并注册。
            let app = App::with_store_and_secrets(&path, secrets.clone()).unwrap();
            assert_eq!(app.accounts(), vec!["acc"]);
            assert!(app.remove_account("acc").unwrap());
        }
        {
            let app = App::with_store_and_secrets(&path, secrets.clone()).unwrap();
            assert!(app.accounts().is_empty());
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn settings_default_and_persist() {
        // 无存储:返回默认值。
        assert_eq!(App::new().settings().concurrency, 3);

        let path = temp_db("settings");
        let secrets: Arc<dyn SecretStore> = Arc::new(MemorySecrets::default());
        {
            let app = App::with_store_and_secrets(&path, secrets.clone()).unwrap();
            let mut s = app.settings();
            s.share_expiry_secs = 1800;
            s.concurrency = 5;
            app.save_settings(&s).unwrap();
        }
        {
            let app = App::with_store_and_secrets(&path, secrets.clone()).unwrap();
            let s = app.settings();
            assert_eq!(s.share_expiry_secs, 1800);
            assert_eq!(s.concurrency, 5);
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn secret_is_not_written_to_sqlite() {
        let path = temp_db("nosecret");
        let secrets: Arc<dyn SecretStore> = Arc::new(MemorySecrets::default());
        {
            let app = App::with_store_and_secrets(&path, secrets.clone()).unwrap();
            app.add_aliyun_account("a", "my-ak", "top-secret", "ep")
                .unwrap();
        }
        // 直接读库:密钥列应为空,ak/endpoint 正常。
        let store = AccountStore::open(&path).unwrap();
        let recs = store.list().unwrap();
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].access_key_secret, "");
        assert_eq!(recs[0].access_key_id, "my-ak");
        // 密钥应能从密钥库取回。
        assert_eq!(secrets.get("a").unwrap(), "top-secret");
        let _ = std::fs::remove_file(&path);
    }
}
