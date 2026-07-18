//! AI 抠图(去背景):用 ONNX Runtime 跑一个显著性分割模型(u2netp),
//! 输出前景掩码作为 alpha 通道,得到透明背景的 RGBA PNG。
//!
//! ONNX Runtime 以 `load-dynamic` 方式运行时加载:默认构建不含它,
//! 作为「插件」由用户在设置里启用后下载模型 + 运行时库,重启生效。

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Once;

use image::imageops::FilterType;
use image::{GenericImageView, RgbaImage};

/// u2netp 的输入边长。
const SIZE: usize = 320;

#[derive(Debug, thiserror::Error)]
pub enum MattingError {
    #[error("onnx runtime: {0}")]
    Ort(String),
    #[error("decode image: {0}")]
    Decode(String),
    #[error("encode image: {0}")]
    Encode(String),
    #[error("runtime not initialized (plugin not installed)")]
    NotInitialized,
}

impl From<ort::Error> for MattingError {
    fn from(e: ort::Error) -> Self {
        MattingError::Ort(e.to_string())
    }
}

static INIT: Once = Once::new();
static INIT_OK: AtomicBool = AtomicBool::new(false);

/// 用下载好的 ONNX Runtime 动态库初始化(整个进程一次)。插件启用 / 应用启动时调用。
pub fn init(dylib_path: impl AsRef<Path>) -> Result<(), MattingError> {
    let path = dylib_path.as_ref().to_path_buf();
    INIT.call_once(|| {
        let ok = ort::init_from(path.to_string_lossy().as_ref())
            .commit()
            .is_ok();
        INIT_OK.store(ok, Ordering::SeqCst);
    });
    if INIT_OK.load(Ordering::SeqCst) {
        Ok(())
    } else {
        Err(MattingError::NotInitialized)
    }
}

/// 去背景:输入图片字节 + 模型字节,返回透明背景的 PNG 字节。
pub fn remove_background(model_bytes: &[u8], img_bytes: &[u8]) -> Result<Vec<u8>, MattingError> {
    let mut session = ort::session::Session::builder()?.commit_from_memory(model_bytes)?;

    let img =
        image::load_from_memory(img_bytes).map_err(|e| MattingError::Decode(e.to_string()))?;
    let (ow, oh) = img.dimensions();

    // 前处理:缩到 320²、RGB、按最大值归一到 0..1、再做 ImageNet 标准化,布局 NCHW。
    let small = img
        .resize_exact(SIZE as u32, SIZE as u32, FilterType::Lanczos3)
        .to_rgb8();
    let mut maxv = 1.0f32;
    for p in small.pixels() {
        maxv = maxv.max(p[0] as f32).max(p[1] as f32).max(p[2] as f32);
    }
    let mean = [0.485f32, 0.456, 0.406];
    let std = [0.229f32, 0.224, 0.225];
    let mut input = vec![0f32; 3 * SIZE * SIZE];
    for y in 0..SIZE {
        for x in 0..SIZE {
            let p = small.get_pixel(x as u32, y as u32).0;
            for c in 0..3 {
                let v = (p[c] as f32 / maxv - mean[c]) / std[c];
                input[c * SIZE * SIZE + y * SIZE + x] = v;
            }
        }
    }

    let tensor = ort::value::Tensor::from_array(([1usize, 3, SIZE, SIZE], input))?;
    let outputs = session.run(ort::inputs![tensor])?;
    let (_shape, pred) = outputs[0].try_extract_tensor::<f32>()?;

    // 后处理:d0 掩码归一化到 0..1。
    let (mut mi, mut ma) = (f32::MAX, f32::MIN);
    for &v in pred.iter().take(SIZE * SIZE) {
        mi = mi.min(v);
        ma = ma.max(v);
    }
    let range = (ma - mi).max(1e-6);

    // 掩码缩回原图尺寸,作为 alpha 贴到原图上。
    let mut mask = image::GrayImage::new(SIZE as u32, SIZE as u32);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let v = (pred[y * SIZE + x] - mi) / range;
            mask.put_pixel(x as u32, y as u32, image::Luma([(v * 255.0) as u8]));
        }
    }
    let mask = image::imageops::resize(&mask, ow, oh, FilterType::Triangle);

    let rgb = img.to_rgba8();
    let mut out = RgbaImage::new(ow, oh);
    for y in 0..oh {
        for x in 0..ow {
            let mut px = rgb.get_pixel(x, y).0;
            px[3] = mask.get_pixel(x, y).0[0];
            out.put_pixel(x, y, image::Rgba(px));
        }
    }

    let mut buf = std::io::Cursor::new(Vec::new());
    out.write_to(&mut buf, image::ImageFormat::Png)
        .map_err(|e| MattingError::Encode(e.to_string()))?;
    Ok(buf.into_inner())
}
