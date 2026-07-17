//! Nebula 图片处理流水线(纯函数,无云依赖)。
//!
//! App 在 Rust 侧解码 / 缩放 / 编码 / 读 EXIF,前端只拿到「已经摆正、已经缩到视口大小」的
//! 小图,浏览器不必再啃几十 MB 的原图 —— 这是图片浏览器加速的核心。

mod exif;

use std::io::Cursor;

use image::imageops::FilterType;
use image::{DynamicImage, ImageFormat, ImageReader};
use serde::{Deserialize, Serialize};

pub use exif::{read as read_exif, ExifInfo};

/// 裁剪矩形,坐标是「摆正后的原图」像素。
#[derive(Debug, Clone, Deserialize)]
pub struct CropRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// 一组编辑操作。几何操作先应用,再颜色调整;字段全部可选(默认无变化)。
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Ops {
    #[serde(default)]
    pub crop: Option<CropRect>,
    /// 旋转角度,仅取 0 / 90 / 180 / 270。
    #[serde(default)]
    pub rotate: i32,
    #[serde(default)]
    pub flip_h: bool,
    #[serde(default)]
    pub flip_v: bool,
    /// 亮度增量,-100..100。
    #[serde(default)]
    pub brightness: i32,
    /// 对比度,-100..100(正数增强)。
    #[serde(default)]
    pub contrast: f32,
    #[serde(default)]
    pub grayscale: bool,
    #[serde(default)]
    pub invert: bool,
}

impl Ops {
    /// 是否为「无任何改动」(用于保存前提示 / 跳过)。
    pub fn is_identity(&self) -> bool {
        self.crop.is_none()
            && self.rotate % 360 == 0
            && !self.flip_h
            && !self.flip_v
            && self.brightness == 0
            && self.contrast == 0.0
            && !self.grayscale
            && !self.invert
    }
}

/// 依次应用编辑操作:旋转 → 翻转 → 裁剪 → 亮度 → 对比度 → 灰度 → 反相。
///
/// 裁剪放在旋转 / 翻转**之后**,裁剪坐标基于变换后的图 —— 前端在预览图上画框即所见即所裁。
fn apply_ops(mut img: DynamicImage, ops: &Ops) -> DynamicImage {
    img = match ((ops.rotate % 360) + 360) % 360 {
        90 => img.rotate90(),
        180 => img.rotate180(),
        270 => img.rotate270(),
        _ => img,
    };
    if ops.flip_h {
        img = img.fliph();
    }
    if ops.flip_v {
        img = img.flipv();
    }
    if let Some(c) = &ops.crop {
        let (iw, ih) = (img.width(), img.height());
        if c.x < iw && c.y < ih {
            let w = c.width.min(iw - c.x).max(1);
            let h = c.height.min(ih - c.y).max(1);
            img = img.crop_imm(c.x, c.y, w, h);
        }
    }
    if ops.brightness != 0 {
        img = img.brighten(ops.brightness);
    }
    if ops.contrast != 0.0 {
        img = img.adjust_contrast(ops.contrast);
    }
    if ops.grayscale {
        img = img.grayscale();
    }
    if ops.invert {
        img.invert();
    }
    img
}

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

/// 渲染一张「编辑预览」图:摆正 → 应用编辑操作 → 可选缩到 `max_edge` → 编码展示。
pub fn render_edit(bytes: &[u8], ops: &Ops, max_edge: Option<u32>) -> Result<Rendered> {
    let mut img = apply_ops(
        apply_orientation(decode(bytes)?, exif::orientation(bytes)),
        ops,
    );
    let (full_w, full_h) = (img.width(), img.height());
    if let Some(me) = max_edge {
        if me > 0 && full_w.max(full_h) > me {
            img = img.resize(me, me, FilterType::Lanczos3);
        }
    }
    let (bytes, mime) = encode_display(&img)?;
    Ok(Rendered {
        bytes,
        mime,
        width: img.width(),
        height: img.height(),
        orig_width: full_w,
        orig_height: full_h,
    })
}

/// 生成「编辑底图」:摆正 → 缩到 `max_edge` → 无损 PNG 编码。
///
/// 大图编辑时,先把原图缩到视口大小做成底图缓存起来;之后每次调参预览都在这张小图上应用操作,
/// 不必反复解码 / 缩放几千万像素的原图。PNG 无损,重复应用操作不会累积压缩失真。
pub fn downscaled_png(bytes: &[u8], max_edge: u32) -> Result<Vec<u8>> {
    let mut img = apply_orientation(decode(bytes)?, exif::orientation(bytes));
    if max_edge > 0 && img.width().max(img.height()) > max_edge {
        img = img.resize(max_edge, max_edge, FilterType::Lanczos3);
    }
    let mut out = Cursor::new(Vec::new());
    img.write_to(&mut out, ImageFormat::Png)
        .map_err(|e| ImageError::Encode(e.to_string()))?;
    Ok(out.into_inner())
}

/// 应用编辑操作后按指定格式全分辨率编码,用于保存回云端。
///
/// `format` 取 `png` 或 `jpeg`(其余按 `jpeg`);`quality` 仅 JPEG 用(1..100)。
pub fn encode_edit(
    bytes: &[u8],
    ops: &Ops,
    format: &str,
    quality: u8,
) -> Result<(Vec<u8>, String)> {
    let img = apply_ops(
        apply_orientation(decode(bytes)?, exif::orientation(bytes)),
        ops,
    );
    let mut out = Cursor::new(Vec::new());
    if format.eq_ignore_ascii_case("png") {
        img.write_to(&mut out, ImageFormat::Png)
            .map_err(|e| ImageError::Encode(e.to_string()))?;
        Ok((out.into_inner(), "image/png".into()))
    } else {
        let q = quality.clamp(1, 100);
        let rgb = img.to_rgb8();
        let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, q);
        enc.encode_image(&rgb)
            .map_err(|e| ImageError::Encode(e.to_string()))?;
        Ok((out.into_inner(), "image/jpeg".into()))
    }
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

    #[test]
    fn edit_rotate90_swaps_dimensions() {
        let src = png(800, 300);
        let ops = Ops {
            rotate: 90,
            ..Default::default()
        };
        let r = render_edit(&src, &ops, None).unwrap();
        assert_eq!((r.width, r.height), (300, 800));
    }

    #[test]
    fn edit_crop_produces_cropped_size() {
        let src = png(400, 400);
        let ops = Ops {
            crop: Some(CropRect {
                x: 50,
                y: 60,
                width: 100,
                height: 120,
            }),
            ..Default::default()
        };
        let r = render_edit(&src, &ops, None).unwrap();
        assert_eq!((r.width, r.height), (100, 120));
    }

    #[test]
    fn edit_crop_applies_after_rotate() {
        // 800×300 旋转 90° → 300×800,再裁一块 → 裁剪基于旋转后的尺寸。
        let src = png(800, 300);
        let ops = Ops {
            rotate: 90,
            crop: Some(CropRect {
                x: 10,
                y: 20,
                width: 200,
                height: 400,
            }),
            ..Default::default()
        };
        let r = render_edit(&src, &ops, None).unwrap();
        assert_eq!((r.width, r.height), (200, 400));
    }

    #[test]
    fn edit_crop_clamps_to_bounds() {
        let src = png(100, 100);
        // 越界的裁剪框应被夹到图内,不 panic。
        let ops = Ops {
            crop: Some(CropRect {
                x: 80,
                y: 80,
                width: 999,
                height: 999,
            }),
            ..Default::default()
        };
        let r = render_edit(&src, &ops, None).unwrap();
        assert_eq!((r.width, r.height), (20, 20));
    }

    #[test]
    fn encode_edit_png_and_jpeg_are_decodable() {
        let src = png(64, 48);
        let ops = Ops {
            grayscale: true,
            ..Default::default()
        };
        let (png_bytes, mime) = encode_edit(&src, &ops, "png", 90).unwrap();
        assert_eq!(mime, "image/png");
        assert!(decode(&png_bytes).is_ok());
        let (jpg_bytes, mime) = encode_edit(&src, &ops, "jpeg", 90).unwrap();
        assert_eq!(mime, "image/jpeg");
        assert!(decode(&jpg_bytes).is_ok());
    }

    #[test]
    fn downscaled_png_shrinks_and_stays_png() {
        let src = png(2000, 1000);
        let out = downscaled_png(&src, 500).unwrap();
        let m = meta(&out).unwrap();
        assert_eq!(m.format, "png");
        assert_eq!(m.width.max(m.height), 500);
    }

    #[test]
    fn ops_identity_detection() {
        assert!(Ops::default().is_identity());
        assert!(!Ops {
            rotate: 90,
            ..Default::default()
        }
        .is_identity());
    }
}
