//! Nebula 桌面应用后端:把 [`app_core::App`] 的方法包成 Tauri command。
//!
//! 业务逻辑都在 `app-core`(可 `cargo test`),这里只做 JS ↔ Rust 的桥接与本地文件读写。

use app_core::{App, Settings};
use bytes::Bytes;
use nebula_provider::Entry;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

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

/// 列出已注册账号。
#[tauri::command]
fn list_accounts(state: State<'_, App>) -> Vec<String> {
    state.accounts()
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

/// 移除账号(从注册表与 SQLite)。
#[tauri::command]
fn remove_account(state: State<'_, App>, id: String) -> Result<bool, String> {
    state.remove_account(&id).map_err(|e| e.to_string())
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

/// 读取对象元信息。
#[tauri::command]
async fn stat(state: State<'_, App>, account: String, path: String) -> Result<Entry, String> {
    let app = state.inner().clone();
    app.stat(&account, &path).await.map_err(|e| e.to_string())
}

/// 把本地文件上传到远端路径。
#[tauri::command]
async fn upload_file(
    app: AppHandle,
    state: State<'_, App>,
    account: String,
    remote_path: String,
    local_path: String,
    content_type: Option<String>,
) -> Result<(), String> {
    let core = state.inner().clone();
    let data = tokio::fs::read(&local_path)
        .await
        .map_err(|e| format!("读取本地文件失败: {e}"))?;

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

    core.upload_with_progress(
        &account,
        &remote_path,
        Bytes::from(data),
        content_type.as_deref(),
        &progress,
    )
    .await
    .map_err(|e| e.to_string())
}

/// 流式下载远端对象到本地路径,边写边发 `download-progress` 事件。
#[tauri::command]
async fn download_file(
    app: AppHandle,
    state: State<'_, App>,
    account: String,
    remote_path: String,
    local_path: String,
) -> Result<(), String> {
    use futures::StreamExt;
    use tokio::io::AsyncWriteExt;

    let core = state.inner().clone();
    let (total, mut stream) = core
        .download_stream(&account, &remote_path)
        .await
        .map_err(|e| e.to_string())?;
    let total = total.unwrap_or(0);

    let mut file = tokio::fs::File::create(&local_path)
        .await
        .map_err(|e| format!("创建本地文件失败: {e}"))?;
    let mut downloaded = 0u64;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| e.to_string())?;
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
        .map_err(|e| format!("写入本地文件失败: {e}"))
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

/// 启动 Tauri 应用。账号存到应用数据目录下的 `nebula.db`。
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let core = App::with_store(dir.join("nebula.db"))?;
            app.manage(core);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_accounts,
            add_aliyun_account,
            remove_account,
            browse,
            stat,
            upload_file,
            download_file,
            delete,
            create_folder,
            rename,
            copy,
            presign,
            presign_batch,
            expand_upload_paths,
            get_settings,
            save_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
