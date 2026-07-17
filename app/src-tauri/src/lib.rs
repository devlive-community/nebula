//! Nebula 桌面应用后端:把 [`app_core::App`] 的方法包成 Tauri command。
//!
//! 业务逻辑都在 `app-core`(可 `cargo test`),这里只做 JS ↔ Rust 的桥接与本地文件读写。

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use app_core::{
    AccountInfo, App, Bookmark, EditSave, ExifInfo, FolderStats, ImageData, IncompleteUpload,
    Integrity, Ops, Page, RenamePlan, RenameRule, SearchResult, Settings, StorageBreakdown,
    TextPreview, TransferRecord,
};
use bytes::Bytes;
use nebula_provider::Entry;
use serde::Serialize;
use tauri::menu::{MenuBuilder, MenuItemBuilder, SubmenuBuilder};
use tauri::{AppHandle, Emitter, Manager, State};

/// 进行中传输的取消登记表:传输 id → 取消标志。前端点"取消"时置位对应标志,
/// 传输循环在分片 / 数据块之间检查后中止。
#[derive(Default)]
struct Transfers {
    map: Mutex<HashMap<String, Arc<AtomicBool>>>,
}

impl Transfers {
    /// 登记一个传输,返回其取消标志(初始 false)。
    fn begin(&self, id: &str) -> Arc<AtomicBool> {
        let flag = Arc::new(AtomicBool::new(false));
        self.map
            .lock()
            .unwrap()
            .insert(id.to_string(), flag.clone());
        flag
    }
    /// 请求取消某传输(若在册)。
    fn cancel(&self, id: &str) {
        if let Some(f) = self.map.lock().unwrap().get(id) {
            f.store(true, Ordering::Relaxed);
        }
    }
    /// 注销传输(完成 / 出错 / 取消后)。
    fn end(&self, id: &str) {
        self.map.lock().unwrap().remove(id);
    }
}

/// 作用域退出即注销传输,确保任意返回路径都清理登记表。
struct CancelGuard<'a> {
    transfers: &'a Transfers,
    id: String,
}
impl Drop for CancelGuard<'_> {
    fn drop(&mut self) {
        self.transfers.end(&self.id);
    }
}

/// 请求取消一个进行中的传输(下载 / 上传 / 文件夹下载)。
#[tauri::command]
fn cancel_transfer(transfers: State<'_, Transfers>, id: String) {
    transfers.cancel(&id);
}

/// 列出持久化的传输任务(重启后恢复面板)。
#[tauri::command]
fn list_transfers(state: State<'_, App>) -> Vec<TransferRecord> {
    state.transfers()
}

/// 写入(或覆盖)一条传输任务。
#[tauri::command]
fn save_transfer(state: State<'_, App>, record: TransferRecord) {
    state.save_transfer(&record);
}

/// 删除一条持久化传输任务(完成或清除时)。
#[tauri::command]
fn delete_transfer(state: State<'_, App>, id: String) {
    state.delete_transfer(&id);
}

/// 上传进度事件负载,发往前端 `upload-progress`。
#[derive(Clone, Serialize)]
struct UploadProgress {
    path: String,
    uploaded: u64,
    total: u64,
}

/// 下载进度事件负载,发往前端 `download-progress`。
#[derive(Clone, Serialize)]
struct DownloadProgress {
    path: String,
    downloaded: u64,
    total: u64,
}

/// 跨账号迁移进度事件负载,发往前端 `transfer-progress`。
#[derive(Clone, Serialize)]
struct TransferProgress {
    /// 目标端路径,前端以此定位进度条。
    to: String,
    transferred: u64,
    total: u64,
}

/// 文件夹级操作进度事件负载,发往前端 `folder-progress`。以**文件数**计量(非字节)。
#[derive(Clone, Serialize)]
struct FolderProgress {
    /// 操作类型:`download` / `migrate` / `delete`。
    op: String,
    /// 被操作的文件夹路径,前端以此定位进度条。
    path: String,
    /// 已完成文件数。
    done: u64,
    /// 总文件数。
    total: u64,
}

/// 列出已注册账号(仅 id)。
#[tauri::command]
fn list_accounts(state: State<'_, App>) -> Vec<String> {
    state.accounts()
}

/// 列出账号的非敏感信息(id + 厂商 + endpoint),供 UI 按厂商展示图标。
#[tauri::command]
fn list_account_infos(state: State<'_, App>) -> Vec<AccountInfo> {
    state.account_infos()
}

/// 新增一个阿里云 OSS 账号(持久化到 SQLite)。
#[tauri::command]
fn add_aliyun_account(
    state: State<'_, App>,
    id: String,
    access_key_id: String,
    access_key_secret: String,
    endpoint: String,
) -> Result<(), String> {
    state
        .add_aliyun_account(id, access_key_id, access_key_secret, endpoint)
        .map_err(|e| e.to_string())
}

/// 新增一个华为云 OBS 账号(持久化到 SQLite)。
#[tauri::command]
fn add_huawei_account(
    state: State<'_, App>,
    id: String,
    access_key_id: String,
    access_key_secret: String,
    endpoint: String,
) -> Result<(), String> {
    state
        .add_huawei_account(id, access_key_id, access_key_secret, endpoint)
        .map_err(|e| e.to_string())
}

/// 新增一个七牛云 Kodo 账号(S3 兼容,持久化到 SQLite)。
#[tauri::command]
fn add_qiniu_account(
    state: State<'_, App>,
    id: String,
    access_key_id: String,
    access_key_secret: String,
    endpoint: String,
) -> Result<(), String> {
    state
        .add_qiniu_account(id, access_key_id, access_key_secret, endpoint)
        .map_err(|e| e.to_string())
}

/// 新增一个 AWS S3 账号(持久化到 SQLite)。
#[tauri::command]
fn add_aws_account(
    state: State<'_, App>,
    id: String,
    access_key_id: String,
    access_key_secret: String,
    endpoint: String,
) -> Result<(), String> {
    state
        .add_aws_account(id, access_key_id, access_key_secret, endpoint)
        .map_err(|e| e.to_string())
}

/// 新增一个 Cloudflare R2 账号(持久化到 SQLite)。
#[tauri::command]
fn add_r2_account(
    state: State<'_, App>,
    id: String,
    access_key_id: String,
    access_key_secret: String,
    endpoint: String,
) -> Result<(), String> {
    state
        .add_r2_account(id, access_key_id, access_key_secret, endpoint)
        .map_err(|e| e.to_string())
}

/// 新增一个 MinIO 账号(持久化到 SQLite)。
#[tauri::command]
fn add_minio_account(
    state: State<'_, App>,
    id: String,
    access_key_id: String,
    access_key_secret: String,
    endpoint: String,
) -> Result<(), String> {
    state
        .add_minio_account(id, access_key_id, access_key_secret, endpoint)
        .map_err(|e| e.to_string())
}

/// 新增一个腾讯云 COS 账号(持久化到 SQLite)。access_key_id 传 SecretId。
#[tauri::command]
fn add_tencent_account(
    state: State<'_, App>,
    id: String,
    access_key_id: String,
    access_key_secret: String,
    endpoint: String,
) -> Result<(), String> {
    state
        .add_tencent_account(id, access_key_id, access_key_secret, endpoint)
        .map_err(|e| e.to_string())
}

/// 移除账号(从注册表与 SQLite)。
#[tauri::command]
fn remove_account(state: State<'_, App>, id: String) -> Result<bool, String> {
    state.remove_account(&id).map_err(|e| e.to_string())
}

/// 设置某账号的自定义公共域名(CDN / CNAME);空串清除。
#[tauri::command]
fn set_account_domain(state: State<'_, App>, id: String, domain: String) -> Result<(), String> {
    state
        .set_account_domain(&id, &domain)
        .map_err(|e| e.to_string())
}

/// 读取账号非敏感信息(编辑回填用)。
#[tauri::command]
fn get_account(state: State<'_, App>, id: String) -> Option<AccountInfo> {
    state.account_info(&id)
}

/// 浏览某账号下某路径(桶 / 前缀)。
#[tauri::command]
async fn browse(
    state: State<'_, App>,
    account: String,
    path: String,
) -> Result<Vec<Entry>, String> {
    let app = state.inner().clone();
    app.browse(&account, &path).await.map_err(|e| e.to_string())
}

/// 分页浏览:返回某路径下的一页条目 + 下一页游标(`cursor` 为空取第一页)。
#[tauri::command]
async fn browse_page(
    state: State<'_, App>,
    account: String,
    path: String,
    cursor: Option<String>,
) -> Result<Page, String> {
    let app = state.inner().clone();
    app.browse_page(&account, &path, cursor)
        .await
        .map_err(|e| e.to_string())
}

/// 读取对象元信息。
#[tauri::command]
async fn stat(state: State<'_, App>, account: String, path: String) -> Result<Entry, String> {
    let app = state.inner().clone();
    app.stat(&account, &path).await.map_err(|e| e.to_string())
}

/// 校验对象内容完整性:下载内容并与远端 ETag(整对象上传即为 MD5)比对。
#[tauri::command]
async fn verify_object(
    state: State<'_, App>,
    account: String,
    path: String,
) -> Result<Integrity, String> {
    let app = state.inner().clone();
    app.verify(&account, &path).await.map_err(|e| e.to_string())
}

/// 在某账号的 `root`(桶 / 前缀)下递归搜索名字包含 `query` 的文件,最多 `max_results` 条。
#[tauri::command]
async fn search(
    state: State<'_, App>,
    account: String,
    root: String,
    query: String,
    min_size: Option<u64>,
    ext: Option<String>,
    max_results: usize,
) -> Result<SearchResult, String> {
    let app = state.inner().clone();
    let filter = app_core::SearchFilter { min_size, ext };
    app.search(&account, &root, &query, &filter, max_results)
        .await
        .map_err(|e| e.to_string())
}

/// 把本地文件上传到远端路径。
#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn upload_file(
    app: AppHandle,
    state: State<'_, App>,
    transfers: State<'_, Transfers>,
    transfer_id: String,
    account: String,
    remote_path: String,
    local_path: String,
    content_type: Option<String>,
) -> Result<bool, String> {
    let core = state.inner().clone();
    let cancel = transfers.begin(&transfer_id);
    let _guard = CancelGuard {
        transfers: transfers.inner(),
        id: transfer_id.clone(),
    };

    // 秒传:远端已存在且内容与本地一致(ETag = 本地 MD5)则跳过上传,返回 skipped=true。
    if core
        .is_unchanged(&account, &remote_path, &local_path)
        .await
        .unwrap_or(false)
    {
        return Ok(true);
    }

    let event_path = remote_path.clone();
    let progress = move |uploaded: u64, total: u64| {
        let _ = app.emit(
            "upload-progress",
            UploadProgress {
                path: event_path.clone(),
                uploaded,
                total,
            },
        );
    };

    // 大文件走可断点续传的分片上传;小文件回退整体上传。逐块从磁盘读,内存受控。
    core.upload_resumable(
        &account,
        &remote_path,
        &local_path,
        content_type.as_deref(),
        &cancel,
        &progress,
    )
    .await
    .map_err(|e| match e {
        app_core::AppError::Cancelled => "已取消".to_string(),
        other => other.to_string(),
    })?;
    Ok(false)
}

/// 流式下载远端对象到本地路径,边写边发 `download-progress` 事件。
///
/// **断点续传**:先下到 `{local_path}.part`;若该临时文件已存在,则从其大小处用 HTTP Range
/// 续传(服务端拒绝续传时清掉重下)。全部写完再把 `.part` 改名为最终文件。中断后重新下载即续传。
#[tauri::command]
async fn download_file(
    app: AppHandle,
    state: State<'_, App>,
    transfers: State<'_, Transfers>,
    transfer_id: String,
    account: String,
    remote_path: String,
    local_path: String,
) -> Result<bool, String> {
    use futures::StreamExt;
    use tokio::io::AsyncWriteExt;

    let core = state.inner().clone();
    let limits = core.transfer_limits();
    let cancel = transfers.begin(&transfer_id);
    let _guard = CancelGuard {
        transfers: transfers.inner(),
        id: transfer_id.clone(),
    };

    // 下载秒传:本地目标已存在且与远端内容一致(ETag = 本地 MD5)则跳过下载。
    if core
        .is_unchanged(&account, &remote_path, &local_path)
        .await
        .unwrap_or(false)
    {
        return Ok(true);
    }

    let part_path = format!("{local_path}.part");

    // 已有 .part → 从其大小续传;offset>0 且服务端拒绝(如 416 过期/越界)则清掉从头下。
    let mut offset = tokio::fs::metadata(&part_path)
        .await
        .map(|m| m.len())
        .unwrap_or(0);
    let (total, mut stream) = match core.download_range(&account, &remote_path, offset).await {
        Ok(v) => v,
        Err(e) if offset > 0 => {
            let _ = tokio::fs::remove_file(&part_path).await;
            offset = 0;
            let _ = e;
            core.download_range(&account, &remote_path, 0)
                .await
                .map_err(|e| e.to_string())?
        }
        Err(e) => return Err(e.to_string()),
    };
    let total = total.unwrap_or(0);

    let mut file = if offset > 0 {
        tokio::fs::OpenOptions::new()
            .append(true)
            .open(&part_path)
            .await
            .map_err(|e| format!("打开续传文件失败: {e}"))?
    } else {
        tokio::fs::File::create(&part_path)
            .await
            .map_err(|e| format!("创建本地文件失败: {e}"))?
    };

    let mut downloaded = offset;
    while let Some(chunk) = stream.next().await {
        // 取消:保留 .part,重下即续传。
        if cancel.load(Ordering::Relaxed) {
            return Err("已取消".into());
        }
        let chunk = chunk.map_err(|e| e.to_string())?;
        limits.throttle(chunk.len() as u64).await; // 全局带宽限速
        file.write_all(&chunk)
            .await
            .map_err(|e| format!("写入本地文件失败: {e}"))?;
        downloaded += chunk.len() as u64;
        let _ = app.emit(
            "download-progress",
            DownloadProgress {
                path: remote_path.clone(),
                downloaded,
                total,
            },
        );
    }
    file.flush()
        .await
        .map_err(|e| format!("写入本地文件失败: {e}"))?;
    // 完成:.part → 最终文件名。
    tokio::fs::rename(&part_path, &local_path)
        .await
        .map_err(|e| format!("重命名文件失败: {e}"))?;
    Ok(false)
}

/// 删除远端对象。
#[tauri::command]
async fn delete(state: State<'_, App>, account: String, path: String) -> Result<(), String> {
    let app = state.inner().clone();
    app.delete(&account, &path).await.map_err(|e| e.to_string())
}

/// 新建"文件夹":写一个以 `/` 结尾的零字节对象作为目录占位。
#[tauri::command]
async fn create_folder(state: State<'_, App>, account: String, path: String) -> Result<(), String> {
    let app = state.inner().clone();
    app.upload(&account, &path, Bytes::new(), None)
        .await
        .map_err(|e| e.to_string())
}

/// 修改对象的内容类型(Content-Type)。
#[tauri::command]
async fn set_content_type(
    state: State<'_, App>,
    account: String,
    path: String,
    content_type: String,
) -> Result<(), String> {
    let app = state.inner().clone();
    app.set_content_type(&account, &path, &content_type)
        .await
        .map_err(|e| e.to_string())
}

/// 列举某桶下未完成(残留)的分片上传。
#[tauri::command]
async fn incomplete_uploads(
    state: State<'_, App>,
    account: String,
    bucket: String,
) -> Result<Vec<IncompleteUpload>, String> {
    let app = state.inner().clone();
    app.incomplete_uploads(&account, &bucket)
        .await
        .map_err(|e| e.to_string())
}

/// 清理某桶下所有未完成的分片上传,返回清理数量。
#[tauri::command]
async fn clean_incomplete_uploads(
    state: State<'_, App>,
    account: String,
    bucket: String,
) -> Result<u64, String> {
    let app = state.inner().clone();
    app.clean_incomplete_uploads(&account, &bucket)
        .await
        .map_err(|e| e.to_string())
}

/// 设置对象为公开读 / 私有。
#[tauri::command]
async fn set_object_acl(
    state: State<'_, App>,
    account: String,
    path: String,
    public: bool,
) -> Result<(), String> {
    let app = state.inner().clone();
    app.set_object_acl(&account, &path, public)
        .await
        .map_err(|e| e.to_string())
}

/// 对象的永久公共直链(不签名);未支持返回 null。
#[tauri::command]
async fn public_url(
    state: State<'_, App>,
    account: String,
    path: String,
) -> Result<Option<String>, String> {
    let app = state.inner().clone();
    app.public_url(&account, &path).map_err(|e| e.to_string())
}

/// 读取对象标签(键值对)。
#[tauri::command]
async fn object_tags(
    state: State<'_, App>,
    account: String,
    path: String,
) -> Result<Vec<(String, String)>, String> {
    let app = state.inner().clone();
    app.object_tags(&account, &path)
        .await
        .map_err(|e| e.to_string())
}

/// 覆盖对象标签(整套替换;空列表即清空)。
#[tauri::command]
async fn set_object_tags(
    state: State<'_, App>,
    account: String,
    path: String,
    tags: Vec<(String, String)>,
) -> Result<(), String> {
    let app = state.inner().clone();
    app.set_object_tags(&account, &path, &tags)
        .await
        .map_err(|e| e.to_string())
}

/// 读取对象前若干字节并解码为文本,用于预览。
#[tauri::command]
async fn read_preview(
    state: State<'_, App>,
    account: String,
    path: String,
    max_bytes: usize,
) -> Result<TextPreview, String> {
    let app = state.inner().clone();
    app.read_preview(&account, &path, max_bytes)
        .await
        .map_err(|e| e.to_string())
}

/// 统计文件夹 / Bucket 下的文件数与总字节数。
#[tauri::command]
async fn folder_stats(
    state: State<'_, App>,
    account: String,
    path: String,
) -> Result<FolderStats, String> {
    let app = state.inner().clone();
    app.folder_stats(&account, &path)
        .await
        .map_err(|e| e.to_string())
}

/// 统计文件夹 / Bucket 下对象按存储类型的分布。
#[tauri::command]
async fn storage_breakdown(
    state: State<'_, App>,
    account: String,
    path: String,
) -> Result<StorageBreakdown, String> {
    let app = state.inner().clone();
    app.storage_breakdown(&account, &path)
        .await
        .map_err(|e| e.to_string())
}

/// 新建一个 bucket。
#[tauri::command]
async fn create_bucket(
    state: State<'_, App>,
    account: String,
    bucket: String,
) -> Result<(), String> {
    let app = state.inner().clone();
    app.create_bucket(&account, &bucket)
        .await
        .map_err(|e| e.to_string())
}

/// 删除一个 bucket(通常要求为空)。
#[tauri::command]
async fn delete_bucket(
    state: State<'_, App>,
    account: String,
    bucket: String,
) -> Result<(), String> {
    let app = state.inner().clone();
    app.delete_bucket(&account, &bucket)
        .await
        .map_err(|e| e.to_string())
}

/// 重命名 / 移动对象。
#[tauri::command]
async fn rename(
    state: State<'_, App>,
    account: String,
    from: String,
    to: String,
) -> Result<(), String> {
    let app = state.inner().clone();
    app.rename(&account, &from, &to)
        .await
        .map_err(|e| e.to_string())
}

/// 复制对象到新路径(保留原对象)。
#[tauri::command]
async fn copy(
    state: State<'_, App>,
    account: String,
    from: String,
    to: String,
) -> Result<(), String> {
    let app = state.inner().clone();
    app.copy(&account, &from, &to)
        .await
        .map_err(|e| e.to_string())
}

/// 转换对象存储类型 / 归档层。`class` 为厂商的存储类型字符串。
#[tauri::command]
async fn set_storage_class(
    state: State<'_, App>,
    account: String,
    path: String,
    class: String,
) -> Result<(), String> {
    let app = state.inner().clone();
    app.set_storage_class(&account, &path, &class)
        .await
        .map_err(|e| e.to_string())
}

/// 取回(解冻)归档对象,`days` 为保持天数。
#[tauri::command]
async fn restore_object(
    state: State<'_, App>,
    account: String,
    path: String,
    days: u32,
) -> Result<(), String> {
    let app = state.inner().clone();
    app.restore(&account, &path, days)
        .await
        .map_err(|e| e.to_string())
}

/// 跨账号 / 跨云迁移复制:把 `src_account` 的 `src_path` 搬到 `dst_account` 的 `dst_path`,
/// 保留源对象。跨账号时走"下载源 → 上传目标",过程中发 `transfer-progress` 事件。
#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn copy_across(
    app: AppHandle,
    state: State<'_, App>,
    transfers: State<'_, Transfers>,
    transfer_id: String,
    src_account: String,
    src_path: String,
    dst_account: String,
    dst_path: String,
) -> Result<(), String> {
    let core = state.inner().clone();
    let cancel = transfers.begin(&transfer_id);
    let _guard = CancelGuard {
        transfers: transfers.inner(),
        id: transfer_id.clone(),
    };

    let event_to = dst_path.clone();
    let progress = move |transferred: u64, total: u64| {
        let _ = app.emit(
            "transfer-progress",
            TransferProgress {
                to: event_to.clone(),
                transferred,
                total,
            },
        );
    };

    core.copy_across_with_progress(
        &src_account,
        &src_path,
        &dst_account,
        &dst_path,
        cancel,
        &progress,
    )
    .await
    .map_err(|e| match e {
        app_core::AppError::Cancelled => "已取消".to_string(),
        other => other.to_string(),
    })
}

/// 递归下载整个远端文件夹到本地目录,保留相对结构(落到 `{local_dir}/{文件夹名}/…`)。
/// 逐个文件流式写入,每完成一个发一次 `folder-progress`(按文件数计)。
#[tauri::command]
async fn download_folder(
    app: AppHandle,
    state: State<'_, App>,
    transfers: State<'_, Transfers>,
    transfer_id: String,
    account: String,
    remote_root: String,
    local_dir: String,
) -> Result<u64, String> {
    use futures::StreamExt;
    use tokio::io::AsyncWriteExt;

    let core = state.inner().clone();
    let limits = core.transfer_limits();
    let cancel = transfers.begin(&transfer_id);
    let _guard = CancelGuard {
        transfers: transfers.inner(),
        id: transfer_id.clone(),
    };
    let files = core
        .list_all_files(&account, &remote_root)
        .await
        .map_err(|e| e.to_string())?;
    let total = files.len() as u64;
    let mut skipped = 0u64;

    let base = if remote_root.ends_with('/') {
        remote_root.clone()
    } else {
        format!("{remote_root}/")
    };
    let folder = remote_root
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or("download");
    let root_dir = std::path::Path::new(&local_dir).join(folder);

    for (i, file) in files.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Err("已取消".into());
        }
        let rel = file.path.strip_prefix(&base).unwrap_or(&file.path);
        // 按 `/` 分段拼本地路径,跨平台安全。
        let mut local = root_dir.clone();
        for seg in rel.split('/') {
            local.push(seg);
        }
        // 下载秒传:本地已存在且与远端一致则跳过该文件。
        if core
            .is_unchanged(&account, &file.path, &local.to_string_lossy())
            .await
            .unwrap_or(false)
        {
            skipped += 1;
            let _ = app.emit(
                "folder-progress",
                FolderProgress {
                    op: "download".into(),
                    path: remote_root.clone(),
                    done: (i + 1) as u64,
                    total,
                },
            );
            continue;
        }
        if let Some(parent) = local.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| format!("创建目录失败: {e}"))?;
        }
        let (_len, mut stream) = core
            .download_stream(&account, &file.path)
            .await
            .map_err(|e| e.to_string())?;
        let mut f = tokio::fs::File::create(&local)
            .await
            .map_err(|e| format!("创建本地文件失败: {e}"))?;
        while let Some(chunk) = stream.next().await {
            if cancel.load(Ordering::Relaxed) {
                return Err("已取消".into());
            }
            let chunk = chunk.map_err(|e| e.to_string())?;
            limits.throttle(chunk.len() as u64).await; // 全局带宽限速
            f.write_all(&chunk)
                .await
                .map_err(|e| format!("写入本地文件失败: {e}"))?;
        }
        f.flush()
            .await
            .map_err(|e| format!("写入本地文件失败: {e}"))?;
        let _ = app.emit(
            "folder-progress",
            FolderProgress {
                op: "download".into(),
                path: remote_root.clone(),
                done: (i + 1) as u64,
                total,
            },
        );
    }
    Ok(skipped)
}

/// 把整个远端文件夹迁移到另一账号的目标目录下(作为子目录),保留相对结构;源保留。
/// 每完成一个文件发一次 `folder-progress`。
#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn migrate_folder(
    app: AppHandle,
    state: State<'_, App>,
    transfers: State<'_, Transfers>,
    transfer_id: String,
    src_account: String,
    src_root: String,
    dst_account: String,
    dst_dir: String,
) -> Result<(), String> {
    let core = state.inner().clone();
    let cancel = transfers.begin(&transfer_id);
    let _guard = CancelGuard {
        transfers: transfers.inner(),
        id: transfer_id.clone(),
    };
    let event_path = src_root.clone();
    let progress = move |done: u64, total: u64| {
        let _ = app.emit(
            "folder-progress",
            FolderProgress {
                op: "migrate".into(),
                path: event_path.clone(),
                done,
                total,
            },
        );
    };
    core.migrate_folder(
        &src_account,
        &src_root,
        &dst_account,
        &dst_dir,
        cancel,
        &progress,
    )
    .await
    .map_err(|e| match e {
        app_core::AppError::Cancelled => "已取消".to_string(),
        other => other.to_string(),
    })
}

/// 把整个远端文件夹移动 / 重命名为新的完整路径(同账号,服务端复制后删除原对象)。
/// 每完成一个文件发一次 `folder-progress`。
#[tauri::command]
async fn move_folder(
    app: AppHandle,
    state: State<'_, App>,
    transfers: State<'_, Transfers>,
    transfer_id: String,
    account: String,
    src_root: String,
    dst_root: String,
) -> Result<(), String> {
    let core = state.inner().clone();
    let cancel = transfers.begin(&transfer_id);
    let _guard = CancelGuard {
        transfers: transfers.inner(),
        id: transfer_id.clone(),
    };
    let event_path = src_root.clone();
    let progress = move |done: u64, total: u64| {
        let _ = app.emit(
            "folder-progress",
            FolderProgress {
                op: "move".into(),
                path: event_path.clone(),
                done,
                total,
            },
        );
    };
    core.move_folder(&account, &src_root, &dst_root, cancel, &progress)
        .await
        .map_err(|e| match e {
            app_core::AppError::Cancelled => "已取消".to_string(),
            other => other.to_string(),
        })
}

/// 把整个远端文件夹复制到新的完整路径(同账号,保留源)。每完成一个文件发一次 `folder-progress`。
#[tauri::command]
async fn copy_folder(
    app: AppHandle,
    state: State<'_, App>,
    transfers: State<'_, Transfers>,
    transfer_id: String,
    account: String,
    src_root: String,
    dst_root: String,
) -> Result<(), String> {
    let core = state.inner().clone();
    let cancel = transfers.begin(&transfer_id);
    let _guard = CancelGuard {
        transfers: transfers.inner(),
        id: transfer_id.clone(),
    };
    let event_path = src_root.clone();
    let progress = move |done: u64, total: u64| {
        let _ = app.emit(
            "folder-progress",
            FolderProgress {
                op: "copy".into(),
                path: event_path.clone(),
                done,
                total,
            },
        );
    };
    core.copy_folder(&account, &src_root, &dst_root, cancel, &progress)
        .await
        .map_err(|e| match e {
            app_core::AppError::Cancelled => "已取消".to_string(),
            other => other.to_string(),
        })
}

/// 递归删除整个远端文件夹(文件 + 目录占位)。每删一个发一次 `folder-progress`。
#[tauri::command]
async fn delete_folder(
    app: AppHandle,
    state: State<'_, App>,
    account: String,
    path: String,
) -> Result<(), String> {
    let core = state.inner().clone();
    let event_path = path.clone();
    let progress = move |done: u64, total: u64| {
        let _ = app.emit(
            "folder-progress",
            FolderProgress {
                op: "delete".into(),
                path: event_path.clone(),
                done,
                total,
            },
        );
    };
    core.delete_folder(&account, &path, &progress)
        .await
        .map_err(|e| e.to_string())
}

/// 递归把整个远端文件夹转换存储类型。每处理一个发一次 `folder-progress`。
#[tauri::command]
async fn set_storage_class_folder(
    app: AppHandle,
    state: State<'_, App>,
    account: String,
    path: String,
    class: String,
) -> Result<(), String> {
    let core = state.inner().clone();
    let event_path = path.clone();
    let progress = move |done: u64, total: u64| {
        let _ = app.emit(
            "folder-progress",
            FolderProgress {
                op: "storage-class".into(),
                path: event_path.clone(),
                done,
                total,
            },
        );
    };
    core.set_storage_class_folder(&account, &path, &class, &progress)
        .await
        .map_err(|e| e.to_string())
}

/// 递归取回整个远端文件夹里的归档对象。每处理一个发一次 `folder-progress`。
#[tauri::command]
async fn restore_folder(
    app: AppHandle,
    state: State<'_, App>,
    account: String,
    path: String,
    days: u32,
) -> Result<(), String> {
    let core = state.inner().clone();
    let event_path = path.clone();
    let progress = move |done: u64, total: u64| {
        let _ = app.emit(
            "folder-progress",
            FolderProgress {
                op: "restore".into(),
                path: event_path.clone(),
                done,
                total,
            },
        );
    };
    core.restore_folder(&account, &path, days, &progress)
        .await
        .map_err(|e| e.to_string())
}

/// 生成对象的预签名下载链接。
#[tauri::command]
async fn presign(
    state: State<'_, App>,
    account: String,
    path: String,
    expires_secs: u64,
) -> Result<String, String> {
    let app = state.inner().clone();
    app.presign(&account, &path, expires_secs)
        .await
        .map_err(|e| e.to_string())
}

/// 生成对象的预签名上传链接(PUT)。
#[tauri::command]
async fn presign_put(
    state: State<'_, App>,
    account: String,
    path: String,
    expires_secs: u64,
) -> Result<String, String> {
    let app = state.inner().clone();
    app.presign_put(&account, &path, expires_secs)
        .await
        .map_err(|e| e.to_string())
}

/// 一个待上传的本地文件:本地绝对路径 + 相对(远端)路径。
#[derive(Clone, Serialize)]
struct UploadEntry {
    local: String,
    rel: String,
}

/// 把拖入的本地路径(文件或文件夹)展开成文件列表,文件夹递归并保留相对路径。
#[tauri::command]
fn expand_upload_paths(paths: Vec<String>) -> Result<Vec<UploadEntry>, String> {
    let mut out = Vec::new();
    for p in paths {
        let path = std::path::Path::new(&p);
        let base = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if path.is_dir() {
            collect_dir(path, &base, &mut out).map_err(|e| e.to_string())?;
        } else if path.is_file() {
            out.push(UploadEntry {
                local: p,
                rel: base,
            });
        }
    }
    Ok(out)
}

/// 递归收集目录下的文件,`rel_prefix` 为其相对根路径。
fn collect_dir(
    dir: &std::path::Path,
    rel_prefix: &str,
    out: &mut Vec<UploadEntry>,
) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let rel = format!("{rel_prefix}/{name}");
        let child = entry.path();
        if file_type.is_dir() {
            collect_dir(&child, &rel, out)?;
        } else if file_type.is_file() {
            out.push(UploadEntry {
                local: child.to_string_lossy().into_owned(),
                rel,
            });
        }
    }
    Ok(())
}

/// 批量生成预签名链接(网格缩略图用)。
#[tauri::command]
async fn presign_batch(
    state: State<'_, App>,
    account: String,
    paths: Vec<String>,
    expires_secs: u64,
) -> Result<Vec<String>, String> {
    let app = state.inner().clone();
    app.presign_batch(&account, &paths, expires_secs)
        .await
        .map_err(|e| e.to_string())
}

/// 读取应用设置。
#[tauri::command]
fn get_settings(state: State<'_, App>) -> Settings {
    state.settings()
}

/// 保存应用设置。
#[tauri::command]
fn save_settings(state: State<'_, App>, settings: Settings) -> Result<(), String> {
    state.save_settings(&settings).map_err(|e| e.to_string())
}

/// 列出所有收藏(最近的在前)。
#[tauri::command]
fn bookmarks(state: State<'_, App>) -> Vec<Bookmark> {
    state.bookmarks()
}

/// 收藏一个位置(账号 + 路径)。
#[tauri::command]
fn add_bookmark(state: State<'_, App>, account: String, path: String) -> Result<(), String> {
    state
        .add_bookmark(&account, &path)
        .map_err(|e| e.to_string())
}

/// 取消收藏一个位置。
#[tauri::command]
fn remove_bookmark(state: State<'_, App>, account: String, path: String) -> Result<(), String> {
    state
        .remove_bookmark(&account, &path)
        .map_err(|e| e.to_string())
}

/// 记录一次访问(账号 + 路径),供「最近访问」用。
#[tauri::command]
fn record_visit(state: State<'_, App>, account: String, path: String) -> Result<(), String> {
    state
        .record_visit(&account, &path)
        .map_err(|e| e.to_string())
}

/// 列出最近访问(最新在前)。
#[tauri::command]
fn recent_locations(state: State<'_, App>) -> Vec<Bookmark> {
    state.recent_locations()
}

/// 为一批对象计算批量重命名计划(纯函数,前端用于预览与执行)。
#[tauri::command]
fn plan_batch_rename(paths: Vec<String>, rule: RenameRule) -> Vec<RenamePlan> {
    app_core::plan_batch_rename(&paths, &rule)
}

/// 渲染一张浏览用图:Rust 侧解码 + 缩到 `max_edge` + 缓存,返回内联 data URL。
#[tauri::command]
async fn image_view(
    state: State<'_, App>,
    account: String,
    path: String,
    etag: Option<String>,
    max_edge: u32,
) -> Result<ImageData, String> {
    let app = state.inner().clone();
    app.image_view(&account, &path, etag, max_edge)
        .await
        .map_err(|e| e.to_string())
}

/// 渲染一张方形缩略图(Rust 侧生成 + 缓存)。
#[tauri::command]
async fn image_thumb(
    state: State<'_, App>,
    account: String,
    path: String,
    etag: Option<String>,
    size: u32,
) -> Result<ImageData, String> {
    let app = state.inner().clone();
    app.image_thumb(&account, &path, etag, size)
        .await
        .map_err(|e| e.to_string())
}

/// 解析对象的 EXIF 摘要(相机 / 拍摄参数 / GPS)。
#[tauri::command]
async fn image_exif(
    state: State<'_, App>,
    account: String,
    path: String,
    etag: Option<String>,
) -> Result<ExifInfo, String> {
    let app = state.inner().clone();
    app.image_exif(&account, &path, etag)
        .await
        .map_err(|e| e.to_string())
}

/// 编辑预览:在缓存原图上应用操作并缩到 `max_edge`,返回内联 data URL。
#[tauri::command]
async fn image_edit_preview(
    state: State<'_, App>,
    account: String,
    path: String,
    etag: Option<String>,
    ops: Ops,
    max_edge: u32,
) -> Result<ImageData, String> {
    let app = state.inner().clone();
    app.image_edit_preview(&account, &path, etag, ops, max_edge)
        .await
        .map_err(|e| e.to_string())
}

/// 保存编辑结果回云端(`dest == path` 覆盖,否则另存为新对象)。
#[tauri::command]
async fn image_edit_save(
    state: State<'_, App>,
    account: String,
    path: String,
    etag: Option<String>,
    ops: Ops,
    save: EditSave,
) -> Result<(), String> {
    let app = state.inner().clone();
    app.image_edit_save(&account, &path, etag, ops, save)
        .await
        .map_err(|e| e.to_string())
}

/// 全分辨率渲染编辑结果(不缩放),返回 data URL,供前端叠加文字后合成。
#[tauri::command]
async fn image_edit_full(
    state: State<'_, App>,
    account: String,
    path: String,
    etag: Option<String>,
    ops: Ops,
) -> Result<ImageData, String> {
    let app = state.inner().clone();
    app.image_edit_full(&account, &path, etag, ops)
        .await
        .map_err(|e| e.to_string())
}

/// 把前端合成好的字节写回云端 `dest`。
#[tauri::command]
async fn put_image_bytes(
    state: State<'_, App>,
    account: String,
    dest: String,
    bytes: Vec<u8>,
    content_type: String,
) -> Result<(), String> {
    let app = state.inner().clone();
    app.put_bytes(&account, &dest, bytes, &content_type)
        .await
        .map_err(|e| e.to_string())
}

/// 把前端合成好的字节写到本地文件 `dest`。
#[tauri::command]
async fn save_image_bytes_local(dest: String, bytes: Vec<u8>) -> Result<(), String> {
    tokio::fs::write(&dest, bytes)
        .await
        .map_err(|e| e.to_string())
}

/// 把编辑结果编码后保存到本地文件(`save.dest` 为本地路径,不回云端)。
#[tauri::command]
async fn image_edit_download(
    state: State<'_, App>,
    account: String,
    path: String,
    etag: Option<String>,
    ops: Ops,
    save: EditSave,
) -> Result<(), String> {
    let app = state.inner().clone();
    let (bytes, _mime) = app
        .image_edit_bytes(&account, &path, etag, ops, save.format, save.quality)
        .await
        .map_err(|e| e.to_string())?;
    tokio::fs::write(&save.dest, bytes)
        .await
        .map_err(|e| e.to_string())
}

/// 读取一个界面偏好(主题 / 视图 / 语言 / 侧栏宽度)。
#[tauri::command]
fn get_pref(state: State<'_, App>, key: String) -> Option<String> {
    state.get_pref(&key)
}

/// 写入一个界面偏好。
#[tauri::command]
fn set_pref(state: State<'_, App>, key: String, value: String) -> Result<(), String> {
    state.set_pref(&key, &value).map_err(|e| e.to_string())
}

/// 构建并设置应用的自定义原生菜单,替换 Tauri 默认菜单。
///
/// 自定义项(设置 / 添加账号 / 刷新 / 切换主题)点击后经 `menu-action` 事件发往前端;
/// 编辑子菜单保留系统撤销/复制/粘贴等,确保输入框的键盘快捷键仍可用。
fn setup_menu(app: &AppHandle) -> tauri::Result<()> {
    let settings = MenuItemBuilder::with_id("settings", "设置…")
        .accelerator("CmdOrCtrl+,")
        .build(app)?;
    let add_account = MenuItemBuilder::with_id("add-account", "添加账号")
        .accelerator("CmdOrCtrl+N")
        .build(app)?;
    let refresh = MenuItemBuilder::with_id("refresh", "刷新")
        .accelerator("CmdOrCtrl+R")
        .build(app)?;
    let toggle_theme = MenuItemBuilder::with_id("toggle-theme", "切换主题")
        .accelerator("CmdOrCtrl+Shift+L")
        .build(app)?;
    let check_update = MenuItemBuilder::with_id("check-update", "检查更新…").build(app)?;

    // 自定义"关于"项(点击弹出 App 自绘的关于弹窗,替代系统默认关于面板)。
    let about = MenuItemBuilder::with_id("about", "关于 Nebula").build(app)?;

    let app_menu = SubmenuBuilder::new(app, "Nebula")
        .item(&about)
        .item(&check_update)
        .separator()
        .item(&settings)
        .separator()
        .hide()
        .hide_others()
        .show_all()
        .separator()
        .quit()
        .build()?;
    let account_menu = SubmenuBuilder::new(app, "账号")
        .item(&add_account)
        .build()?;
    let edit_menu = SubmenuBuilder::new(app, "编辑")
        .undo()
        .redo()
        .separator()
        .cut()
        .copy()
        .paste()
        .select_all()
        .build()?;
    let view_menu = SubmenuBuilder::new(app, "视图")
        .item(&refresh)
        .item(&toggle_theme)
        .build()?;

    let menu = MenuBuilder::new(app)
        .items(&[&app_menu, &account_menu, &edit_menu, &view_menu])
        .build()?;
    app.set_menu(menu)?;
    Ok(())
}

/// 启动 Tauri 应用。账号存到应用数据目录下的 `nebula.db`。
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            setup_menu(app.handle())?;
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let core = App::with_store(dir.join("nebula.db"))?;
            // 图片渲染缓存放缓存目录(可被系统清理,不占用户数据目录)。
            if let Ok(cache) = app.path().app_cache_dir() {
                core.set_cache_dir(cache.join("nebula"));
            }
            app.manage(core);
            app.manage(Transfers::default());
            Ok(())
        })
        .on_menu_event(|app, event| {
            // 只转发自定义项;系统预定义项(退出/复制等)自行处理。
            if let id @ ("about" | "check-update" | "settings" | "add-account" | "refresh"
            | "toggle-theme") = event.id().as_ref()
            {
                let _ = app.emit("menu-action", id);
            }
        })
        .invoke_handler(tauri::generate_handler![
            list_accounts,
            list_account_infos,
            set_account_domain,
            add_aliyun_account,
            add_huawei_account,
            add_qiniu_account,
            add_aws_account,
            add_r2_account,
            add_minio_account,
            add_tencent_account,
            remove_account,
            get_account,
            browse,
            browse_page,
            stat,
            verify_object,
            cancel_transfer,
            list_transfers,
            save_transfer,
            delete_transfer,
            search,
            upload_file,
            download_file,
            delete,
            create_folder,
            create_bucket,
            delete_bucket,
            set_content_type,
            object_tags,
            set_object_tags,
            set_object_acl,
            public_url,
            incomplete_uploads,
            clean_incomplete_uploads,
            read_preview,
            folder_stats,
            storage_breakdown,
            rename,
            copy,
            set_storage_class,
            restore_object,
            copy_across,
            download_folder,
            migrate_folder,
            move_folder,
            copy_folder,
            delete_folder,
            set_storage_class_folder,
            restore_folder,
            presign,
            presign_put,
            presign_batch,
            expand_upload_paths,
            get_settings,
            save_settings,
            bookmarks,
            add_bookmark,
            remove_bookmark,
            record_visit,
            recent_locations,
            plan_batch_rename,
            image_view,
            image_thumb,
            image_exif,
            image_edit_preview,
            image_edit_save,
            image_edit_download,
            image_edit_full,
            put_image_bytes,
            save_image_bytes_local,
            get_pref,
            set_pref,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
