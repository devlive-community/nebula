//! EXIF 解析(尽力而为:任何缺失字段都留空,绝不报错阻断浏览)。

use std::io::Cursor;

use exif::{In, Tag, Value};
use serde::{Deserialize, Serialize};

/// 从图片字节里解析出的 EXIF 摘要。字段全部可选。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExifInfo {
    /// 相机 / 手机厂商(Make)。
    pub make: Option<String>,
    /// 机型(Model)。
    pub model: Option<String>,
    /// 镜头型号(LensModel)。
    pub lens: Option<String>,
    /// 拍摄时间(DateTimeOriginal)。
    pub taken_at: Option<String>,
    /// 曝光时间,如 `1/200 s`。
    pub exposure: Option<String>,
    /// 光圈,如 `f/2.8`。
    pub aperture: Option<String>,
    /// ISO 感光度。
    pub iso: Option<String>,
    /// 焦距,如 `50 mm`。
    pub focal_length: Option<String>,
    /// EXIF 方向标记(1..8);渲染时用它把图摆正。
    pub orientation: Option<u16>,
    /// GPS 纬度(十进制度,南半球为负)。
    pub gps_lat: Option<f64>,
    /// GPS 经度(十进制度,西半球为负)。
    pub gps_lon: Option<f64>,
}

/// 解析 EXIF;无 EXIF 或解析失败时返回全空的 [`ExifInfo`](自身),不 panic。
pub fn read(bytes: &[u8]) -> ExifInfo {
    let mut info = ExifInfo::default();
    let mut cursor = Cursor::new(bytes);
    let reader = exif::Reader::new();
    let Ok(exif) = reader.read_from_container(&mut cursor) else {
        return info;
    };

    let text = |tag: Tag| {
        exif.get_field(tag, In::PRIMARY)
            .map(|f| f.display_value().with_unit(&exif).to_string())
            .map(|s| s.trim_matches('"').trim().to_string())
            .filter(|s| !s.is_empty())
    };

    info.make = text(Tag::Make);
    info.model = text(Tag::Model);
    info.lens = text(Tag::LensModel);
    info.taken_at = text(Tag::DateTimeOriginal).or_else(|| text(Tag::DateTime));
    info.exposure = text(Tag::ExposureTime);
    info.aperture = text(Tag::FNumber);
    info.iso = text(Tag::PhotographicSensitivity).or_else(|| text(Tag::ISOSpeed));
    info.focal_length = text(Tag::FocalLength);

    if let Some(o) = exif
        .get_field(Tag::Orientation, In::PRIMARY)
        .and_then(|f| f.value.get_uint(0))
    {
        info.orientation = Some(o as u16);
    }

    info.gps_lat = gps_coord(&exif, Tag::GPSLatitude, Tag::GPSLatitudeRef, 'S');
    info.gps_lon = gps_coord(&exif, Tag::GPSLongitude, Tag::GPSLongitudeRef, 'W');

    info
}

/// 只读 EXIF 方向标记(渲染前把图摆正用),无则返回 1(正常)。
pub fn orientation(bytes: &[u8]) -> u16 {
    read(bytes).orientation.unwrap_or(1)
}

/// 把 GPS 的「度/分/秒」有理数三元组 + 参考方向转成十进制度。
fn gps_coord(exif: &exif::Exif, coord: Tag, refer: Tag, negative_ref: char) -> Option<f64> {
    let field = exif.get_field(coord, In::PRIMARY)?;
    let Value::Rational(ref parts) = field.value else {
        return None;
    };
    if parts.len() < 3 {
        return None;
    }
    let deg = parts[0].to_f64() + parts[1].to_f64() / 60.0 + parts[2].to_f64() / 3600.0;
    let sign = exif
        .get_field(refer, In::PRIMARY)
        .map(|f| f.display_value().to_string())
        .map(|s| {
            if s.starts_with(negative_ref) {
                -1.0
            } else {
                1.0
            }
        })
        .unwrap_or(1.0);
    Some(deg * sign)
}
