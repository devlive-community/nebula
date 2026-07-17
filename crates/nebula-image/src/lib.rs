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

/// 目标尺寸(像素)。作为最后一步把结果缩放到该尺寸。
#[derive(Debug, Clone, Deserialize)]
pub struct Resize {
    pub width: u32,
    pub height: u32,
}

/// 一笔马赛克涂抹:折线各点为相对坐标 0..1;`width` 是相对图较长边的笔刷宽度比例。
#[derive(Debug, Clone, Deserialize)]
pub struct MosaicStroke {
    pub points: Vec<[f32; 2]>,
    pub width: f32,
}

/// 一笔画笔标注:折线各点为相对坐标 0..1;`width` 是相对图较长边的线宽比例。
#[derive(Debug, Clone, Deserialize)]
pub struct Stroke {
    pub points: Vec<[f32; 2]>,
    pub color: [u8; 3],
    pub width: f32,
}

/// 一组编辑操作。几何操作先应用,再颜色调整;字段全部可选(默认无变化)。
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Ops {
    #[serde(default)]
    pub crop: Option<CropRect>,
    /// 旋转角度,仅取 0 / 90 / 180 / 270。
    #[serde(default)]
    pub rotate: i32,
    /// 微调 / 拉直角度(度,顺时针为正),用于校正倾斜;保持画布尺寸,转出去的边角透明。
    #[serde(default)]
    pub straighten: f32,
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
    /// 饱和度,-100..100(-100 去饱和成灰、0 不变、100 加倍)。
    #[serde(default)]
    pub saturation: i32,
    /// 色温,-100..100(正数偏暖 / 加红减蓝,负数偏冷)。
    #[serde(default)]
    pub temperature: i32,
    /// 锐化强度,0..100(0 不锐化)。
    #[serde(default)]
    pub sharpen: i32,
    /// 色相旋转,-180..180 度(0 不变)。
    #[serde(default)]
    pub hue: i32,
    /// 高斯模糊强度,0..100(0 不模糊)。
    #[serde(default)]
    pub blur: i32,
    #[serde(default)]
    pub grayscale: bool,
    #[serde(default)]
    pub invert: bool,
    /// 目标尺寸;设了就在最后把结果缩放到该尺寸。
    #[serde(default)]
    pub resize: Option<Resize>,
    /// 马赛克涂抹(可多笔),在几何变换后、调色前应用。
    #[serde(default)]
    pub mosaics: Vec<MosaicStroke>,
    /// 画笔标注(可多笔),在所有处理的最后烧录进图。
    #[serde(default)]
    pub strokes: Vec<Stroke>,
}

impl Ops {
    /// 是否为「无任何改动」(用于保存前提示 / 跳过)。
    pub fn is_identity(&self) -> bool {
        self.crop.is_none()
            && self.rotate % 360 == 0
            && self.straighten == 0.0
            && !self.flip_h
            && !self.flip_v
            && self.brightness == 0
            && self.contrast == 0.0
            && self.saturation == 0
            && self.temperature == 0
            && self.sharpen == 0
            && self.hue == 0
            && self.blur == 0
            && !self.grayscale
            && !self.invert
            && self.resize.is_none()
            && self.mosaics.is_empty()
            && self.strokes.is_empty()
    }
}

/// 绕图中心按 `degrees`(顺时针为正)旋转任意角度,保持画布尺寸;转出画布的边角填透明。
///
/// 反向映射 + 双线性插值:对输出每个像素,反旋转到源坐标采样。主要用于「拉直」小角度倾斜。
fn rotate_arbitrary(img: &DynamicImage, degrees: f32) -> DynamicImage {
    use image::RgbaImage;
    let src = img.to_rgba8();
    let (w, h) = (src.width(), src.height());
    let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
    let theta = degrees.to_radians();
    let (sin, cos) = theta.sin_cos();
    let mut out = RgbaImage::new(w, h);
    for oy in 0..h {
        for ox in 0..w {
            let dx = ox as f32 - cx;
            let dy = oy as f32 - cy;
            // 反向:输出=源顺时针转 θ,故源坐标=输出逆时针转 θ。
            let sx = cos * dx + sin * dy + cx;
            let sy = -sin * dx + cos * dy + cy;
            let px = bilinear(&src, sx, sy);
            out.put_pixel(ox, oy, px);
        }
    }
    DynamicImage::ImageRgba8(out)
}

/// 在 `src` 的浮点坐标 `(sx, sy)` 处双线性采样;越界返回透明。
fn bilinear(src: &image::RgbaImage, sx: f32, sy: f32) -> image::Rgba<u8> {
    use image::Rgba;
    let (w, h) = (src.width() as i32, src.height() as i32);
    let x0 = sx.floor() as i32;
    let y0 = sy.floor() as i32;
    if x0 < 0 || y0 < 0 || x0 + 1 >= w || y0 + 1 >= h {
        return Rgba([0, 0, 0, 0]);
    }
    let fx = sx - x0 as f32;
    let fy = sy - y0 as f32;
    let p = |x: i32, y: i32| src.get_pixel(x as u32, y as u32).0;
    let (p00, p10, p01, p11) = (p(x0, y0), p(x0 + 1, y0), p(x0, y0 + 1), p(x0 + 1, y0 + 1));
    let mut out = [0u8; 4];
    for c in 0..4 {
        let top = p00[c] as f32 * (1.0 - fx) + p10[c] as f32 * fx;
        let bot = p01[c] as f32 * (1.0 - fx) + p11[c] as f32 * fx;
        out[c] = (top * (1.0 - fy) + bot * fy).round().clamp(0.0, 255.0) as u8;
    }
    Rgba(out)
}

/// 把整张图按 `block` 做块平均像素化,返回像素化后的副本。
fn pixelate_whole(src: &image::RgbaImage, block: u32) -> image::RgbaImage {
    let (w, h) = (src.width(), src.height());
    let mut out = src.clone();
    let mut by = 0;
    while by < h {
        let mut bx = 0;
        while bx < w {
            let (bw, bh) = ((bx + block).min(w), (by + block).min(h));
            let (mut sr, mut sg, mut sb, mut sa, mut n) = (0u64, 0u64, 0u64, 0u64, 0u64);
            for yy in by..bh {
                for xx in bx..bw {
                    let p = src.get_pixel(xx, yy).0;
                    sr += p[0] as u64;
                    sg += p[1] as u64;
                    sb += p[2] as u64;
                    sa += p[3] as u64;
                    n += 1;
                }
            }
            let avg = image::Rgba([
                (sr / n) as u8,
                (sg / n) as u8,
                (sb / n) as u8,
                (sa / n) as u8,
            ]);
            for yy in by..bh {
                for xx in bx..bw {
                    out.put_pixel(xx, yy, avg);
                }
            }
            bx += block;
        }
        by += block;
    }
    out
}

/// 沿马赛克涂抹路径,把圆形笔刷内的像素替换成像素化图对应像素(涂到哪打码到哪)。
fn apply_mosaics(img: &DynamicImage, strokes: &[MosaicStroke]) -> DynamicImage {
    let mut buf = img.to_rgba8();
    let (w, h) = (buf.width() as f32, buf.height() as f32);
    let long = w.max(h);
    let block = ((long * 0.02) as u32).max(6);
    let pixelated = pixelate_whole(&buf, block);
    let stamp = |buf: &mut image::RgbaImage, cx: f32, cy: f32, r: f32| {
        let r2 = r * r;
        let (x0, x1) = ((cx - r).floor() as i32, (cx + r).ceil() as i32);
        let (y0, y1) = ((cy - r).floor() as i32, (cy + r).ceil() as i32);
        for y in y0.max(0)..=y1.min(h as i32 - 1) {
            for x in x0.max(0)..=x1.min(w as i32 - 1) {
                let dx = x as f32 - cx;
                let dy = y as f32 - cy;
                if dx * dx + dy * dy <= r2 {
                    let p = *pixelated.get_pixel(x as u32, y as u32);
                    buf.put_pixel(x as u32, y as u32, p);
                }
            }
        }
    };
    for s in strokes {
        let r = (s.width * long / 2.0).max(block as f32);
        let pts: Vec<(f32, f32)> = s.points.iter().map(|p| (p[0] * w, p[1] * h)).collect();
        if pts.is_empty() {
            continue;
        }
        stamp(&mut buf, pts[0].0, pts[0].1, r);
        for seg in pts.windows(2) {
            let (p0, p1) = (seg[0], seg[1]);
            let dist = ((p1.0 - p0.0).powi(2) + (p1.1 - p0.1).powi(2)).sqrt();
            let steps = (dist / (r * 0.5)).ceil().max(1.0) as usize;
            for i in 1..=steps {
                let t = i as f32 / steps as f32;
                stamp(
                    &mut buf,
                    p0.0 + (p1.0 - p0.0) * t,
                    p0.1 + (p1.1 - p0.1) * t,
                    r,
                );
            }
        }
    }
    DynamicImage::ImageRgba8(buf)
}

/// 在 `buf` 上画一个填充圆(不透明覆盖)。
fn fill_circle(buf: &mut image::RgbaImage, cx: f32, cy: f32, r: f32, color: [u8; 3]) {
    let (w, h) = (buf.width() as i32, buf.height() as i32);
    let r2 = r * r;
    let (x0, x1) = ((cx - r).floor() as i32, (cx + r).ceil() as i32);
    let (y0, y1) = ((cy - r).floor() as i32, (cy + r).ceil() as i32);
    for y in y0.max(0)..=y1.min(h - 1) {
        for x in x0.max(0)..=x1.min(w - 1) {
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            if dx * dx + dy * dy <= r2 {
                buf.put_pixel(
                    x as u32,
                    y as u32,
                    image::Rgba([color[0], color[1], color[2], 255]),
                );
            }
        }
    }
}

/// 把画笔标注烧录进图。坐标相对 0..1,线宽相对图较长边的比例。
fn draw_strokes(img: &DynamicImage, strokes: &[Stroke]) -> DynamicImage {
    let mut buf = img.to_rgba8();
    let (w, h) = (buf.width() as f32, buf.height() as f32);
    let long = w.max(h);
    for s in strokes {
        let r = (s.width * long / 2.0).max(0.5);
        let pts: Vec<(f32, f32)> = s.points.iter().map(|p| (p[0] * w, p[1] * h)).collect();
        if pts.is_empty() {
            continue;
        }
        // 单点也画一个圆点。
        fill_circle(&mut buf, pts[0].0, pts[0].1, r, s.color);
        for seg in pts.windows(2) {
            let (p0, p1) = (seg[0], seg[1]);
            let dist = ((p1.0 - p0.0).powi(2) + (p1.1 - p0.1).powi(2)).sqrt();
            let steps = (dist / (r * 0.5)).ceil().max(1.0) as usize;
            for i in 1..=steps {
                let t = i as f32 / steps as f32;
                let x = p0.0 + (p1.0 - p0.0) * t;
                let y = p0.1 + (p1.1 - p0.1) * t;
                fill_circle(&mut buf, x, y, r, s.color);
            }
        }
    }
    DynamicImage::ImageRgba8(buf)
}

/// 逐像素调整饱和度与色温。`sat`/`temp` 均为 -100..100。
fn adjust_color(img: &DynamicImage, sat: i32, temp: i32) -> DynamicImage {
    let mut buf = img.to_rgba8();
    let factor = 1.0 + sat as f32 / 100.0; // -100→0(灰), 0→1, 100→2
    let shift = temp as f32 / 100.0 * 30.0; // 最多 ±30 的红/蓝偏移
    for p in buf.pixels_mut() {
        let [r, g, b, a] = p.0;
        let (mut rf, mut gf, mut bf) = (r as f32, g as f32, b as f32);
        if sat != 0 {
            let luma = 0.299 * rf + 0.587 * gf + 0.114 * bf;
            rf = luma + (rf - luma) * factor;
            gf = luma + (gf - luma) * factor;
            bf = luma + (bf - luma) * factor;
        }
        if temp != 0 {
            rf += shift;
            bf -= shift;
        }
        let clamp = |v: f32| v.round().clamp(0.0, 255.0) as u8;
        p.0 = [clamp(rf), clamp(gf), clamp(bf), a];
    }
    DynamicImage::ImageRgba8(buf)
}

/// 依次应用编辑操作:旋转 → 翻转 → 拉直 → 裁剪 → 亮度 → 对比度 → 饱和度 / 色温 → 灰度 → 反相 → 锐化。
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
    if ops.straighten != 0.0 {
        img = rotate_arbitrary(&img, ops.straighten);
    }
    if let Some(c) = &ops.crop {
        let (iw, ih) = (img.width(), img.height());
        if c.x < iw && c.y < ih {
            let w = c.width.min(iw - c.x).max(1);
            let h = c.height.min(ih - c.y).max(1);
            img = img.crop_imm(c.x, c.y, w, h);
        }
    }
    if !ops.mosaics.is_empty() {
        img = apply_mosaics(&img, &ops.mosaics);
    }
    if ops.brightness != 0 {
        img = img.brighten(ops.brightness);
    }
    if ops.contrast != 0.0 {
        img = img.adjust_contrast(ops.contrast);
    }
    if ops.saturation != 0 || ops.temperature != 0 {
        img = adjust_color(&img, ops.saturation, ops.temperature);
    }
    if ops.hue != 0 {
        img = img.huerotate(ops.hue);
    }
    if ops.grayscale {
        img = img.grayscale();
    }
    if ops.invert {
        img.invert();
    }
    if ops.blur > 0 {
        // 高斯模糊;sigma 越大越糊(0..100 → 0..10)。
        let sigma = (ops.blur.clamp(0, 100) as f32) / 100.0 * 10.0;
        img = img.blur(sigma);
    }
    if ops.sharpen > 0 {
        // unsharp mask;sigma 越大锐化越强(0..100 → 0..3)。
        let sigma = (ops.sharpen.clamp(0, 100) as f32) / 100.0 * 3.0;
        img = img.unsharpen(sigma, 0);
    }
    // 缩放放最后:对最终结果改尺寸(限个上限,避免异常大值)。
    if let Some(r) = &ops.resize {
        let w = r.width.clamp(1, 20_000);
        let h = r.height.clamp(1, 20_000);
        if (w, h) != (img.width(), img.height()) {
            img = img.resize_exact(w, h, FilterType::Lanczos3);
        }
    }
    // 画笔标注最后烧录(相对坐标,不受缩放影响)。
    if !ops.strokes.is_empty() {
        img = draw_strokes(&img, &ops.strokes);
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
/// `format` 取 `png`(无损)/ `jpeg`(有损,`quality` 1..100)/ `webp`(无损);其余按 `jpeg`。
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
    } else if format.eq_ignore_ascii_case("webp") {
        // image crate 的 WebP 编码为无损;不吃 quality。
        img.write_to(&mut out, ImageFormat::WebP)
            .map_err(|e| ImageError::Encode(e.to_string()))?;
        Ok((out.into_inner(), "image/webp".into()))
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
    fn straighten_keeps_size_and_transparent_corners() {
        let src = png(200, 200);
        let ops = Ops {
            straighten: 20.0,
            ..Default::default()
        };
        let r = render_edit(&src, &ops, None).unwrap();
        // 保持画布尺寸。
        assert_eq!((r.width, r.height), (200, 200));
        // 旋转后有透明边角 → 输出为 PNG。
        assert_eq!(r.mime, "image/png");
        // 左上角像素被转出画布,应为透明。
        let out = decode(&r.bytes).unwrap().to_rgba8();
        assert_eq!(out.get_pixel(0, 0).0[3], 0);
    }

    #[test]
    fn straighten_zero_is_identity() {
        assert!(Ops {
            straighten: 0.0,
            ..Default::default()
        }
        .is_identity());
    }

    #[test]
    fn saturation_minus_100_greys_out() {
        // 一张彩色图,饱和度拉到 -100,每个像素三通道应相等(去饱和成灰)。
        let mut img = RgbImage::new(8, 8);
        for (x, _y, p) in img.enumerate_pixels_mut() {
            *p = Rgb([200, 60, (x * 20) as u8]);
        }
        let mut out = Cursor::new(Vec::new());
        DynamicImage::ImageRgb8(img)
            .write_to(&mut out, ImageFormat::Png)
            .unwrap();
        let ops = Ops {
            saturation: -100,
            ..Default::default()
        };
        let r = render_edit(&out.into_inner(), &ops, None).unwrap();
        let rgba = decode(&r.bytes).unwrap().to_rgba8();
        for p in rgba.pixels() {
            let [rr, gg, bb, _] = p.0;
            assert!(rr.abs_diff(gg) <= 1 && gg.abs_diff(bb) <= 1);
        }
    }

    #[test]
    fn temperature_warm_raises_red_lowers_blue() {
        let src = png(4, 4); // 像素 blue 通道固定 128
        let ops = Ops {
            temperature: 100,
            ..Default::default()
        };
        let r = render_edit(&src, &ops, None).unwrap();
        let rgba = decode(&r.bytes).unwrap().to_rgba8();
        // png(4,4) 在 (2,2) 处原始为 Rgb([2, 2, 128])。
        let p = rgba.get_pixel(2, 2).0;
        assert!(p[0] as i32 > 2); // 红升
        assert!((p[2] as i32) < 128); // 蓝降
    }

    #[test]
    fn color_and_sharpen_zero_is_identity() {
        assert!(Ops {
            saturation: 0,
            temperature: 0,
            sharpen: 0,
            hue: 0,
            blur: 0,
            ..Default::default()
        }
        .is_identity());
    }

    #[test]
    fn hue_and_blur_apply_without_error() {
        let src = png(32, 32);
        let hue = Ops {
            hue: 90,
            ..Default::default()
        };
        let r = render_edit(&src, &hue, None).unwrap();
        assert_eq!((r.width, r.height), (32, 32));
        assert!(decode(&r.bytes).is_ok());
        let blur = Ops {
            blur: 50,
            ..Default::default()
        };
        let r = render_edit(&src, &blur, None).unwrap();
        assert_eq!((r.width, r.height), (32, 32));
        assert!(decode(&r.bytes).is_ok());
    }

    #[test]
    fn mosaic_pixelates_a_region() {
        // 一张每像素都不同的图,对中间区域打码后,区域内相邻像素应出现成块相等。
        let mut img = RgbImage::new(100, 100);
        for (x, y, p) in img.enumerate_pixels_mut() {
            *p = Rgb([(x * 2) as u8, (y * 2) as u8, ((x + y) % 256) as u8]);
        }
        let mut out = Cursor::new(Vec::new());
        DynamicImage::ImageRgb8(img)
            .write_to(&mut out, ImageFormat::Png)
            .unwrap();
        let ops = Ops {
            mosaics: vec![MosaicStroke {
                points: vec![[0.3, 0.5], [0.7, 0.5]], // 横向涂抹一笔
                width: 0.2,
            }],
            ..Default::default()
        };
        let r = render_edit(&out.into_inner(), &ops, None).unwrap();
        let rgba = decode(&r.bytes).unwrap().to_rgba8();
        // 涂抹经过的中点附近相邻像素应相等(落在同一马赛克块)。
        assert_eq!(rgba.get_pixel(50, 50).0, rgba.get_pixel(51, 50).0);
    }

    #[test]
    fn stroke_paints_its_color() {
        let src = png(100, 100); // (50,50) 原始约 Rgb([50,50,128])
        let ops = Ops {
            strokes: vec![Stroke {
                points: vec![[0.2, 0.5], [0.8, 0.5]], // 横穿中线
                color: [255, 0, 0],
                width: 0.05,
            }],
            ..Default::default()
        };
        let r = render_edit(&src, &ops, None).unwrap();
        let rgba = decode(&r.bytes).unwrap().to_rgba8();
        // 线经过的中点应为红色。
        assert_eq!(rgba.get_pixel(50, 50).0, [255, 0, 0, 255]);
        // 线外(左上角)不受影响。
        assert_ne!(rgba.get_pixel(2, 2).0, [255, 0, 0, 255]);
    }

    #[test]
    fn mosaics_empty_is_identity() {
        assert!(Ops::default().mosaics.is_empty() && Ops::default().is_identity());
    }

    #[test]
    fn edit_resize_changes_output_dimensions() {
        let src = png(800, 600);
        let ops = Ops {
            resize: Some(Resize {
                width: 400,
                height: 300,
            }),
            ..Default::default()
        };
        let r = render_edit(&src, &ops, None).unwrap();
        assert_eq!((r.width, r.height), (400, 300));
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
        let (webp_bytes, mime) = encode_edit(&src, &ops, "webp", 90).unwrap();
        assert_eq!(mime, "image/webp");
        assert!(decode(&webp_bytes).is_ok());
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
