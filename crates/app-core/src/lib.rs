//! # app-core
//!
//! Nebula 的**框架无关**业务逻辑层。它持有一个 [`ProviderRegistry`],对上暴露以
//! "账号 id + 路径"为参数的高层操作(浏览 / 上传 / 下载 / 删除),对下依赖统一的
//! [`StorageProvider`] 抽象。
//!
//! Tauri 外壳(`app/src-tauri`)只是把这里的方法包成 `#[tauri::command]`,因此这些
//! 逻辑可以完全用 `cargo test` 覆盖,无需启动 GUI。

mod breakdown;
mod cancel;
mod dedup;
mod error;
mod imaging;
mod integrity;
mod limits;
mod matting_plugin;
mod pdf;
mod preview;
mod secret;
mod settings;
mod store;
mod sync;
mod upload;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use bytes::Bytes;
use nebula_provider::{Entry, ProviderRegistry, StorageProvider};
use provider_aliyun::AliyunProvider;
use provider_aws::AwsProvider;
use provider_huawei::HuaweiProvider;
use provider_minio::MinioProvider;
use provider_qiniu::QiniuProvider;
use provider_r2::R2Provider;
use provider_tencent::TencentProvider;

pub use breakdown::{ClassStat, StorageBreakdown};
pub use error::{AppError, Result};
pub use imaging::{CropRect, EditSave, ExifInfo, ImageData, Ops};
pub use integrity::{verify_bytes, Integrity};
pub use limits::TransferLimits;
pub use nebula_pdf::{Assembly, NumberPos, PageInfo, PageNumbers, PageSpec, PdfInfo, Watermark};
pub use nebula_provider::{
    ByteStream, Capabilities, EntryKind, IncompleteUpload, Page, ProgressFn,
};
pub use preview::TextPreview;
pub use secret::{KeyringSecrets, MemorySecrets, SecretStore};
pub use settings::Settings;
pub use store::{AccountRecord, AccountStore};
pub use sync::{
    diff as sync_diff, summarize as sync_summarize, ConflictChoice, DiffItem, DiffSummary,
    LocalFile, RemoteFile, SyncAction, SyncJob, SyncMode, SyncReport, SyncSpec,
};

const VENDOR_ALIYUN: &str = "aliyun";
const VENDOR_HUAWEI: &str = "huawei";
const VENDOR_QINIU: &str = "qiniu";
const VENDOR_AWS: &str = "aws";
const VENDOR_R2: &str = "r2";
const VENDOR_MINIO: &str = "minio";
const VENDOR_TENCENT: &str = "tencent";

/// 递归搜索结果:命中条目 + 是否因触及上限而**可能不完整**。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SearchResult {
    pub entries: Vec<Entry>,
    /// 因结果数或扫描量触顶而提前结束 → 结果可能不完整。
    pub truncated: bool,
}

/// 搜索过滤条件(在名字匹配之外进一步筛选)。全 `None` / 空表示不过滤。
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct SearchFilter {
    /// 仅保留 `>=` 该字节数的文件。
    pub min_size: Option<u64>,
    /// 仅保留该扩展名的文件(不含点,大小写不敏感)。
    pub ext: Option<String>,
}

impl SearchFilter {
    /// 该文件是否通过过滤(大小 + 扩展名)。
    fn accepts(&self, entry: &Entry) -> bool {
        if let Some(min) = self.min_size {
            if entry.size < min {
                return false;
            }
        }
        if let Some(ext) = &self.ext {
            let want = ext.trim().trim_start_matches('.').to_lowercase();
            if !want.is_empty() && !entry.name.to_lowercase().ends_with(&format!(".{want}")) {
                return false;
            }
        }
        true
    }
}

/// 前缀(文件夹 / Bucket)统计:文件数 + 总字节数。
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FolderStats {
    pub files: u64,
    pub bytes: u64,
    /// 因扫描量触顶而提前结束 → 统计可能偏小。
    pub truncated: bool,
}

/// 账号的非敏感信息(不含密钥),供编辑回填用。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AccountInfo {
    pub id: String,
    pub vendor: String,
    pub access_key_id: String,
    pub endpoint: String,
    /// 自定义公共域名(CDN / CNAME);空表示未配置。
    pub custom_domain: String,
}

/// 传输面板任务的持久化记录,用于跨重启恢复列表。字段与前端 TransferItem 对应;
/// `kind` / `status` 为不透明字符串(App 层不解释其含义)。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TransferRecord {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub account: String,
    pub remote: String,
    pub local: String,
    pub done: u64,
    pub total: u64,
    pub status: String,
}
/// 一个收藏的位置(账号 + 路径),供前端「收藏夹」快速跳转。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Bookmark {
    pub account: String,
    pub path: String,
}

/// 批量重命名规则。`mode` 为 `prefix` / `suffix` / `replace`;
/// `a` 是前缀 / 后缀 / 查找串,`b` 是替换串(仅 `replace` 用)。
#[derive(Debug, Clone, serde::Deserialize)]
pub struct RenameRule {
    pub mode: String,
    #[serde(default)]
    pub a: String,
    #[serde(default)]
    pub b: String,
}

/// 一条重命名计划:把对象从 `from` 改名到 `to`。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct RenamePlan {
    pub from: String,
    pub to: String,
}

/// 按规则变换单个对象的**基名**(路径最后一段);父前缀保持不变。
fn rename_base(base: &str, rule: &RenameRule) -> String {
    match rule.mode.as_str() {
        "prefix" => format!("{}{base}", rule.a),
        "suffix" => {
            // 后缀插在扩展名之前(`a.txt` + `-v2` → `a-v2.txt`);无扩展名则直接追加。
            // 只认最后一个「非开头」的点,`.gitignore` 这类隐藏文件不当作有扩展名。
            match base.rfind('.') {
                Some(i) if i > 0 => format!("{}{}{}", &base[..i], rule.a, &base[i..]),
                _ => format!("{base}{}", rule.a),
            }
        }
        "replace" => {
            if rule.a.is_empty() {
                base.to_string()
            } else {
                base.replace(&rule.a, &rule.b)
            }
        }
        _ => base.to_string(),
    }
}

/// 为一批对象路径计算重命名计划。父前缀不变、仅改基名;
/// 新基名为空、或结果与原路径相同的项会被跳过(不出现在计划里)。
pub fn plan_batch_rename(paths: &[String], rule: &RenameRule) -> Vec<RenamePlan> {
    let mut plans = Vec::new();
    for path in paths {
        // 拆出父前缀(含末尾 `/`)与基名。
        let (parent, base) = match path.rfind('/') {
            Some(i) => (&path[..=i], &path[i + 1..]),
            None => ("", path.as_str()),
        };
        if base.is_empty() {
            continue; // 目录项(以 `/` 结尾),跳过
        }
        let new_base = rename_base(base, rule);
        if new_base.is_empty() {
            continue;
        }
        let to = format!("{parent}{new_base}");
        if to != *path {
            plans.push(RenamePlan {
                from: path.clone(),
                to,
            });
        }
    }
    plans
}

/// 钥匙串里存储密钥用的服务名。
const KEYRING_SERVICE: &str = "org.devlive.nebula";

/// 递归搜索最多扫描的条目数(跨所有层级),防止超大桶把搜索拖死。
const SEARCH_SCAN_LIMIT: usize = 20_000;

/// 「最近访问」保留的位置条数。
const RECENT_LOCATIONS_KEEP: usize = 20;

/// App 的核心状态与操作入口。可低成本 clone(共享注册表 / 存储 / 密钥库)。
#[derive(Clone)]
pub struct App {
    registry: ProviderRegistry,
    /// 账号元信息持久化;`None` 时仅存内存(用于测试)。
    store: Option<Arc<AccountStore>>,
    /// 敏感密钥存储(钥匙串或内存)。
    secrets: Arc<dyn SecretStore>,
    /// 全局传输带宽限速(所有克隆共享同一令牌桶)。
    limits: TransferLimits,
    /// 图片缓存目录(渲染变体落盘);由 Tauri 层用 app_cache_dir 设置。所有克隆共享。
    cache_dir: Arc<std::sync::OnceLock<std::path::PathBuf>>,
    /// 插件安装目录(AI 抠图的模型 + ONNX Runtime 库)。持久,不随缓存清理。
    plugin_dir: Arc<std::sync::OnceLock<std::path::PathBuf>>,
}

impl Default for App {
    fn default() -> Self {
        Self {
            registry: ProviderRegistry::new(),
            store: None,
            secrets: Arc::new(MemorySecrets::default()),
            limits: TransferLimits::default(),
            cache_dir: Arc::new(std::sync::OnceLock::new()),
            plugin_dir: Arc::new(std::sync::OnceLock::new()),
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
            limits: TransferLimits::default(),
            cache_dir: Arc::new(std::sync::OnceLock::new()),
            plugin_dir: Arc::new(std::sync::OnceLock::new()),
        };
        app.load_persisted()?;
        // 应用持久化的限速设置。
        app.limits
            .set_kib_per_sec(app.settings().rate_limit_kib_per_sec);
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
        match rec.vendor.as_str() {
            VENDOR_ALIYUN => self.registry.register(Arc::new(AliyunProvider::new(
                rec.id.clone(),
                rec.access_key_id.clone(),
                secret.to_string(),
                rec.endpoint.clone(),
            ))),
            VENDOR_HUAWEI => self.registry.register(Arc::new(HuaweiProvider::new(
                rec.id.clone(),
                rec.access_key_id.clone(),
                secret.to_string(),
                rec.endpoint.clone(),
            ))),
            VENDOR_QINIU => self.registry.register(Arc::new(QiniuProvider::new(
                rec.id.clone(),
                rec.access_key_id.clone(),
                secret.to_string(),
                rec.endpoint.clone(),
            ))),
            VENDOR_AWS => self.registry.register(Arc::new(AwsProvider::new(
                rec.id.clone(),
                rec.access_key_id.clone(),
                secret.to_string(),
                rec.endpoint.clone(),
            ))),
            VENDOR_R2 => self.registry.register(Arc::new(R2Provider::new(
                rec.id.clone(),
                rec.access_key_id.clone(),
                secret.to_string(),
                rec.endpoint.clone(),
            ))),
            VENDOR_MINIO => self.registry.register(Arc::new(MinioProvider::new(
                rec.id.clone(),
                rec.access_key_id.clone(),
                secret.to_string(),
                rec.endpoint.clone(),
            ))),
            VENDOR_TENCENT => self.registry.register(Arc::new(TencentProvider::new(
                rec.id.clone(),
                rec.access_key_id.clone(),
                secret.to_string(),
                rec.endpoint.clone(),
            ))),
            _ => {}
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
            custom_domain: String::new(),
        };
        if let Some(store) = &self.store {
            store.upsert(&rec)?;
        }
        self.register_record(&rec, &secret);
        Ok(())
    }

    /// 便捷:新增一个华为云 OBS 账号。密钥进钥匙串,元信息进 SQLite。
    pub fn add_huawei_account(
        &self,
        id: impl Into<String>,
        access_key: impl Into<String>,
        secret_key: impl Into<String>,
        endpoint: impl Into<String>,
    ) -> Result<()> {
        let id = id.into();
        let secret = secret_key.into();
        self.secrets.set(&id, &secret)?;

        // secret_key 不落 SQLite,置空;真实密钥在钥匙串。
        let rec = AccountRecord {
            id,
            vendor: VENDOR_HUAWEI.to_string(),
            access_key_id: access_key.into(),
            access_key_secret: String::new(),
            endpoint: endpoint.into(),
            custom_domain: String::new(),
        };
        if let Some(store) = &self.store {
            store.upsert(&rec)?;
        }
        self.register_record(&rec, &secret);
        Ok(())
    }

    /// 便捷:新增一个七牛云 Kodo 账号(S3 兼容)。密钥进钥匙串,元信息进 SQLite。
    pub fn add_qiniu_account(
        &self,
        id: impl Into<String>,
        access_key: impl Into<String>,
        secret_key: impl Into<String>,
        endpoint: impl Into<String>,
    ) -> Result<()> {
        let id = id.into();
        let secret = secret_key.into();
        self.secrets.set(&id, &secret)?;

        let rec = AccountRecord {
            id,
            vendor: VENDOR_QINIU.to_string(),
            access_key_id: access_key.into(),
            access_key_secret: String::new(),
            endpoint: endpoint.into(),
            custom_domain: String::new(),
        };
        if let Some(store) = &self.store {
            store.upsert(&rec)?;
        }
        self.register_record(&rec, &secret);
        Ok(())
    }

    /// 便捷:新增一个 AWS S3 账号。密钥进钥匙串,元信息进 SQLite。
    pub fn add_aws_account(
        &self,
        id: impl Into<String>,
        access_key: impl Into<String>,
        secret_key: impl Into<String>,
        endpoint: impl Into<String>,
    ) -> Result<()> {
        let id = id.into();
        let secret = secret_key.into();
        self.secrets.set(&id, &secret)?;

        let rec = AccountRecord {
            id,
            vendor: VENDOR_AWS.to_string(),
            access_key_id: access_key.into(),
            access_key_secret: String::new(),
            endpoint: endpoint.into(),
            custom_domain: String::new(),
        };
        if let Some(store) = &self.store {
            store.upsert(&rec)?;
        }
        self.register_record(&rec, &secret);
        Ok(())
    }

    /// 便捷:新增一个 Cloudflare R2 账号。密钥进钥匙串,元信息进 SQLite。
    pub fn add_r2_account(
        &self,
        id: impl Into<String>,
        access_key: impl Into<String>,
        secret_key: impl Into<String>,
        endpoint: impl Into<String>,
    ) -> Result<()> {
        let id = id.into();
        let secret = secret_key.into();
        self.secrets.set(&id, &secret)?;

        let rec = AccountRecord {
            id,
            vendor: VENDOR_R2.to_string(),
            access_key_id: access_key.into(),
            access_key_secret: String::new(),
            endpoint: endpoint.into(),
            custom_domain: String::new(),
        };
        if let Some(store) = &self.store {
            store.upsert(&rec)?;
        }
        self.register_record(&rec, &secret);
        Ok(())
    }

    /// 便捷:新增一个 MinIO 账号。密钥进钥匙串,元信息进 SQLite。
    pub fn add_minio_account(
        &self,
        id: impl Into<String>,
        access_key: impl Into<String>,
        secret_key: impl Into<String>,
        endpoint: impl Into<String>,
    ) -> Result<()> {
        let id = id.into();
        let secret = secret_key.into();
        self.secrets.set(&id, &secret)?;

        let rec = AccountRecord {
            id,
            vendor: VENDOR_MINIO.to_string(),
            access_key_id: access_key.into(),
            access_key_secret: String::new(),
            endpoint: endpoint.into(),
            custom_domain: String::new(),
        };
        if let Some(store) = &self.store {
            store.upsert(&rec)?;
        }
        self.register_record(&rec, &secret);
        Ok(())
    }

    /// 便捷:新增一个腾讯云 COS 账号。密钥进钥匙串,元信息进 SQLite。
    /// COS 的凭证是 SecretId/SecretKey;`access_key` 传 SecretId,`secret_key` 传 SecretKey。
    pub fn add_tencent_account(
        &self,
        id: impl Into<String>,
        access_key: impl Into<String>,
        secret_key: impl Into<String>,
        endpoint: impl Into<String>,
    ) -> Result<()> {
        let id = id.into();
        let secret = secret_key.into();
        self.secrets.set(&id, &secret)?;

        let rec = AccountRecord {
            id,
            vendor: VENDOR_TENCENT.to_string(),
            access_key_id: access_key.into(),
            access_key_secret: String::new(),
            endpoint: endpoint.into(),
            custom_domain: String::new(),
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

    /// 列出所有账号的非敏感信息(id + 厂商 + endpoint),按注册 id 字典序。
    ///
    /// 供 UI 按厂商展示图标等;无存储时返回空。
    pub fn account_infos(&self) -> Vec<AccountInfo> {
        self.accounts()
            .into_iter()
            .filter_map(|id| self.account_info(&id))
            .collect()
    }

    /// 读取某账号的非敏感信息(不含密钥),用于编辑回填。
    pub fn account_info(&self, id: &str) -> Option<AccountInfo> {
        let store = self.store.as_ref()?;
        store
            .list()
            .ok()?
            .into_iter()
            .find(|r| r.id == id)
            .map(|r| AccountInfo {
                id: r.id,
                vendor: r.vendor,
                access_key_id: r.access_key_id,
                endpoint: r.endpoint,
                custom_domain: r.custom_domain,
            })
    }

    /// 设置某账号的自定义公共域名(CDN / CNAME);空串清除。
    pub fn set_account_domain(&self, account: &str, domain: &str) -> Result<()> {
        if let Some(store) = &self.store {
            store.set_custom_domain(account, domain.trim())?;
        }
        Ok(())
    }

    /// 浏览某账号下某路径(桶 / 前缀)的条目。
    pub async fn browse(&self, account: &str, path: &str) -> Result<Vec<Entry>> {
        Ok(self.provider(account)?.list(path).await?)
    }

    /// 分页浏览:返回某路径下的**一页**条目 + 下一页游标(`cursor` 为 `None` 取第一页)。
    pub async fn browse_page(
        &self,
        account: &str,
        path: &str,
        cursor: Option<String>,
    ) -> Result<Page> {
        Ok(self.provider(account)?.list_page(path, cursor).await?)
    }

    /// 读取某路径的元信息。
    pub async fn stat(&self, account: &str, path: &str) -> Result<Entry> {
        Ok(self.provider(account)?.stat(path).await?)
    }

    /// 在 `root`(桶 / 前缀)下**递归**搜索名字包含 `query`(大小写不敏感)的文件,
    /// 最多返回 `max_results` 条。
    ///
    /// **逐页**下降(用 [`list_page`](StorageProvider::list_page)):每取一页就判断,凑够
    /// `max_results` 立即返回,不会把整层拉完;并用总扫描量上限 [`SEARCH_SCAN_LIMIT`] 兜底,
    /// 避免超大桶把搜索拖到"卡死"。命中稀疏的超大桶会扫到上限后返回已找到的部分。
    pub async fn search(
        &self,
        account: &str,
        root: &str,
        query: &str,
        filter: &SearchFilter,
        max_results: usize,
    ) -> Result<SearchResult> {
        let provider = self.provider(account)?;
        let needle = query.trim().to_lowercase();
        let mut results = Vec::new();
        let mut scanned = 0usize;
        let mut truncated = false;
        let mut queue = std::collections::VecDeque::new();
        queue.push_back(root.to_string());

        'walk: while let Some(dir) = queue.pop_front() {
            let mut cursor = None;
            loop {
                let page = provider.list_page(&dir, cursor).await?;
                for entry in page.entries {
                    scanned += 1;
                    if entry.is_dir() {
                        queue.push_back(entry.path);
                    } else if (needle.is_empty() || entry.name.to_lowercase().contains(&needle))
                        && filter.accepts(&entry)
                    {
                        results.push(entry);
                        if results.len() >= max_results {
                            truncated = true;
                            break 'walk;
                        }
                    }
                }
                if scanned >= SEARCH_SCAN_LIMIT {
                    truncated = true;
                    break 'walk;
                }
                match page.cursor {
                    Some(next) => cursor = Some(next),
                    None => break,
                }
            }
        }
        Ok(SearchResult {
            entries: results,
            truncated,
        })
    }

    /// 下载对象内容。
    pub async fn download(&self, account: &str, path: &str) -> Result<Bytes> {
        Ok(self.provider(account)?.read(path).await?)
    }

    /// 校验对象内容完整性:下载内容,用远端 ETag 与其 MD5 比对。
    ///
    /// 整对象上传的文件 ETag 即内容 MD5,可判断是否损坏;分片对象无法这样校验,
    /// 返回 [`Integrity::Unverifiable`]。逻辑对所有厂商通用(见 [`integrity`])。
    pub async fn verify(&self, account: &str, path: &str) -> Result<Integrity> {
        let provider = self.provider(account)?;
        let meta = provider.stat(path).await?;
        let data = provider.read(path).await?;
        Ok(integrity::verify_bytes(&data, meta.etag.as_deref()))
    }

    /// 流式下载:返回 `(内容长度, 分块流)`,供调用方边写边报进度。
    pub async fn download_stream(
        &self,
        account: &str,
        path: &str,
    ) -> Result<(Option<u64>, ByteStream)> {
        Ok(self.provider(account)?.read_stream(path).await?)
    }

    /// 从 `offset` 字节开始流式下载(HTTP Range),返回 `(对象总大小, 剩余流)`,用于断点续传。
    pub async fn download_range(
        &self,
        account: &str,
        path: &str,
        offset: u64,
    ) -> Result<(Option<u64>, ByteStream)> {
        Ok(self.provider(account)?.read_range(path, offset).await?)
    }

    /// 读取对象前若干字节并按 UTF-8 (lossy) 解码,用于文本 / 代码 / 配置文件预览。
    /// 至多读取 `max_bytes` 字节,超出即在 [`TextPreview::truncated`] 标记。
    pub async fn read_preview(
        &self,
        account: &str,
        path: &str,
        max_bytes: usize,
    ) -> Result<TextPreview> {
        use futures::StreamExt;
        let (_, mut stream) = self.provider(account)?.read_stream(path).await?;
        let mut buf: Vec<u8> = Vec::new();
        let mut truncated = false;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            if preview::accumulate(&mut buf, &chunk, max_bytes) {
                truncated = true;
                break;
            }
        }
        Ok(TextPreview {
            text: preview::decode(&buf),
            truncated,
        })
    }

    /// 秒传判断:远端 `remote_path` 是否已与本地 `local_path` 内容一致(可跳过上传)。
    ///
    /// 保守判定——仅当远端存在、ETag 为整对象 MD5、大小与本地一致、且本地 MD5 与之相等时返回
    /// `true`;任何不确定(远端缺失 / 分片 ETag / 大小不同 / 读取失败)都返回 `false` 以照常上传。
    pub async fn is_unchanged(
        &self,
        account: &str,
        remote_path: &str,
        local_path: &str,
    ) -> Result<bool> {
        let Ok(meta) = self.provider(account)?.stat(remote_path).await else {
            return Ok(false); // 远端不存在 → 需要上传
        };
        let local_size = tokio::fs::metadata(local_path).await?.len();
        let Some(expected) = dedup::worth_comparing(meta.etag.as_deref(), meta.size, local_size)
        else {
            return Ok(false); // 分片 ETag / 无 ETag / 大小不同 → 不值得比对,照常上传
        };
        Ok(dedup::file_md5(local_path).await? == expected)
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

    /// 转换对象存储类型 / 归档层(`class` 为厂商的存储类型字符串)。
    pub async fn set_storage_class(&self, account: &str, path: &str, class: &str) -> Result<()> {
        Ok(self
            .provider(account)?
            .set_storage_class(path, class)
            .await?)
    }

    /// 取回(解冻)归档对象,`days` 为取回后可读的保持天数。
    pub async fn restore(&self, account: &str, path: &str, days: u32) -> Result<()> {
        Ok(self.provider(account)?.restore(path, days).await?)
    }

    /// 设置对象为公开读(`public = true`)或私有。
    pub async fn set_object_acl(&self, account: &str, path: &str, public: bool) -> Result<()> {
        Ok(self.provider(account)?.set_object_acl(path, public).await?)
    }

    /// 对象的永久公共直链(不签名);未支持返回 `None`。
    ///
    /// 若该账号配了自定义公共域名(CDN / CNAME),用它拼直链 `https://{域名}/{key}`
    /// (域名绑定到桶,故去掉路径里的桶名段);否则用 provider 的默认直链。
    pub fn public_url(&self, account: &str, path: &str) -> Result<Option<String>> {
        if let Some(store) = &self.store {
            if let Ok(Some(rec)) = store.get(account) {
                if !rec.custom_domain.is_empty() {
                    let key = path.split_once('/').map(|(_, k)| k).unwrap_or("");
                    let domain = rec
                        .custom_domain
                        .trim()
                        .trim_end_matches('/')
                        .trim_start_matches("https://")
                        .trim_start_matches("http://");
                    return Ok(Some(format!("https://{domain}/{}", encode_url_path(key))));
                }
            }
        }
        Ok(self.provider(account)?.public_url(path))
    }

    /// 修改对象的内容类型(`Content-Type`)。
    pub async fn set_content_type(
        &self,
        account: &str,
        path: &str,
        content_type: &str,
    ) -> Result<()> {
        Ok(self
            .provider(account)?
            .set_content_type(path, content_type)
            .await?)
    }

    /// 读取对象标签(键值对)。
    pub async fn object_tags(&self, account: &str, path: &str) -> Result<Vec<(String, String)>> {
        Ok(self.provider(account)?.object_tags(path).await?)
    }

    /// 列举某桶下未完成(残留)的分片上传。
    pub async fn incomplete_uploads(
        &self,
        account: &str,
        bucket: &str,
    ) -> Result<Vec<IncompleteUpload>> {
        Ok(self
            .provider(account)?
            .list_incomplete_uploads(bucket)
            .await?)
    }

    /// 清理某桶下所有未完成的分片上传(逐个 abort),返回清理数量。
    pub async fn clean_incomplete_uploads(&self, account: &str, bucket: &str) -> Result<u64> {
        let provider = self.provider(account)?;
        let uploads = provider.list_incomplete_uploads(bucket).await?;
        let mut cleaned = 0u64;
        for u in &uploads {
            let path = format!("{bucket}/{}", u.key);
            provider.abort_multipart(&path, &u.upload_id).await?;
            cleaned += 1;
        }
        Ok(cleaned)
    }

    /// 覆盖对象标签(整套替换;空列表即清空)。
    pub async fn set_object_tags(
        &self,
        account: &str,
        path: &str,
        tags: &[(String, String)],
    ) -> Result<()> {
        Ok(self.provider(account)?.set_object_tags(path, tags).await?)
    }

    /// 在某账号下新建一个 bucket。
    pub async fn create_bucket(&self, account: &str, bucket: &str) -> Result<()> {
        Ok(self.provider(account)?.create_bucket(bucket).await?)
    }

    /// 删除某账号下的一个 bucket(通常要求为空)。
    pub async fn delete_bucket(&self, account: &str, bucket: &str) -> Result<()> {
        Ok(self.provider(account)?.delete_bucket(bucket).await?)
    }

    /// 跨账号 / 跨云复制:把 `src_account` 的 `src_path` 搬到 `dst_account` 的 `dst_path`,
    /// 保留源对象。两端可以是不同的云。
    pub async fn copy_across(
        &self,
        src_account: &str,
        src_path: &str,
        dst_account: &str,
        dst_path: &str,
    ) -> Result<()> {
        self.copy_across_with_progress(
            src_account,
            src_path,
            dst_account,
            dst_path,
            Arc::new(AtomicBool::new(false)),
            &|_, _| {},
        )
        .await
    }

    /// 同 [`Self::copy_across`],但在上传过程中回调 `(已传字节, 总字节)`,并支持取消。
    ///
    /// - **同账号**:直接走 provider 的服务端复制,不下载数据。
    /// - **跨账号 / 跨云**:服务端复制无法跨账号,改为**流式中转**——源的分块下载流直接
    ///   喂给目标的流式分片上传,内存只保留一个滑动窗口,与对象大小无关。
    ///
    /// `cancel` 置位后,中转流会在下一个分块处终止,返回 [`AppError::Cancelled`]。
    pub async fn copy_across_with_progress(
        &self,
        src_account: &str,
        src_path: &str,
        dst_account: &str,
        dst_path: &str,
        cancel: Arc<AtomicBool>,
        progress: ProgressFn<'_>,
    ) -> Result<()> {
        if src_account == dst_account {
            let provider = self.provider(src_account)?;
            provider.copy(src_path, dst_path).await?;
            let total = provider.stat(dst_path).await.map(|e| e.size).unwrap_or(0);
            progress(total, total);
            return Ok(());
        }
        // 跨账号:服务端复制无能为力,边下边传中转——源的分块流直接喂给目标的流式分片
        // 上传,内存只保留一个滑动窗口,与对象大小无关。读取流先套取消,再套全局限速。
        let src = self.provider(src_account)?;
        let dst = self.provider(dst_account)?;
        let (len, stream) = src.read_stream(src_path).await?;
        let stream = cancel::cancellable(stream, cancel.clone());
        let stream = self.limits.throttled(stream);
        let result = dst
            .write_stream(dst_path, len, stream, None, progress)
            .await;
        // 取消导致的流错误归一化为 Cancelled。
        if cancel.load(Ordering::Relaxed) {
            return Err(AppError::Cancelled);
        }
        result?;
        Ok(())
    }

    /// 统计 `root`(桶 / 前缀)下的文件数与总字节数。分页遍历,`SEARCH_SCAN_LIMIT` 兜底;
    /// 触顶时 `truncated = true`(统计可能偏小)。用于下载 / 归档 / 删除前先看目录多大。
    pub async fn folder_stats(&self, account: &str, root: &str) -> Result<FolderStats> {
        let provider = self.provider(account)?;
        let mut stats = FolderStats::default();
        let mut scanned = 0usize;
        let mut queue = std::collections::VecDeque::new();
        queue.push_back(root.to_string());
        'walk: while let Some(dir) = queue.pop_front() {
            let mut cursor = None;
            loop {
                let page = provider.list_page(&dir, cursor).await?;
                for entry in page.entries {
                    scanned += 1;
                    if entry.is_dir() {
                        queue.push_back(entry.path);
                    } else {
                        stats.files += 1;
                        stats.bytes += entry.size;
                    }
                }
                if scanned >= SEARCH_SCAN_LIMIT {
                    stats.truncated = true;
                    break 'walk;
                }
                match page.cursor {
                    Some(next) => cursor = Some(next),
                    None => break,
                }
            }
        }
        Ok(stats)
    }

    /// 统计 `root` 下对象按存储类型的分布(标准 / 低频 / 归档各多少个、多大),
    /// 与 [`folder_stats`](Self::folder_stats) 同源遍历,`SEARCH_SCAN_LIMIT` 兜底。
    pub async fn storage_breakdown(&self, account: &str, root: &str) -> Result<StorageBreakdown> {
        let provider = self.provider(account)?;
        let mut result = StorageBreakdown::default();
        let mut tally = breakdown::ClassTally::default();
        let mut scanned = 0usize;
        let mut queue = std::collections::VecDeque::new();
        queue.push_back(root.to_string());
        'walk: while let Some(dir) = queue.pop_front() {
            let mut cursor = None;
            loop {
                let page = provider.list_page(&dir, cursor).await?;
                for entry in page.entries {
                    scanned += 1;
                    if entry.is_dir() {
                        queue.push_back(entry.path);
                    } else {
                        result.files += 1;
                        result.bytes += entry.size;
                        tally.add(entry.storage_class.as_deref(), entry.size);
                    }
                }
                if scanned >= SEARCH_SCAN_LIMIT {
                    result.truncated = true;
                    break 'walk;
                }
                match page.cursor {
                    Some(next) => cursor = Some(next),
                    None => break,
                }
            }
        }
        result.classes = tally.into_sorted();
        Ok(result)
    }

    /// 递归列出 `root`(桶 / 前缀)下的所有文件(不含目录占位)。
    ///
    /// 分页遍历,`SEARCH_SCAN_LIMIT` 兜底防止在超大目录上失控。文件夹级下载 / 迁移 / 删除的基础件。
    pub async fn list_all_files(&self, account: &str, root: &str) -> Result<Vec<Entry>> {
        let provider = self.provider(account)?;
        Ok(walk_dir(&provider, root).await?.0)
    }

    /// 把 `src_account` 下的整个目录 `src_root` 迁移到 `dst_account` 的 `dst_dir` 下——
    /// 作为其子目录(按 `src_root` 最后一段命名),保留内部相对结构;源对象保留。
    ///
    /// 逐个文件复用 [`Self::copy_across_with_progress`](同账号服务端复制、跨账号流式中转);
    /// `progress(已完成文件数, 总文件数)`。`cancel` 置位后在下个文件前(或中转流中)中止。
    pub async fn migrate_folder(
        &self,
        src_account: &str,
        src_root: &str,
        dst_account: &str,
        dst_dir: &str,
        cancel: Arc<AtomicBool>,
        progress: ProgressFn<'_>,
    ) -> Result<()> {
        let files = self.list_all_files(src_account, src_root).await?;
        let total = files.len() as u64;
        let base = ensure_trailing_slash(src_root);
        let dst_base = format!(
            "{}{}/",
            ensure_trailing_slash(dst_dir),
            folder_name(src_root)
        );
        for (i, file) in files.iter().enumerate() {
            if cancel.load(Ordering::Relaxed) {
                return Err(AppError::Cancelled);
            }
            let rel = file.path.strip_prefix(&base).unwrap_or(file.path.as_str());
            let dst_path = format!("{dst_base}{rel}");
            self.copy_across_with_progress(
                src_account,
                &file.path,
                dst_account,
                &dst_path,
                cancel.clone(),
                &|_, _| {},
            )
            .await?;
            progress((i + 1) as u64, total);
        }
        Ok(())
    }

    /// 把整个文件夹 `src_root` 移动 / 重命名为 `dst_root`(同账号,新的完整目录路径)。
    ///
    /// 逐个文件服务端复制到新前缀再删除原对象,最后清理源目录占位对象;`progress(已处理, 总数)`,
    /// 可取消(停在干净的文件边界)。目标不得为源自身或其子目录(否则自我吞噬)。
    pub async fn move_folder(
        &self,
        account: &str,
        src_root: &str,
        dst_root: &str,
        cancel: Arc<AtomicBool>,
        progress: ProgressFn<'_>,
    ) -> Result<()> {
        let src_base = ensure_trailing_slash(src_root);
        let dst_base = ensure_trailing_slash(dst_root);
        if is_within(&src_base, &dst_base) {
            return Err(AppError::InvalidInput(
                "目标目录不能是源目录自身或其子目录".into(),
            ));
        }
        let provider = self.provider(account)?;
        let (files, mut dirs) = walk_dir(&provider, src_root).await?;
        let total = (files.len() + dirs.len() + 1) as u64; // +1:源目录占位对象
        let mut done = 0u64;
        for file in &files {
            if cancel.load(Ordering::Relaxed) {
                return Err(AppError::Cancelled);
            }
            let rel = file.path.strip_prefix(&src_base).unwrap_or(&file.path);
            let dst_path = format!("{dst_base}{rel}");
            provider.copy(&file.path, &dst_path).await?;
            provider.delete(&file.path).await?;
            done += 1;
            progress(done, total);
        }
        // 清理源目录占位对象(含根),深的先删。
        dirs.push(src_base);
        dirs.sort_by_key(|b| std::cmp::Reverse(b.len()));
        for dir in &dirs {
            provider.delete(dir).await?;
            done += 1;
            progress(done, total);
        }
        Ok(())
    }

    /// 把整个文件夹 `src_root` 复制到 `dst_root`(同账号,保留源)。逐个文件服务端复制到新前缀;
    /// `progress(已处理, 总数)`,可取消。目标不得为源自身或其子目录(否则无限自我复制)。
    pub async fn copy_folder(
        &self,
        account: &str,
        src_root: &str,
        dst_root: &str,
        cancel: Arc<AtomicBool>,
        progress: ProgressFn<'_>,
    ) -> Result<()> {
        let src_base = ensure_trailing_slash(src_root);
        let dst_base = ensure_trailing_slash(dst_root);
        if is_within(&src_base, &dst_base) {
            return Err(AppError::InvalidInput(
                "目标目录不能是源目录自身或其子目录".into(),
            ));
        }
        let provider = self.provider(account)?;
        let files = walk_dir(&provider, src_root).await?.0;
        let total = files.len() as u64;
        for (i, file) in files.iter().enumerate() {
            if cancel.load(Ordering::Relaxed) {
                return Err(AppError::Cancelled);
            }
            let rel = file.path.strip_prefix(&src_base).unwrap_or(&file.path);
            let dst_path = format!("{dst_base}{rel}");
            provider.copy(&file.path, &dst_path).await?;
            progress((i + 1) as u64, total);
        }
        Ok(())
    }

    /// 递归删除 `root` 下所有对象(文件 + 目录占位对象)。`progress(已删数, 总数)`。
    ///
    /// 对象存储的 DELETE 是幂等的,合成前缀(无实体占位对象)删除也安全,故一并清理目录占位,
    /// 避免"新建文件夹"留下的零字节 `/` 对象残留。
    pub async fn delete_folder(
        &self,
        account: &str,
        root: &str,
        progress: ProgressFn<'_>,
    ) -> Result<()> {
        let provider = self.provider(account)?;
        let (files, mut dirs) = walk_dir(&provider, root).await?;
        dirs.push(ensure_trailing_slash(root));
        dirs.sort_by_key(|b| std::cmp::Reverse(b.len())); // 深的先删
        let total = (files.len() + dirs.len()) as u64;
        let mut done = 0u64;
        for file in &files {
            provider.delete(&file.path).await?;
            done += 1;
            progress(done, total);
        }
        for dir in &dirs {
            provider.delete(dir).await?;
            done += 1;
            progress(done, total);
        }
        Ok(())
    }

    /// 把 `root` 下所有文件递归转换到存储类型 `class`(如整个前缀转归档省钱)。
    /// `progress(已处理, 总数)`。逐个复用 [`set_storage_class`](Self::set_storage_class)。
    pub async fn set_storage_class_folder(
        &self,
        account: &str,
        root: &str,
        class: &str,
        progress: ProgressFn<'_>,
    ) -> Result<()> {
        let provider = self.provider(account)?;
        let files = walk_dir(&provider, root).await?.0;
        let total = files.len() as u64;
        for (i, file) in files.iter().enumerate() {
            provider.set_storage_class(&file.path, class).await?;
            progress((i + 1) as u64, total);
        }
        Ok(())
    }

    /// 递归取回 `root` 下所有归档对象,`days` 为保持天数。`progress(已处理, 总数)`。
    ///
    /// 非归档对象取回会被服务端拒绝;这里逐个尽力发起,单个失败不中断整体(记为已处理)。
    pub async fn restore_folder(
        &self,
        account: &str,
        root: &str,
        days: u32,
        progress: ProgressFn<'_>,
    ) -> Result<()> {
        let provider = self.provider(account)?;
        let files = walk_dir(&provider, root).await?.0;
        let total = files.len() as u64;
        for (i, file) in files.iter().enumerate() {
            // 非归档对象会被拒绝,忽略单个错误以便对整层尽力取回。
            let _ = provider.restore(&file.path, days).await;
            progress((i + 1) as u64, total);
        }
        Ok(())
    }

    /// 生成预签名下载链接,`expires_secs` 秒后失效。
    pub async fn presign(&self, account: &str, path: &str, expires_secs: u64) -> Result<String> {
        Ok(self.provider(account)?.presign(path, expires_secs).await?)
    }

    /// 生成预签名**上传**链接(持链接者可直接 PUT 上传到该路径),`expires_secs` 秒后失效。
    pub async fn presign_put(
        &self,
        account: &str,
        path: &str,
        expires_secs: u64,
    ) -> Result<String> {
        Ok(self
            .provider(account)?
            .presign_put(path, expires_secs)
            .await?)
    }

    /// 批量生成预签名链接(用于网格缩略图)。顺序对应 `paths`。
    pub async fn presign_batch(
        &self,
        account: &str,
        paths: &[String],
        expires_secs: u64,
    ) -> Result<Vec<String>> {
        let provider = self.provider(account)?;
        let mut out = Vec::with_capacity(paths.len());
        for p in paths {
            out.push(provider.presign(p, expires_secs).await?);
        }
        Ok(out)
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
        if let Ok(Some(v)) = store.get_setting("rate_limit_kib_per_sec") {
            if let Ok(n) = v.parse() {
                s.rate_limit_kib_per_sec = n;
            }
        }
        s
    }

    /// 保存应用设置到 SQLite,并把限速立即应用到进行中的传输。
    pub fn save_settings(&self, settings: &Settings) -> Result<()> {
        if let Some(store) = &self.store {
            store.set_setting("share_expiry_secs", &settings.share_expiry_secs.to_string())?;
            store.set_setting("concurrency", &settings.concurrency.to_string())?;
            store.set_setting(
                "rate_limit_kib_per_sec",
                &settings.rate_limit_kib_per_sec.to_string(),
            )?;
        }
        self.limits.set_kib_per_sec(settings.rate_limit_kib_per_sec);
        Ok(())
    }

    /// 列出所有收藏(最近的在前)。无存储时返回空。
    pub fn bookmarks(&self) -> Vec<Bookmark> {
        self.store
            .as_ref()
            .and_then(|s| s.list_bookmarks().ok())
            .map(|rows| {
                rows.into_iter()
                    .map(|r| Bookmark {
                        account: r.account,
                        path: r.path,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// 收藏一个位置(账号 + 路径)。无存储时无操作。
    pub fn add_bookmark(&self, account: &str, path: &str) -> Result<()> {
        if let Some(store) = &self.store {
            store.add_bookmark(account, path)?;
        }
        Ok(())
    }

    /// 取消收藏一个位置。无存储时无操作。
    pub fn remove_bookmark(&self, account: &str, path: &str) -> Result<()> {
        if let Some(store) = &self.store {
            store.remove_bookmark(account, path)?;
        }
        Ok(())
    }

    /// 记录一次访问(账号 + 路径),供「最近访问」用;只保留最近若干条。无存储时无操作。
    pub fn record_visit(&self, account: &str, path: &str) -> Result<()> {
        if let Some(store) = &self.store {
            store.record_visit(account, path, RECENT_LOCATIONS_KEEP)?;
        }
        Ok(())
    }

    /// 列出最近访问(最新在前)。无存储时返回空。
    pub fn recent_locations(&self) -> Vec<Bookmark> {
        self.store
            .as_ref()
            .and_then(|s| s.list_recent(RECENT_LOCATIONS_KEEP).ok())
            .map(|rows| {
                rows.into_iter()
                    .map(|r| Bookmark {
                        account: r.account,
                        path: r.path,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// 读取一个界面偏好(主题 / 视图 / 语言 / 侧栏宽度)。无存储或未设置时返回 `None`。
    pub fn get_pref(&self, key: &str) -> Option<String> {
        self.store
            .as_ref()
            .and_then(|s| s.get_pref(key).ok().flatten())
    }

    /// 写入一个界面偏好。无存储时无操作。
    pub fn set_pref(&self, key: &str, value: &str) -> Result<()> {
        if let Some(store) = &self.store {
            store.set_pref(key, value)?;
        }
        Ok(())
    }

    /// 全局传输限速句柄(与 App 各克隆共享)。Tauri 下载循环用它对每块限速。
    pub fn transfer_limits(&self) -> TransferLimits {
        self.limits.clone()
    }

    /// 列出持久化的传输任务(重启后恢复面板用)。无存储时返回空。
    pub fn transfers(&self) -> Vec<TransferRecord> {
        self.store
            .as_ref()
            .and_then(|s| s.list_transfers().ok())
            .unwrap_or_default()
    }

    /// 写入(或覆盖)一条传输任务。无存储时无操作。
    pub fn save_transfer(&self, record: &TransferRecord) {
        if let Some(store) = &self.store {
            let _ = store.put_transfer(record);
        }
    }

    /// 删除一条传输任务(完成或清除时)。无存储时无操作。
    pub fn delete_transfer(&self, id: &str) {
        if let Some(store) = &self.store {
            let _ = store.delete_transfer(id);
        }
    }

    /// 按 id 解析 provider,未注册则报 [`AppError::NoSuchProvider`]。
    fn provider(&self, account: &str) -> Result<Arc<dyn StorageProvider>> {
        self.registry
            .get(account)
            .ok_or_else(|| AppError::NoSuchProvider(account.to_string()))
    }
}

/// 递归遍历 `root` 下所有条目,分页进行,`SEARCH_SCAN_LIMIT` 兜底防失控。
/// 返回 `(文件条目, 目录路径)`;触及扫描上限时尽力而为地提前返回已收集部分。
async fn walk_dir(
    provider: &Arc<dyn StorageProvider>,
    root: &str,
) -> Result<(Vec<Entry>, Vec<String>)> {
    let mut files = Vec::new();
    let mut dirs = Vec::new();
    let mut scanned = 0usize;
    let mut queue = std::collections::VecDeque::new();
    queue.push_back(root.to_string());
    while let Some(dir) = queue.pop_front() {
        let mut cursor = None;
        loop {
            let page = provider.list_page(&dir, cursor).await?;
            for entry in page.entries {
                scanned += 1;
                if entry.is_dir() {
                    dirs.push(entry.path.clone());
                    queue.push_back(entry.path);
                } else {
                    files.push(entry);
                }
            }
            if scanned >= SEARCH_SCAN_LIMIT {
                return Ok((files, dirs));
            }
            match page.cursor {
                Some(next) => cursor = Some(next),
                None => break,
            }
        }
    }
    Ok((files, dirs))
}

/// 把对象 key 编码进 URL path:保留 `/` 与 RFC3986 unreserved 字符,其余(空格 / 中文 /
/// 特殊字符,按 UTF-8 字节)百分号编码。用于自定义域名的公共直链。
fn encode_url_path(key: &str) -> String {
    let mut out = String::with_capacity(key.len());
    for b in key.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// 保证路径以 `/` 结尾(桶 / 前缀作为目录前缀使用)。
fn ensure_trailing_slash(p: &str) -> String {
    if p.ends_with('/') {
        p.to_string()
    } else {
        format!("{p}/")
    }
}

/// 取目录路径最后一段作为文件夹名(忽略结尾斜杠)。
fn folder_name(p: &str) -> &str {
    p.trim_end_matches('/').rsplit('/').next().unwrap_or(p)
}

/// 目标目录是否落在源目录内部(自身或子目录)。两参数均须以 `/` 结尾,
/// 结尾斜杠可避免 `a/b/` 与 `a/bc/` 之间的前缀误判。
fn is_within(src_base: &str, dst_base: &str) -> bool {
    dst_base.starts_with(src_base)
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use nebula_provider::{Capabilities, EntryKind, ProviderError};
    use std::collections::HashMap;
    use std::sync::Mutex;

    fn rule(mode: &str, a: &str, b: &str) -> RenameRule {
        RenameRule {
            mode: mode.into(),
            a: a.into(),
            b: b.into(),
        }
    }

    #[test]
    fn batch_rename_prefix_suffix_replace() {
        let paths = vec![
            "bkt/photos/a.jpg".to_string(),
            "bkt/photos/b.png".to_string(),
            "bkt/photos/README".to_string(),
        ];

        // 前缀:只改基名,父前缀不变。
        let p = plan_batch_rename(&paths, &rule("prefix", "2026_", ""));
        assert_eq!(p[0].to, "bkt/photos/2026_a.jpg");
        assert_eq!(p[2].to, "bkt/photos/2026_README");

        // 后缀:插在扩展名之前;无扩展名则直接追加。
        let p = plan_batch_rename(&paths, &rule("suffix", "-v2", ""));
        assert_eq!(p[0].to, "bkt/photos/a-v2.jpg");
        assert_eq!(p[2].to, "bkt/photos/README-v2");

        // 查找替换:只在基名里替换(父前缀不动),不匹配的项被跳过。
        let p = plan_batch_rename(&paths, &rule("replace", ".jpg", ".jpeg"));
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].to, "bkt/photos/a.jpeg");
    }

    #[test]
    fn batch_rename_skips_noops_and_dirs() {
        let paths = vec![
            "bkt/keep.txt".to_string(),
            "bkt/sub/".to_string(), // 目录项:跳过
        ];
        // 查找串不匹配 → 结果不变 → 该项被跳过;目录项也被跳过。
        let p = plan_batch_rename(&paths, &rule("replace", "zzz", "q"));
        assert!(p.is_empty());
        // 空查找串:replace 视为无操作。
        let p = plan_batch_rename(&paths, &rule("replace", "", "x"));
        assert!(p.is_empty());
    }

    #[test]
    fn hidden_dotfile_has_no_extension() {
        let paths = vec!["bkt/.gitignore".to_string()];
        let p = plan_batch_rename(&paths, &rule("suffix", "-bak", ""));
        assert_eq!(p[0].to, "bkt/.gitignore-bak");
    }

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
                // 模拟对象存储:整对象上传的 ETag 即内容 MD5,供完整性校验测试用。
                .map(|v| {
                    Entry::file(path.to_string(), v.len() as u64)
                        .with_etag(cloud_core::crypto::md5_hex(v))
                })
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

    /// 固定层级的只读 provider,用于测试递归搜索的逐层下降。
    struct TreeProvider;

    #[async_trait]
    impl StorageProvider for TreeProvider {
        fn id(&self) -> &str {
            "tree"
        }
        fn capabilities(&self) -> Capabilities {
            Capabilities::default()
        }
        async fn list(&self, path: &str) -> nebula_provider::Result<Vec<Entry>> {
            let entries = match path {
                "b" => vec![
                    Entry::directory("b/photos"),
                    Entry::directory("b/docs"),
                    Entry::file("b/readme.txt", 1),
                ],
                "b/photos" => vec![
                    Entry::file("b/photos/cat.jpg", 1),
                    Entry::file("b/photos/dog.png", 1),
                ],
                "b/docs" => vec![Entry::file("b/docs/cat-notes.md", 1)],
                _ => vec![],
            };
            Ok(entries)
        }
        async fn stat(&self, path: &str) -> nebula_provider::Result<Entry> {
            Ok(Entry::file(path.to_string(), 1))
        }
        async fn read(&self, _path: &str) -> nebula_provider::Result<Bytes> {
            Ok(Bytes::new())
        }
        async fn write(
            &self,
            _path: &str,
            _data: Bytes,
            _ct: Option<&str>,
        ) -> nebula_provider::Result<()> {
            Ok(())
        }
        async fn delete(&self, _path: &str) -> nebula_provider::Result<()> {
            Ok(())
        }
    }

    /// 与 [`TreeProvider`] 同构的层级 provider,但记录 copy / delete 调用,
    /// 用于验证文件夹级迁移 / 删除的路径映射与遍历。
    struct RecordingTree {
        copied: Mutex<Vec<(String, String)>>,
        deleted: Mutex<Vec<String>>,
        classed: Mutex<Vec<(String, String)>>,
        restored: Mutex<Vec<(String, u32)>>,
    }

    impl RecordingTree {
        fn new() -> Self {
            Self {
                copied: Mutex::new(Vec::new()),
                deleted: Mutex::new(Vec::new()),
                classed: Mutex::new(Vec::new()),
                restored: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl StorageProvider for RecordingTree {
        fn id(&self) -> &str {
            "rec"
        }
        fn capabilities(&self) -> Capabilities {
            Capabilities::default()
        }
        async fn list(&self, path: &str) -> nebula_provider::Result<Vec<Entry>> {
            let entries = match path {
                "b" => vec![Entry::directory("b/photos"), Entry::file("b/readme.txt", 1)],
                "b/photos" => vec![
                    Entry::file("b/photos/cat.jpg", 1),
                    Entry::file("b/photos/dog.png", 1),
                ],
                _ => vec![],
            };
            Ok(entries)
        }
        async fn stat(&self, path: &str) -> nebula_provider::Result<Entry> {
            Ok(Entry::file(path.to_string(), 1))
        }
        async fn read(&self, _path: &str) -> nebula_provider::Result<Bytes> {
            Ok(Bytes::new())
        }
        async fn write(
            &self,
            _path: &str,
            _data: Bytes,
            _ct: Option<&str>,
        ) -> nebula_provider::Result<()> {
            Ok(())
        }
        async fn copy(&self, from: &str, to: &str) -> nebula_provider::Result<()> {
            self.copied
                .lock()
                .unwrap()
                .push((from.to_string(), to.to_string()));
            Ok(())
        }
        async fn delete(&self, path: &str) -> nebula_provider::Result<()> {
            self.deleted.lock().unwrap().push(path.to_string());
            Ok(())
        }
        async fn set_storage_class(&self, path: &str, class: &str) -> nebula_provider::Result<()> {
            self.classed
                .lock()
                .unwrap()
                .push((path.to_string(), class.to_string()));
            Ok(())
        }
        async fn restore(&self, path: &str, days: u32) -> nebula_provider::Result<()> {
            self.restored.lock().unwrap().push((path.to_string(), days));
            Ok(())
        }
    }

    #[tokio::test]
    async fn set_storage_class_folder_applies_to_every_file() {
        let app = App::new();
        let rec = Arc::new(RecordingTree::new());
        app.add_account(rec.clone());
        app.set_storage_class_folder("rec", "b", "ARCHIVE", &|_, _| {})
            .await
            .unwrap();
        let mut classed = rec.classed.lock().unwrap().clone();
        classed.sort();
        assert_eq!(
            classed,
            [
                ("b/photos/cat.jpg".into(), "ARCHIVE".into()),
                ("b/photos/dog.png".into(), "ARCHIVE".into()),
                ("b/readme.txt".into(), "ARCHIVE".into()),
            ]
        );
    }

    #[tokio::test]
    async fn restore_folder_requests_every_file() {
        let app = App::new();
        let rec = Arc::new(RecordingTree::new());
        app.add_account(rec.clone());
        app.restore_folder("rec", "b/photos", 3, &|_, _| {})
            .await
            .unwrap();
        let mut restored = rec.restored.lock().unwrap().clone();
        restored.sort();
        assert_eq!(
            restored,
            [
                ("b/photos/cat.jpg".into(), 3),
                ("b/photos/dog.png".into(), 3),
            ]
        );
    }

    #[tokio::test]
    async fn list_all_files_collects_every_file_under_prefix() {
        let app = App::new();
        app.add_account(Arc::new(TreeProvider));
        let mut names: Vec<_> = app
            .list_all_files("tree", "b")
            .await
            .unwrap()
            .into_iter()
            .map(|e| e.path)
            .collect();
        names.sort();
        assert_eq!(
            names,
            [
                "b/docs/cat-notes.md",
                "b/photos/cat.jpg",
                "b/photos/dog.png",
                "b/readme.txt"
            ]
        );
    }

    #[tokio::test]
    async fn folder_stats_counts_files_and_bytes() {
        let app = App::new();
        app.add_account(Arc::new(TreeProvider));
        // TreeProvider 下 4 个文件,每个 1 字节。
        let stats = app.folder_stats("tree", "b").await.unwrap();
        assert_eq!(stats.files, 4);
        assert_eq!(stats.bytes, 4);
        assert!(!stats.truncated);
    }

    #[tokio::test]
    async fn migrate_folder_maps_paths_under_named_subdir() {
        let app = App::new();
        let rec = Arc::new(RecordingTree::new());
        app.add_account(rec.clone());

        // 把 b/photos 迁移到 arch/ 下 → 落到 arch/photos/... 保留相对结构。
        app.migrate_folder(
            "rec",
            "b/photos",
            "rec",
            "arch",
            Arc::new(AtomicBool::new(false)),
            &|_, _| {},
        )
        .await
        .unwrap();
        let mut copied = rec.copied.lock().unwrap().clone();
        copied.sort();
        assert_eq!(
            copied,
            [
                ("b/photos/cat.jpg".into(), "arch/photos/cat.jpg".into()),
                ("b/photos/dog.png".into(), "arch/photos/dog.png".into()),
            ]
        );
    }

    #[tokio::test]
    async fn move_folder_copies_to_new_prefix_and_deletes_originals() {
        let app = App::new();
        let rec = Arc::new(RecordingTree::new());
        app.add_account(rec.clone());

        // 把 b/photos 重命名为 b/archive → 内容改前缀,原对象与目录占位被删。
        app.move_folder(
            "rec",
            "b/photos",
            "b/archive",
            Arc::new(AtomicBool::new(false)),
            &|_, _| {},
        )
        .await
        .unwrap();

        let mut copied = rec.copied.lock().unwrap().clone();
        copied.sort();
        assert_eq!(
            copied,
            [
                ("b/photos/cat.jpg".into(), "b/archive/cat.jpg".into()),
                ("b/photos/dog.png".into(), "b/archive/dog.png".into()),
            ]
        );
        let deleted = rec.deleted.lock().unwrap().clone();
        assert!(deleted.contains(&"b/photos/cat.jpg".to_string()));
        assert!(deleted.contains(&"b/photos/dog.png".to_string()));
        assert!(deleted.contains(&"b/photos/".to_string())); // 源目录占位也清掉
    }

    #[tokio::test]
    async fn copy_folder_duplicates_to_new_prefix_without_deleting() {
        let app = App::new();
        let rec = Arc::new(RecordingTree::new());
        app.add_account(rec.clone());

        app.copy_folder(
            "rec",
            "b/photos",
            "b/photos-copy",
            Arc::new(AtomicBool::new(false)),
            &|_, _| {},
        )
        .await
        .unwrap();

        let mut copied = rec.copied.lock().unwrap().clone();
        copied.sort();
        assert_eq!(
            copied,
            [
                ("b/photos/cat.jpg".into(), "b/photos-copy/cat.jpg".into()),
                ("b/photos/dog.png".into(), "b/photos-copy/dog.png".into()),
            ]
        );
        // 复制不删除源。
        assert!(rec.deleted.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn move_folder_rejects_moving_into_itself() {
        let app = App::new();
        let rec = Arc::new(RecordingTree::new());
        app.add_account(rec.clone());

        let err = app
            .move_folder(
                "rec",
                "b/photos",
                "b/photos/sub",
                Arc::new(AtomicBool::new(false)),
                &|_, _| {},
            )
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::InvalidInput(_)));
        // 未发生任何复制 / 删除。
        assert!(rec.copied.lock().unwrap().is_empty());
        assert!(rec.deleted.lock().unwrap().is_empty());
    }

    #[test]
    fn is_within_uses_trailing_slash_to_avoid_prefix_confusion() {
        assert!(is_within("a/b/", "a/b/")); // 自身
        assert!(is_within("a/b/", "a/b/c/")); // 子目录
        assert!(!is_within("a/b/", "a/bc/")); // 仅前缀相似,不算
        assert!(!is_within("a/b/", "a/c/")); // 无关
    }

    #[test]
    fn encode_url_path_keeps_slash_and_encodes_specials() {
        assert_eq!(encode_url_path("dir/a.jpg"), "dir/a.jpg");
        assert_eq!(encode_url_path("dir/a b.txt"), "dir/a%20b.txt");
        assert_eq!(encode_url_path("图片.png"), "%E5%9B%BE%E7%89%87.png");
    }

    #[tokio::test]
    async fn delete_folder_removes_files_and_the_dir_marker() {
        let app = App::new();
        let rec = Arc::new(RecordingTree::new());
        app.add_account(rec.clone());

        app.delete_folder("rec", "b/photos", &|_, _| {})
            .await
            .unwrap();
        let deleted = rec.deleted.lock().unwrap().clone();
        assert!(deleted.contains(&"b/photos/cat.jpg".to_string()));
        assert!(deleted.contains(&"b/photos/dog.png".to_string()));
        // 目录占位对象也被清掉。
        assert!(deleted.contains(&"b/photos/".to_string()));
    }

    #[test]
    fn folder_path_helpers() {
        assert_eq!(ensure_trailing_slash("a/b"), "a/b/");
        assert_eq!(ensure_trailing_slash("a/b/"), "a/b/");
        assert_eq!(folder_name("bucket/photos/"), "photos");
        assert_eq!(folder_name("bucket/photos"), "photos");
    }

    #[tokio::test]
    async fn search_descends_recursively_and_matches_by_name() {
        let app = App::new();
        app.add_account(Arc::new(TreeProvider));

        // "cat" 命中两层深处的两个文件(photos/cat.jpg 与 docs/cat-notes.md)。
        let hits = app
            .search("tree", "b", "cat", &SearchFilter::default(), 100)
            .await
            .unwrap();
        let mut names: Vec<_> = hits.entries.iter().map(|e| e.name.clone()).collect();
        names.sort();
        assert_eq!(names, ["cat-notes.md", "cat.jpg"]);
        // 只返回文件,不含目录;正常穷尽,不截断。
        assert!(hits.entries.iter().all(|e| !e.is_dir()));
        assert!(!hits.truncated);
    }

    #[tokio::test]
    async fn search_empty_query_returns_all_files_capped() {
        let app = App::new();
        app.add_account(Arc::new(TreeProvider));

        // 空查询返回全部文件(readme + cat.jpg + dog.png + cat-notes.md = 4)。
        let all = app
            .search("tree", "b", "", &SearchFilter::default(), 100)
            .await
            .unwrap();
        assert_eq!(all.entries.len(), 4);
        assert!(!all.truncated);
        // 结果数封顶生效,并标记结果不完整。
        let capped = app
            .search("tree", "b", "", &SearchFilter::default(), 2)
            .await
            .unwrap();
        assert_eq!(capped.entries.len(), 2);
        assert!(capped.truncated);
    }

    #[tokio::test]
    async fn search_filters_by_extension_and_size() {
        let app = App::new();
        app.add_account(Arc::new(TreeProvider));

        // 只要 .jpg → 命中 photos/cat.jpg。
        let jpg = SearchFilter {
            ext: Some("jpg".into()),
            ..Default::default()
        };
        let hits = app.search("tree", "b", "", &jpg, 100).await.unwrap();
        let names: Vec<_> = hits.entries.iter().map(|e| e.name.clone()).collect();
        assert_eq!(names, ["cat.jpg"]);

        // TreeProvider 里每个文件都是 1 字节;min_size=2 → 全过滤掉。
        let big = SearchFilter {
            min_size: Some(2),
            ..Default::default()
        };
        let none = app.search("tree", "b", "", &big, 100).await.unwrap();
        assert!(none.entries.is_empty());
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
    async fn verify_passes_for_intact_object() {
        let app = app_with_memory();
        app.upload(
            "mem",
            "b/k.txt",
            Bytes::from_static(b"hello app-core"),
            None,
        )
        .await
        .unwrap();
        // MemoryProvider 的 stat 返回内容 MD5 作为 ETag → 校验应通过。
        assert_eq!(
            app.verify("mem", "b/k.txt").await.unwrap(),
            Integrity::Verified
        );
    }

    #[tokio::test]
    async fn storage_class_ops_unsupported_by_default() {
        // MemoryProvider 不覆盖 set_storage_class / restore → 走 trait 默认(Unsupported),
        // 上层安全地报错而非 panic。真实厂商适配层覆盖它们。
        let app = app_with_memory();
        app.upload("mem", "b/k", Bytes::from_static(b"x"), None)
            .await
            .unwrap();
        assert!(app
            .set_storage_class("mem", "b/k", "ARCHIVE")
            .await
            .is_err());
        assert!(app.restore("mem", "b/k", 1).await.is_err());
        // bucket 与元数据操作同样默认 Unsupported。
        assert!(app.create_bucket("mem", "newb").await.is_err());
        assert!(app.delete_bucket("mem", "newb").await.is_err());
        assert!(app
            .set_content_type("mem", "b/k", "image/png")
            .await
            .is_err());
    }

    #[tokio::test]
    async fn copy_across_moves_object_between_accounts() {
        let app = App::new();
        app.add_account(Arc::new(MemoryProvider::new("src")));
        app.add_account(Arc::new(MemoryProvider::new("dst")));
        let data = Bytes::from_static(b"cross-cloud payload");
        app.upload("src", "b/from.bin", data.clone(), None)
            .await
            .unwrap();

        app.copy_across("src", "b/from.bin", "dst", "b/to.bin")
            .await
            .unwrap();

        // 目标账号拿到了副本,源对象仍在。
        assert_eq!(app.download("dst", "b/to.bin").await.unwrap(), data);
        assert_eq!(app.download("src", "b/from.bin").await.unwrap(), data);
    }

    #[tokio::test]
    async fn copy_across_same_account_uses_server_side_copy() {
        let app = app_with_memory();
        let data = Bytes::from_static(b"same-account");
        app.upload("mem", "b/a.txt", data.clone(), None)
            .await
            .unwrap();

        let seen = std::sync::Mutex::new(None);
        app.copy_across_with_progress(
            "mem",
            "b/a.txt",
            "mem",
            "b/b.txt",
            Arc::new(AtomicBool::new(false)),
            &|done, total| {
                *seen.lock().unwrap() = Some((done, total));
            },
        )
        .await
        .unwrap();

        assert_eq!(app.download("mem", "b/b.txt").await.unwrap(), data);
        // 进度回调以总大小收尾。
        assert_eq!(
            seen.into_inner().unwrap(),
            Some((data.len() as u64, data.len() as u64))
        );
    }

    /// 分块 provider:`read_stream` 把对象切成 4 字节小块逐个吐出,`write_stream` 覆盖为
    /// 逐块消费并记录拉到的块数——用于证明跨账号迁移走的是流式路径(而非整块读+写)。
    struct ChunkedProvider {
        id: String,
        store: Mutex<HashMap<String, Bytes>>,
        last_write_chunks: Arc<std::sync::atomic::AtomicUsize>,
    }

    impl ChunkedProvider {
        fn new(id: &str) -> Self {
            Self {
                id: id.to_string(),
                store: Mutex::new(HashMap::new()),
                last_write_chunks: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            }
        }
    }

    #[async_trait]
    impl StorageProvider for ChunkedProvider {
        fn id(&self) -> &str {
            &self.id
        }
        fn capabilities(&self) -> Capabilities {
            Capabilities::default()
        }
        async fn list(&self, _path: &str) -> nebula_provider::Result<Vec<Entry>> {
            Ok(vec![])
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
        async fn read_stream(
            &self,
            path: &str,
        ) -> nebula_provider::Result<(Option<u64>, nebula_provider::ByteStream)> {
            let data = self.read(path).await?;
            let len = data.len() as u64;
            let chunks: Vec<nebula_provider::Result<Bytes>> = (0..data.len())
                .step_by(4)
                .map(|i| Ok(data.slice(i..(i + 4).min(data.len()))))
                .collect();
            Ok((Some(len), Box::pin(futures::stream::iter(chunks))))
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
        async fn write_stream(
            &self,
            path: &str,
            _len: Option<u64>,
            mut stream: nebula_provider::ByteStream,
            _ct: Option<&str>,
            progress: ProgressFn<'_>,
        ) -> nebula_provider::Result<()> {
            use futures::StreamExt;
            let mut buf = bytes::BytesMut::new();
            let mut n = 0usize;
            while let Some(chunk) = stream.next().await {
                buf.extend_from_slice(&chunk?);
                n += 1;
            }
            self.last_write_chunks
                .store(n, std::sync::atomic::Ordering::SeqCst);
            let total = buf.len() as u64;
            self.store
                .lock()
                .unwrap()
                .insert(path.to_string(), buf.freeze());
            progress(total, total);
            Ok(())
        }
        async fn delete(&self, path: &str) -> nebula_provider::Result<()> {
            self.store.lock().unwrap().remove(path);
            Ok(())
        }
    }

    #[tokio::test]
    async fn copy_across_streams_in_chunks() {
        let app = App::new();
        let dst = Arc::new(ChunkedProvider::new("dst"));
        let dst_chunks = dst.last_write_chunks.clone();
        app.add_account(Arc::new(ChunkedProvider::new("src")));
        app.add_account(dst);

        let data = Bytes::from(vec![7u8; 20]); // 20 字节 → read_stream 切成 5 块
        app.upload("src", "b/f.bin", data.clone(), None)
            .await
            .unwrap();

        app.copy_across("src", "b/f.bin", "dst", "b/g.bin")
            .await
            .unwrap();

        // 目标拿到完整数据,且是分多块流式喂进来的(证明没走整块缓冲)。
        assert_eq!(app.download("dst", "b/g.bin").await.unwrap(), data);
        assert_eq!(dst_chunks.load(std::sync::atomic::Ordering::SeqCst), 5);
    }

    /// 分页 provider:3 页、每页 2 个,游标为页索引字符串。
    struct PagedProvider;

    #[async_trait]
    impl StorageProvider for PagedProvider {
        fn id(&self) -> &str {
            "paged"
        }
        fn capabilities(&self) -> Capabilities {
            Capabilities::default()
        }
        async fn list(&self, _path: &str) -> nebula_provider::Result<Vec<Entry>> {
            Ok(vec![])
        }
        async fn list_page(
            &self,
            _path: &str,
            cursor: Option<String>,
        ) -> nebula_provider::Result<Page> {
            let pages = [["a", "b"], ["c", "d"], ["e", "f"]];
            let idx: usize = cursor.as_deref().and_then(|c| c.parse().ok()).unwrap_or(0);
            let entries = pages[idx]
                .iter()
                .map(|n| Entry::file(format!("b/{n}"), 1))
                .collect();
            let next = (idx + 1 < pages.len()).then(|| (idx + 1).to_string());
            Ok(Page {
                entries,
                cursor: next,
            })
        }
        async fn stat(&self, path: &str) -> nebula_provider::Result<Entry> {
            Ok(Entry::file(path.to_string(), 1))
        }
        async fn read(&self, _path: &str) -> nebula_provider::Result<Bytes> {
            Ok(Bytes::new())
        }
        async fn write(
            &self,
            _path: &str,
            _data: Bytes,
            _ct: Option<&str>,
        ) -> nebula_provider::Result<()> {
            Ok(())
        }
        async fn delete(&self, _path: &str) -> nebula_provider::Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn browse_page_threads_cursor_across_pages() {
        let app = App::new();
        app.add_account(Arc::new(PagedProvider));

        let p0 = app.browse_page("paged", "b", None).await.unwrap();
        assert_eq!(p0.entries.len(), 2);
        assert_eq!(p0.cursor.as_deref(), Some("1"));
        let p1 = app.browse_page("paged", "b", p0.cursor).await.unwrap();
        assert_eq!(p1.cursor.as_deref(), Some("2"));
        let p2 = app.browse_page("paged", "b", p1.cursor).await.unwrap();
        // 末页无下一页游标。
        assert_eq!(p2.entries.len(), 2);
        assert!(p2.cursor.is_none());
    }

    #[tokio::test]
    async fn browse_page_default_returns_single_page() {
        let app = app_with_memory();
        app.upload("mem", "x", Bytes::from_static(b"1"), None)
            .await
            .unwrap();
        // 未覆盖 list_page 的 provider:一次列全,无下一页游标。
        let page = app.browse_page("mem", "", None).await.unwrap();
        assert!(page.cursor.is_none());
        assert_eq!(page.entries.len(), 1);
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
            s.rate_limit_kib_per_sec = 2048;
            app.save_settings(&s).unwrap();
            // 保存后限速立即生效在共享令牌桶上。
            assert_eq!(app.transfer_limits().kib_per_sec(), 2048);
        }
        {
            let app = App::with_store_and_secrets(&path, secrets.clone()).unwrap();
            let s = app.settings();
            assert_eq!(s.share_expiry_secs, 1800);
            assert_eq!(s.concurrency, 5);
            assert_eq!(s.rate_limit_kib_per_sec, 2048);
            // 启动时从持久化设置恢复限速。
            assert_eq!(app.transfer_limits().kib_per_sec(), 2048);
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn huawei_account_registers_provider() {
        let path = temp_db("huawei");
        let secrets: Arc<dyn SecretStore> = Arc::new(MemorySecrets::default());
        {
            let app = App::with_store_and_secrets(&path, secrets.clone()).unwrap();
            app.add_huawei_account("obs-acc", "ak", "sk", "obs.cn-north-4.myhuaweicloud.com")
                .unwrap();
            assert_eq!(app.accounts(), vec!["obs-acc"]);
        }
        {
            // 重开:华为账号应从库 + 密钥库重新注册。
            let app = App::with_store_and_secrets(&path, secrets.clone()).unwrap();
            assert_eq!(app.accounts(), vec!["obs-acc"]);
            assert_eq!(
                app.account_info("obs-acc").unwrap().vendor,
                VENDOR_HUAWEI.to_string()
            );
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
