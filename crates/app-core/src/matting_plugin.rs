//! AI 抠图插件的下载 / 安装 / 状态。
//!
//! 「插件式」:默认不含 ONNX Runtime;用户在设置里启用后,下载模型 + 平台对应的
//! 运行时库到 `plugin_dir/matting/`。运行时库只在实际调用 [`crate::App::remove_background`]
//! 时懒加载,绝不在应用启动时加载——不兼容的动态库可能直接让进程崩溃。
//!
//! ONNX Runtime 版本(1.22.0)已与 `ort-sys` 2.0.0-rc.10 的 `ONNXRUNTIME_VERSION`(API 22)对齐;
//! 三平台发布包 URL 均已核对可下载。升级 `ort` 时同步改 [`ORT_VERSION`]。

use std::io::Read;
use std::path::{Path, PathBuf};

use futures::StreamExt;
use tokio::io::AsyncWriteExt;

use crate::{App, AppError, Result};

/// u2netp 显著性模型(~4.6 MB)。
const MODEL_URL: &str = "https://github.com/danielgatis/rembg/releases/download/v0.0.0/u2netp.onnx";
/// 与 `ort` 2.0.0-rc.10 匹配的 ONNX Runtime 版本。
/// 必须与 `ort-sys` 的 `ONNXRUNTIME_VERSION`(API 版本 22)一致,否则 Session 创建会卡死 / 失败。
const ORT_VERSION: &str = "1.22.0";

/// 当前平台的运行时库文件名(下载解压后统一命名)。
fn dylib_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "onnxruntime.dll"
    } else if cfg!(target_os = "macos") {
        "libonnxruntime.dylib"
    } else {
        "libonnxruntime.so"
    }
}

/// 当前平台的 ONNX Runtime 官方发布包 URL + 解压时匹配库文件的关键片段。
fn runtime_archive() -> (String, bool) {
    // 返回 (url, is_zip)。
    let v = ORT_VERSION;
    if cfg!(target_os = "windows") {
        (format!("https://github.com/microsoft/onnxruntime/releases/download/v{v}/onnxruntime-win-x64-{v}.zip"), true)
    } else if cfg!(target_os = "macos") {
        (format!("https://github.com/microsoft/onnxruntime/releases/download/v{v}/onnxruntime-osx-universal2-{v}.tgz"), false)
    } else {
        (format!("https://github.com/microsoft/onnxruntime/releases/download/v{v}/onnxruntime-linux-x64-{v}.tgz"), false)
    }
}

/// 解压后判断某个成员是不是我们要的运行时库(`lib/` 下、名字含 onnxruntime 的动态库)。
fn is_runtime_lib(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.contains("onnxruntime")
        && (n.ends_with(".dll") || n.contains(".dylib") || n.contains(".so"))
        && !n.contains("providers") // 排除 provider 插件库
}

impl App {
    /// 设置插件目录(Tauri 层用 `app_data_dir()/plugins` 调一次)。
    pub fn set_plugin_dir(&self, dir: impl Into<PathBuf>) {
        let _ = self.plugin_dir.set(dir.into());
    }

    fn matting_dir(&self) -> Option<PathBuf> {
        self.plugin_dir.get().map(|d| d.join("matting"))
    }

    /// 抠图模型文件路径。
    pub fn matting_model_path(&self) -> Option<PathBuf> {
        self.matting_dir().map(|d| d.join("u2netp.onnx"))
    }

    /// ONNX Runtime 库文件路径。
    pub fn matting_dylib_path(&self) -> Option<PathBuf> {
        self.matting_dir().map(|d| d.join(dylib_name()))
    }

    /// 插件是否已安装(模型 + 运行时库都在)。
    pub fn matting_installed(&self) -> bool {
        matches!(
            (self.matting_model_path(), self.matting_dylib_path()),
            (Some(m), Some(d)) if m.exists() && d.exists()
        )
    }

    /// 懒加载 ONNX Runtime(仅在实际用到「去背景」时调用)。失败返回错误,不 panic。
    /// 注意:加载不兼容的动态库理论上可能崩溃,故绝不在应用启动时调用。
    fn ensure_matting_init(&self) -> Result<()> {
        let dylib = self
            .matting_dylib_path()
            .filter(|p| p.exists())
            .ok_or_else(|| AppError::InvalidInput("matting plugin not installed".into()))?;
        nebula_matting::init(&dylib).map_err(|e| AppError::Image(e.to_string()))
    }

    /// 安装插件:下载模型 + 运行时库(解压提取库),`progress(done, total)` 报总进度。
    pub async fn install_matting<F: Fn(u64, u64)>(&self, progress: F) -> Result<()> {
        let dir = self
            .matting_dir()
            .ok_or_else(|| AppError::InvalidInput("plugin dir not set".into()))?;
        std::fs::create_dir_all(&dir)?;

        let (rt_url, is_zip) = runtime_archive();

        // 先探两个下载的总大小,合并成一个进度。
        let model_total = content_length(MODEL_URL).await.unwrap_or(0);
        let rt_total = content_length(&rt_url).await.unwrap_or(0);
        let grand = model_total + rt_total;
        let mut done = 0u64;

        // 1) 模型直接落盘。
        download_to(
            MODEL_URL,
            &dir.join("u2netp.onnx"),
            &mut done,
            grand,
            &progress,
        )
        .await?;

        // 2) 运行时包下载到临时文件,解压提取库。
        let archive = dir.join(if is_zip { "ort.zip" } else { "ort.tgz" });
        download_to(&rt_url, &archive, &mut done, grand, &progress).await?;
        extract_runtime(&archive, &dir.join(dylib_name()), is_zip)?;
        let _ = std::fs::remove_file(&archive);

        progress(grand, grand);
        Ok(())
    }

    /// 卸载插件(删除模型 + 库)。需重启才彻底释放已加载的库。
    pub fn uninstall_matting(&self) -> Result<()> {
        if let Some(dir) = self.matting_dir() {
            let _ = std::fs::remove_dir_all(&dir);
        }
        Ok(())
    }

    /// 对一张图去背景,返回透明背景 PNG 字节。插件未安装 / 未初始化则报错。
    pub async fn remove_background(&self, account: &str, path: &str) -> Result<Vec<u8>> {
        eprintln!("[matting] init runtime…");
        self.ensure_matting_init()?;
        let model = self
            .matting_model_path()
            .filter(|p| p.exists())
            .ok_or_else(|| AppError::InvalidInput("matting plugin not installed".into()))?;
        let model_bytes = std::fs::read(model)?;
        eprintln!(
            "[matting] model {} bytes; fetching source image",
            model_bytes.len()
        );
        let img = self.provider(account)?.read(path).await?.to_vec();
        eprintln!("[matting] source {} bytes; running inference", img.len());
        let out = tokio::task::spawn_blocking(move || {
            nebula_matting::remove_background(&model_bytes, &img)
        })
        .await
        .map_err(|e| AppError::Image(e.to_string()))?
        .map_err(|e| AppError::Image(e.to_string()))?;
        eprintln!("[matting] done, {} bytes PNG", out.len());
        Ok(out)
    }
}

/// HEAD 探测内容长度。
async fn content_length(url: &str) -> Option<u64> {
    let resp = reqwest::Client::new().head(url).send().await.ok()?;
    resp.content_length()
}

/// 流式下载到文件,把已下载字节累加进 `done` 并调 `progress`。
async fn download_to<F: Fn(u64, u64)>(
    url: &str,
    dest: &Path,
    done: &mut u64,
    grand: u64,
    progress: &F,
) -> Result<()> {
    let resp = reqwest::get(url)
        .await
        .map_err(|e| AppError::Image(e.to_string()))?;
    let mut file = tokio::fs::File::create(dest).await?;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| AppError::Image(e.to_string()))?;
        file.write_all(&chunk).await?;
        *done += chunk.len() as u64;
        progress(*done, grand);
    }
    file.flush().await?;
    Ok(())
}

/// 从下载的压缩包里提取 ONNX Runtime 库到 `dest`。
fn extract_runtime(archive: &Path, dest: &Path, is_zip: bool) -> Result<()> {
    let f = std::fs::File::open(archive)?;
    if is_zip {
        let mut zip = zip::ZipArchive::new(f).map_err(|e| AppError::Image(e.to_string()))?;
        for i in 0..zip.len() {
            let mut entry = zip
                .by_index(i)
                .map_err(|e| AppError::Image(e.to_string()))?;
            let name = entry.name().to_string();
            if is_runtime_lib(&name) {
                let mut out = std::fs::File::create(dest)?;
                std::io::copy(&mut entry, &mut out)?;
                return Ok(());
            }
        }
    } else {
        let gz = flate2::read::GzDecoder::new(f);
        let mut tar = tar::Archive::new(gz);
        for entry in tar.entries().map_err(|e| AppError::Image(e.to_string()))? {
            let mut entry = entry.map_err(|e| AppError::Image(e.to_string()))?;
            // 只要常规文件:`libonnxruntime.dylib` 是指向带版本号真实库的软链,
            // 软链项 read 出 0 字节会写出坏库,必须跳过。
            if !entry.header().entry_type().is_file() {
                continue;
            }
            let name = entry
                .path()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default();
            if is_runtime_lib(&name) {
                let mut buf = Vec::new();
                entry.read_to_end(&mut buf)?;
                std::fs::write(dest, buf)?;
                return Ok(());
            }
        }
    }
    Err(AppError::Image(
        "onnxruntime library not found in archive".into(),
    ))
}
