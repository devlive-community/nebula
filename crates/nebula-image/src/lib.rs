//! Nebula 图片处理流水线(纯函数,无云依赖)。
//!
//! App 在 Rust 侧解码 / 缩放 / 编码 / 读 EXIF,前端只拿到「已经摆正、已经缩到视口大小」的
//! 小图,浏览器不必再啃几十 MB 的原图 —— 这是图片浏览器加速的核心。

mod exif;

use std::io::Cursor;

use image::imageops::FilterType;
use image::{DynamicImage, ImageFormat, ImageReader};
use serde::Serialize;

pub use exif::{read as read_exif, ExifInfo};

/// 处理出错。任何一步失败都归到这里,调用方可回退到「让前端直接用预签名原图」。
#[derive(Debug, thiserror::Error)]
pub enum ImageError {
    #[error("decode image: {0}")]
    Decode(String),
    #[error("encode image: {0}")]
    Encode(String),
}

type Result<T> = std::result::Result<T, ImageError>;

/// 一张渲染好的图:编码后的字节 + 展示格式 + 展示尺寸 + 原图尺寸。
#[derive(Debug, Clone)]
pub struct Rendered {
    /// 编码后的字节(PNG 或 JPEG)。
    pub bytes: Vec<u8>,
    /// 展示图的 MIME,如 `image/jpeg`。
    pub mime: &'static str,
    /// 展示图(缩放后)的宽高。
    pub width: u32,
    pub height: u32,
    /// 摆正后的原图宽高(信息面板 / 「实际像素」用)。
    pub orig_width: u32,
    pub orig_height: u32,
}

/// 只解析图片尺寸与格式,不解码全部像素(尽量快)。
#[derive(Debug, Clone, Serialize)]
pub struct ImageMeta {
    pub width: u32,
    pub height: u32,
    pub format: String,
}

fn decode(bytes: &[u8]) -> Result<DynamicImage> {
    let reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| ImageError::Decode(e.to_string()))?;
    reader
        .decode()
        .map_err(|e| ImageError::Decode(e.to_string()))
}

/// 按 EXIF 方向标记把图摆正(1..8);其余值当作正常。
fn apply_orientation(img: DynamicImage, orientation: u16) -> DynamicImage {
    match orientation {
        2 => img.fliph(),
        3 => img.rotate180(),
        4 => img.flipv(),
        5 => img.rotate90().fliph(),
        6 => img.rotate90(),
        7 => img.rotate270().fliph(),
        8 => img.rotate270(),
        _ => img,
    }
}

/// 把图编码成适合展示的字节:带透明通道用 PNG(保透明),否则用 JPEG(更小)。
fn encode_display(img: &DynamicImage) -> Result<(Vec<u8>, &'static str)> {
    let mut out = Cursor::new(Vec::new());
    if img.color().has_alpha() {
        img.write_to(&mut out, ImageFormat::Png)
            .map_err(|e| ImageError::Encode(e.to_string()))?;
        Ok((out.into_inner(), "image/png"))
    } else {
        let rgb = img.to_rgb8();
        let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 85);
        enc.encode_image(&rgb)
            .map_err(|e| ImageError::Encode(e.to_string()))?;
        Ok((out.into_inner(), "image/jpeg"))
    }
}

/// 渲染一张「浏览用」的图:摆正 → 最长边缩到 `max_edge`(不放大)→ 编码。
///
/// 前端把它当作视口里的展示图;放大到实际像素时再按需请求原图 / 局部。
pub fn render_view(bytes: &[u8], max_edge: u32) -> Result<Rendered> {
    let mut img = apply_orientation(decode(bytes)?, exif::orientation(bytes));
    let (orig_w, orig_h) = (img.width(), img.height());
    if max_edge > 0 && orig_w.max(orig_h) > max_edge {
        // resize 会保持宽高比,把图缩进 max_edge × max_edge 的框里。
        img = img.resize(max_edge, max_edge, FilterType::Lanczos3);
    }
    let (bytes, mime) = encode_display(&img)?;
    Ok(Rendered {
        bytes,
        mime,
        width: img.width(),
        height: img.height(),
        orig_width: orig_w,
        orig_height: orig_h,
    })
}

/// 渲染一张方形缩略图:摆正 → 居中裁成正方形 → 缩到 `size` → 编码。
pub fn render_thumb(bytes: &[u8], size: u32) -> Result<Rendered> {
    let img = apply_orientation(decode(bytes)?, exif::orientation(bytes));
    let (orig_w, orig_h) = (img.width(), img.height());
    let edge = orig_w.min(orig_h);
    let x = (orig_w - edge) / 2;
    let y = (orig_h - edge) / 2;
    let square = img.crop_imm(x, y, edge, edge);
    let thumb = square.resize_exact(size, size, FilterType::Triangle);
    let (bytes, mime) = encode_display(&thumb)?;
    Ok(Rendered {
        bytes,
        mime,
        width: thumb.width(),
        height: thumb.height(),
        orig_width: orig_w,
        orig_height: orig_h,
    })
}

/// 只读尺寸 / 格式(不解码全部像素)。
pub fn meta(bytes: &[u8]) -> Result<ImageMeta> {
    let reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| ImageError::Decode(e.to_string()))?;
    let format = reader
        .format()
        .map(|f| format!("{f:?}").to_lowercase())
        .unwrap_or_else(|| "unknown".into());
    let (width, height) = reader
        .into_dimensions()
        .map_err(|e| ImageError::Decode(e.to_string()))?;
    Ok(ImageMeta {
        width,
        height,
        format,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageFormat, Rgb, RgbImage, Rgba, RgbaImage};

    /// 造一张 w×h 的不透明测试图(RGB,无 alpha 通道)并编码成 PNG 字节。
    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut img = RgbImage::new(w, h);
        for (x, y, p) in img.enumerate_pixels_mut() {
            *p = Rgb([(x % 256) as u8, (y % 256) as u8, 128]);
        }
        let mut out = Cursor::new(Vec::new());
        DynamicImage::ImageRgb8(img)
            .write_to(&mut out, ImageFormat::Png)
            .unwrap();
        out.into_inner()
    }

    #[test]
    fn view_downscales_longest_edge_and_keeps_aspect() {
        let src = png(1000, 400);
        let r = render_view(&src, 500).unwrap();
        // 最长边缩到 500,宽高比 1000:400 保持 → 500×200。
        assert_eq!(r.width, 500);
        assert_eq!(r.height, 200);
        assert_eq!(r.orig_width, 1000);
        assert_eq!(r.orig_height, 400);
        // 不透明源 → JPEG。
        assert_eq!(r.mime, "image/jpeg");
        // 输出确实是能被解回的合法图。
        assert!(decode(&r.bytes).is_ok());
    }

    #[test]
    fn view_does_not_upscale() {
        let src = png(200, 200);
        let r = render_view(&src, 4000).unwrap();
        assert_eq!(r.width, 200);
        assert_eq!(r.height, 200);
    }

    #[test]
    fn alpha_source_stays_png() {
        // 半透明像素 → 需要保留透明 → PNG。
        let mut img = RgbaImage::new(10, 10);
        for p in img.pixels_mut() {
            *p = Rgba([10, 20, 30, 128]);
        }
        let mut out = Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(img)
            .write_to(&mut out, ImageFormat::Png)
            .unwrap();
        let r = render_view(&out.into_inner(), 100).unwrap();
        assert_eq!(r.mime, "image/png");
    }

    #[test]
    fn thumb_is_square() {
        let src = png(800, 300);
        let r = render_thumb(&src, 128).unwrap();
        assert_eq!(r.width, 128);
        assert_eq!(r.height, 128);
    }

    #[test]
    fn meta_reads_dimensions_without_full_decode() {
        let src = png(640, 480);
        let m = meta(&src).unwrap();
        assert_eq!((m.width, m.height), (640, 480));
        assert_eq!(m.format, "png");
    }

    #[test]
    fn exif_on_plain_png_is_empty_not_error() {
        let src = png(16, 16);
        let e = read_exif(&src);
        assert!(e.make.is_none() && e.gps_lat.is_none());
    }
}
