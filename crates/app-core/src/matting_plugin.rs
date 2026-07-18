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

/// 当前平台 + 架构的 ONNX Runtime 官方发布包 URL,以及是否 zip(否则 tgz)。
/// 覆盖:Windows x64/arm64、macOS(universal2,含 Intel 与 Apple Silicon)、Linux x64/arm64。
/// 返回 `None` 表示该平台 / 架构没有官方预编译包(如 32 位),插件不可用。
fn runtime_archive() -> Option<(String, bool)> {
    let v = ORT_VERSION;
    let base = "https://github.com/microsoft/onnxruntime/releases/download";
    let (slug, is_zip) = if cfg!(target_os = "windows") {
        match () {
            _ if cfg!(target_arch = "x86_64") => (format!("onnxruntime-win-x64-{v}"), true),
            _ if cfg!(target_arch = "aarch64") => (format!("onnxruntime-win-arm64-{v}"), true),
            _ => return None,
        }
    } else if cfg!(target_os = "macos") {
        // universal2 同时含 x86_64 与 arm64。
        (format!("onnxruntime-osx-universal2-{v}"), false)
    } else if cfg!(target_os = "linux") {
        match () {
            _ if cfg!(target_arch = "x86_64") => (format!("onnxruntime-linux-x64-{v}"), false),
            _ if cfg!(target_arch = "aarch64") => (format!("onnxruntime-linux-aarch64-{v}"), false),
            _ => return None,
        }
    } else {
        return None;
    };
    Some((
        format!("{base}/v{v}/{slug}.{}", if is_zip { "zip" } else { "tgz" }),
        is_zip,
    ))
}

/// 解压后成功提取的库最小合理体积(真实库都 > 10 MB;软链 / 坏文件会远小于此)。
const MIN_LIB_BYTES: u64 = 1_000_000;

/// 判断归档成员是不是要提取的主运行时库。只认文件名以 onnxruntime/libonnxruntime
/// 开头、带平台扩展名的主库本体;三平台归档结构不同,必须精确排除这些同名干扰项:
///
/// - macOS:`libonnxruntime.1.22.0.dylib.dSYM/…/DWARF/libonnxruntime.…dylib`(调试符号,同名);
/// - Windows:`onnxruntime.pdb`(调试符号);
/// - 三平台:`*_providers_shared.*`(provider 插件库)。
fn is_runtime_lib(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    if n.contains(".dsym") || n.contains("dwarf") || n.ends_with(".pdb") {
        return false; // 调试符号
    }
    let base = n.rsplit(['/', '\\']).next().unwrap_or(n.as_str());
    if !base.starts_with("onnxruntime") && !base.starts_with("libonnxruntime") {
        return false;
    }
    if base.contains("providers") {
        return false; // provider 插件库
    }
    base.contains(".dll") || base.contains(".dylib") || base.contains(".so")
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

    /// 诊断日志:同时写 stderr 和 `plugin_dir/matting.log`,GUI 启动看不到终端时也能取证。
    fn matting_log(&self, msg: &str) {
        eprintln!("[matting] {msg}");
        if let Some(dir) = self.plugin_dir.get() {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(dir.join("matting.log"))
            {
                use std::io::Write;
                let _ = writeln!(f, "{ts} {msg}");
            }
        }
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

        let (rt_url, is_zip) = runtime_archive().ok_or_else(|| {
            AppError::InvalidInput(
                "AI matting is not available on this platform / architecture".into(),
            )
        })?;

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
        let dylib = self.matting_dylib_path();
        let dylib_len = dylib
            .as_ref()
            .and_then(|p| std::fs::metadata(p).ok())
            .map(|m| m.len())
            .unwrap_or(0);
        self.matting_log(&format!("start; dylib={dylib:?} ({dylib_len} bytes)"));
        self.matting_log("init runtime…");
        self.ensure_matting_init()?;
        let model = self
            .matting_model_path()
            .filter(|p| p.exists())
            .ok_or_else(|| AppError::InvalidInput("matting plugin not installed".into()))?;
        let model_bytes = std::fs::read(model)?;
        self.matting_log(&format!(
            "runtime ok; model {} bytes; fetching source image",
            model_bytes.len()
        ));
        let img = self.provider(account)?.read(path).await?.to_vec();
        self.matting_log(&format!(
            "source {} bytes; calling inference (session build + run)",
            img.len()
        ));
        let out = tokio::task::spawn_blocking(move || {
            nebula_matting::remove_background(&model_bytes, &img)
        })
        .await
        .map_err(|e| AppError::Image(e.to_string()))?
        .map_err(|e| AppError::Image(e.to_string()))?;
        self.matting_log(&format!("done, {} bytes PNG", out.len()));
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

/// 从下载的压缩包里提取 ONNX Runtime 库到 `dest`,并校验体积合理(防止提取到软链 / 坏文件)。
fn extract_runtime(archive: &Path, dest: &Path, is_zip: bool) -> Result<()> {
    let f = std::fs::File::open(archive)?;
    if is_zip {
        let mut zip = zip::ZipArchive::new(f).map_err(|e| AppError::Image(e.to_string()))?;
        for i in 0..zip.len() {
            let mut entry = zip
                .by_index(i)
                .map_err(|e| AppError::Image(e.to_string()))?;
            if !entry.is_file() {
                continue;
            }
            let name = entry.name().to_string();
            if is_runtime_lib(&name) {
                let mut out = std::fs::File::create(dest)?;
                std::io::copy(&mut entry, &mut out)?;
                return verify_lib_size(dest);
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
                return verify_lib_size(dest);
            }
        }
    }
    Err(AppError::Image(
        "onnxruntime library not found in archive".into(),
    ))
}

/// 提取后校验:真实运行时库都很大(> 1 MB),过小说明取到了软链 / 坏文件。
fn verify_lib_size(dest: &Path) -> Result<()> {
    let len = std::fs::metadata(dest).map(|m| m.len()).unwrap_or(0);
    if len < MIN_LIB_BYTES {
        let _ = std::fs::remove_file(dest);
        return Err(AppError::Image(format!(
            "extracted runtime library looks corrupt ({len} bytes)"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::is_runtime_lib;

    // 文件清单取自 onnxruntime v1.22.0 官方发布包。名字过滤放行「像主库」的成员;
    // 真实文件与软链(如 Linux 的 .so / .so.1)都放行,再由提取时的 is_file 跳过软链。

    #[test]
    fn accepts_real_main_library_each_platform() {
        assert!(is_runtime_lib(
            "onnxruntime-osx-universal2-1.22.0/lib/libonnxruntime.1.22.0.dylib"
        ));
        assert!(is_runtime_lib(
            "onnxruntime-linux-x64-1.22.0/lib/libonnxruntime.so.1.22.0"
        ));
        assert!(is_runtime_lib(
            "onnxruntime-win-x64-1.22.0/lib/onnxruntime.dll"
        ));
    }

    #[test]
    fn rejects_debug_symbols() {
        // macOS 的 .dSYM 里有同名 dylib;Windows 的 .pdb。都不能当库提取。
        assert!(!is_runtime_lib(
            "onnxruntime-osx-universal2-1.22.0/lib/libonnxruntime.1.22.0.dylib.dSYM/Contents/Resources/DWARF/libonnxruntime.1.22.0.dylib"
        ));
        assert!(!is_runtime_lib(
            "onnxruntime-win-x64-1.22.0/lib/onnxruntime.pdb"
        ));
    }

    #[test]
    fn rejects_provider_plugins() {
        assert!(!is_runtime_lib(
            "onnxruntime-linux-x64-1.22.0/lib/libonnxruntime_providers_shared.so"
        ));
        assert!(!is_runtime_lib(
            "onnxruntime-win-x64-1.22.0/lib/onnxruntime_providers_shared.dll"
        ));
    }
}
