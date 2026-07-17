//! 图片浏览加速:在 Rust 侧下载 → 解码 → 缩放 → 编码,并把渲染结果按
//! (账号 + 路径 + ETag + 变体)缓存到磁盘,前端只拿到已缩好的小图(data URL)。
//!
//! 缓存的失效靠 ETag:云端对象变了(ETag 变)就自然命不中旧缓存。

use std::hash::{Hash, Hasher};
use std::path::PathBuf;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::{App, AppError, Result};

pub use nebula_image::{CropRect, ExifInfo, Ops};

/// 保存编辑结果的目标:写到哪、什么格式、什么画质。
#[derive(Debug, Clone, Deserialize)]
pub struct EditSave {
    /// 目标对象路径(等于源路径即覆盖原图)。
    pub dest: String,
    /// `png` 或 `jpeg`。
    pub format: String,
    /// JPEG 画质 1..100(PNG 忽略)。
    pub quality: u8,
}

/// 交给前端的一张渲染好的图:内嵌 data URL + 展示尺寸 + 原图尺寸。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageData {
    /// `data:image/jpeg;base64,...`,前端直接塞进 `<img src>`。
    pub data_url: String,
    /// 展示图(缩放后)宽高。
    pub width: u32,
    pub height: u32,
    /// 摆正后的原图宽高(信息面板 / 「实际像素」用)。
    pub orig_width: u32,
    pub orig_height: u32,
}

impl From<nebula_image::Rendered> for ImageData {
    fn from(r: nebula_image::Rendered) -> Self {
        let data_url = format!("data:{};base64,{}", r.mime, base64(&r.bytes));
        Self {
            data_url,
            width: r.width,
            height: r.height,
            orig_width: r.orig_width,
            orig_height: r.orig_height,
        }
    }
}

impl App {
    /// 设置图片缓存目录(Tauri 层用 `app_cache_dir()` 调一次)。只可设一次。
    pub fn set_cache_dir(&self, dir: impl Into<PathBuf>) {
        let _ = self.cache_dir.set(dir.into());
    }

    /// 渲染一张浏览用图(最长边 `max_edge`);命中缓存直接返回。
    pub async fn image_view(
        &self,
        account: &str,
        path: &str,
        etag: Option<String>,
        max_edge: u32,
    ) -> Result<ImageData> {
        let variant = format!("view-{max_edge}");
        if let Some(hit) = self.cache_get::<ImageData>(account, path, &etag, &variant) {
            return Ok(hit);
        }
        let bytes = self.fetch_bytes(account, path).await?;
        let rendered = spawn_render(move || nebula_image::render_view(&bytes, max_edge)).await?;
        let data = ImageData::from(rendered);
        self.cache_put(account, path, &etag, &variant, &data);
        Ok(data)
    }

    /// 渲染一张方形缩略图(边长 `size`);命中缓存直接返回。
    pub async fn image_thumb(
        &self,
        account: &str,
        path: &str,
        etag: Option<String>,
        size: u32,
    ) -> Result<ImageData> {
        let variant = format!("thumb-{size}");
        if let Some(hit) = self.cache_get::<ImageData>(account, path, &etag, &variant) {
            return Ok(hit);
        }
        let bytes = self.fetch_bytes(account, path).await?;
        let rendered = spawn_render(move || nebula_image::render_thumb(&bytes, size)).await?;
        let data = ImageData::from(rendered);
        self.cache_put(account, path, &etag, &variant, &data);
        Ok(data)
    }

    /// 解析对象的 EXIF 摘要;命中缓存直接返回。
    pub async fn image_exif(
        &self,
        account: &str,
        path: &str,
        etag: Option<String>,
    ) -> Result<ExifInfo> {
        if let Some(hit) = self.cache_get::<ExifInfo>(account, path, &etag, "exif") {
            return Ok(hit);
        }
        let bytes = self.fetch_bytes(account, path).await?;
        let info = spawn_blocking_ok(move || nebula_image::read_exif(&bytes)).await?;
        self.cache_put(account, path, &etag, "exif", &info);
        Ok(info)
    }

    /// 编辑预览:在缓存的原图上应用操作 → 缩到 `max_edge` → 返回 data URL。
    /// 用缓存原图,拖滑块 / 旋转时无需反复下载。
    pub async fn image_edit_preview(
        &self,
        account: &str,
        path: &str,
        etag: Option<String>,
        ops: Ops,
        max_edge: u32,
    ) -> Result<ImageData> {
        // 裁剪坐标基于原图像素,须在全分辨率上做;其余操作在缩小底图上预览即可(大图快很多)。
        let rendered = if ops.crop.is_some() {
            let orig = self.original_bytes(account, path, &etag).await?;
            spawn_render(move || nebula_image::render_edit(&orig, &ops, Some(max_edge))).await?
        } else {
            let base = self.edit_base(account, path, &etag, max_edge).await?;
            spawn_render(move || nebula_image::render_edit(&base, &ops, None)).await?
        };
        Ok(ImageData::from(rendered))
    }

    /// 保存编辑结果:全分辨率应用操作 → 按 `format`/`quality` 编码 → 写回云端 `dest`
    /// (`dest == path` 即覆盖原图;否则另存为新对象)。
    pub async fn image_edit_save(
        &self,
        account: &str,
        path: &str,
        etag: Option<String>,
        ops: Ops,
        save: EditSave,
    ) -> Result<()> {
        let orig = self.original_bytes(account, path, &etag).await?;
        let EditSave {
            dest,
            format,
            quality,
        } = save;
        let (bytes, mime) = tokio::task::spawn_blocking(move || {
            nebula_image::encode_edit(&orig, &ops, &format, quality)
        })
        .await
        .map_err(|e| AppError::Image(e.to_string()))?
        .map_err(|e| AppError::Image(e.to_string()))?;
        self.provider(account)?
            .write(&dest, bytes::Bytes::from(bytes), Some(&mime))
            .await?;
        Ok(())
    }

    /// 下载对象原始字节。
    async fn fetch_bytes(&self, account: &str, path: &str) -> Result<Vec<u8>> {
        Ok(self.provider(account)?.read(path).await?.to_vec())
    }

    /// 取「编辑底图」:原图缩到 `max_edge` 的无损 PNG,磁盘缓存。
    /// 大图编辑时,调参预览都在这张小图上做,不必反复解码 / 缩放原图。
    async fn edit_base(
        &self,
        account: &str,
        path: &str,
        etag: &Option<String>,
        max_edge: u32,
    ) -> Result<Vec<u8>> {
        let variant = format!("editbase-{max_edge}");
        if let Some(dir) = self.cache_dir.get() {
            let file = dir.join("editbase").join(format!(
                "{:016x}.png",
                digest(account, path, etag, &variant)
            ));
            if let Ok(bytes) = std::fs::read(&file) {
                return Ok(bytes);
            }
            let orig = self.original_bytes(account, path, etag).await?;
            let png = spawn_blocking_ok(move || nebula_image::downscaled_png(&orig, max_edge))
                .await?
                .map_err(|e| AppError::Image(e.to_string()))?;
            if let Some(parent) = file.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(&file, &png);
            return Ok(png);
        }
        let orig = self.original_bytes(account, path, etag).await?;
        spawn_blocking_ok(move || nebula_image::downscaled_png(&orig, max_edge))
            .await?
            .map_err(|e| AppError::Image(e.to_string()))
    }

    /// 取原图字节:命中磁盘缓存直接读,否则下载并缓存(编辑时反复用)。
    async fn original_bytes(
        &self,
        account: &str,
        path: &str,
        etag: &Option<String>,
    ) -> Result<Vec<u8>> {
        if let Some(dir) = self.cache_dir.get() {
            let file = dir
                .join("orig")
                .join(format!("{:016x}.bin", digest(account, path, etag, "orig")));
            if let Ok(bytes) = std::fs::read(&file) {
                return Ok(bytes);
            }
            let bytes = self.fetch_bytes(account, path).await?;
            if let Some(parent) = file.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(&file, &bytes);
            return Ok(bytes);
        }
        self.fetch_bytes(account, path).await
    }

    /// 某个渲染变体的缓存文件路径;未设缓存目录时返回 `None`(直接走渲染)。
    fn cache_file(
        &self,
        account: &str,
        path: &str,
        etag: &Option<String>,
        variant: &str,
    ) -> Option<PathBuf> {
        let dir = self.cache_dir.get()?;
        Some(dir.join("images").join(format!(
            "{:016x}.json",
            digest(account, path, etag, variant)
        )))
    }

    fn cache_get<T: DeserializeOwned>(
        &self,
        account: &str,
        path: &str,
        etag: &Option<String>,
        variant: &str,
    ) -> Option<T> {
        let file = self.cache_file(account, path, etag, variant)?;
        let text = std::fs::read_to_string(file).ok()?;
        serde_json::from_str(&text).ok()
    }

    fn cache_put<T: Serialize>(
        &self,
        account: &str,
        path: &str,
        etag: &Option<String>,
        variant: &str,
        value: &T,
    ) {
        let Some(file) = self.cache_file(account, path, etag, variant) else {
            return;
        };
        if let Some(parent) = file.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(bytes) = serde_json::to_vec(value) {
            let _ = std::fs::write(file, bytes);
        }
    }
}

/// (账号 + 路径 + ETag + 变体)的稳定哈希,用作缓存文件名。
fn digest(account: &str, path: &str, etag: &Option<String>, variant: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    account.hash(&mut h);
    path.hash(&mut h);
    etag.hash(&mut h);
    variant.hash(&mut h);
    h.finish()
}

/// 在阻塞线程池上跑图片渲染(解码/缩放是 CPU 密集,别占住 async 线程),并把两层错误摊平。
async fn spawn_render<F>(f: F) -> Result<nebula_image::Rendered>
where
    F: FnOnce() -> std::result::Result<nebula_image::Rendered, nebula_image::ImageError>
        + Send
        + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| AppError::Image(e.to_string()))?
        .map_err(|e| AppError::Image(e.to_string()))
}

/// 在阻塞线程池上跑一个不返回错误的图片相关计算(如 EXIF 解析)。
async fn spawn_blocking_ok<F, T>(f: F) -> Result<T>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| AppError::Image(e.to_string()))
}

/// 无依赖的标准 base64 编码(用于把渲染字节内联成 data URL)。
fn base64(input: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0];
        let b1 = *chunk.get(1).unwrap_or(&0);
        let b2 = *chunk.get(2).unwrap_or(&0);
        let n = ((b0 as u32) << 16) | ((b1 as u32) << 8) | (b2 as u32);
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            T[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::base64;

    #[test]
    fn base64_matches_known_vectors() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }
}
