//! Nebula 的 PDF 页面级操作:纯 Rust(lopdf),无原生依赖,CI 可直接编译。
//!
//! 核心是一个统一的 [`assemble`]:给定若干源 PDF 与一份「输出由哪些页按什么顺序、
//! 各自旋转多少度组成」的清单,产出新 PDF。它一并覆盖:
//! - 删除:清单里省略该页;
//! - 重排:按清单顺序;
//! - 旋转:每页带绝对旋转角(0/90/180/270);
//! - 合并:清单引用多个源文档;
//! - 提取 / 拆分:只取某个源文档的一部分页。
//!
//! 另有 [`info`] 读出页数、每页尺寸与当前旋转,供前端展示与缩略图布局。

use lopdf::{Dictionary, Document, Object, ObjectId};
use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum PdfError {
    #[error("pdf parse: {0}")]
    Parse(String),
    #[error("pdf save: {0}")]
    Save(String),
    #[error("empty output: no pages selected")]
    Empty,
    #[error("page out of range: doc {doc} has no page {page}")]
    PageRange { doc: usize, page: usize },
    #[error("source doc index {0} out of range")]
    DocRange(usize),
}

impl From<lopdf::Error> for PdfError {
    fn from(e: lopdf::Error) -> Self {
        PdfError::Parse(e.to_string())
    }
}

type Result<T> = std::result::Result<T, PdfError>;

/// 一页在源文档里的尺寸(PDF 用户单位,1/72 英寸)与当前旋转角。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageInfo {
    pub width: f32,
    pub height: f32,
    pub rotate: i64,
}

/// 一个 PDF 的页面概览。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PdfInfo {
    pub pages: Vec<PageInfo>,
}

/// 输出里的一页:来自第几个源文档、该文档里第几页(0 起)、最终绝对旋转角。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageSpec {
    pub doc: usize,
    pub page: usize,
    /// 绝对旋转(0/90/180/270);前端传最终值,避免重复累加。
    #[serde(default)]
    pub rotate: i64,
}

/// 输出文档的组成清单。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Assembly {
    pub pages: Vec<PageSpec>,
}

/// 页码 / 页眉页脚数字的摆放位置(相对页面可见方向)。
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NumberPos {
    TopLeft,
    TopCenter,
    TopRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
}

/// 加页码的参数。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageNumbers {
    /// 起始编号(第一页显示的数字)。
    pub start: i64,
    pub position: NumberPos,
    /// 字号(pt)。
    pub size: f32,
    /// 到页边的距离(pt)。
    pub margin: f32,
    /// 格式串,`{n}` 替换为页码;默认 `{n}`。
    #[serde(default)]
    pub format: String,
}

/// 文字水印参数。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Watermark {
    /// 水印文字。
    pub text: String,
    /// 字号(pt)。
    pub size: f32,
    /// 不透明度 0..1。
    pub opacity: f32,
    /// 旋转角(度,逆时针为正);常用 45。
    pub angle: f32,
    /// 灰度 0..255(0 黑 255 白);默认中灰。
    #[serde(default)]
    pub gray: u8,
    /// 是否平铺铺满整页(否则页面居中一处)。
    #[serde(default)]
    pub tile: bool,
}

/// 抽取整份 PDF 的纯文本,按页返回(下标即 0 起页序;无文字层的页为空串)。
///
/// 注意:扫描件(纯图片)没有文字层,抽出来会是空的——这属正常。
pub fn extract_text(bytes: &[u8]) -> Result<Vec<String>> {
    let doc = Document::load_mem(bytes)?;
    // get_pages() 的键是 1 起页码;按页码升序逐页抽取。
    let mut nums: Vec<u32> = doc.get_pages().into_keys().collect();
    nums.sort_unstable();
    let mut out = Vec::with_capacity(nums.len());
    for n in nums {
        let text = doc.extract_text(&[n]).unwrap_or_default();
        out.push(text.trim().to_string());
    }
    Ok(out)
}

/// 压缩瘦身:压缩所有未压缩的流、剪除未被引用的对象,并用对象流 + 交叉引用流保存。
///
/// 返回 `(新字节, 原大小, 新大小)`。对已高度压缩的 PDF 收益有限,属正常。
pub fn compress(bytes: &[u8]) -> Result<(Vec<u8>, usize, usize)> {
    let orig = bytes.len();
    let mut doc = Document::load_mem(bytes)?;

    // 1) 逐个压缩尚未带 Filter 的流(内容流 / 图片等)。
    for obj in doc.objects.values_mut() {
        if let Object::Stream(stream) = obj {
            let _ = stream.compress();
        }
    }
    // 2) 剪除没有被引用到的孤儿对象。
    doc.prune_objects();
    doc.renumber_objects();

    // 3) 用对象流 + 交叉引用流(PDF 1.5+)保存,把大量小对象打包压缩。
    let options = lopdf::SaveOptions::builder()
        .use_object_streams(true)
        .use_xref_streams(true)
        .compression_level(9)
        .build();
    let mut buf = Vec::new();
    doc.save_with_options(&mut buf, options)
        .map_err(|e| PdfError::Save(e.to_string()))?;

    // 若「瘦身」反而更大(小文件加对象流开销),退回普通保存取较小者。
    if buf.len() >= orig {
        let mut plain = Vec::new();
        if doc.save_to(&mut plain).is_ok() && plain.len() < buf.len() {
            buf = plain;
        }
    }
    let new = buf.len();
    Ok((buf, orig, new))
}

/// 读取 PDF 的页面概览(页数、每页尺寸与旋转)。
pub fn info(bytes: &[u8]) -> Result<PdfInfo> {
    let doc = Document::load_mem(bytes)?;
    let mut pages = Vec::new();
    for (_num, id) in doc.get_pages() {
        let (w, h) = page_size(&doc, id).unwrap_or((595.0, 842.0)); // 默认 A4
        let rotate = page_rotation(&doc, id);
        pages.push(PageInfo {
            width: w,
            height: h,
            rotate,
        });
    }
    Ok(PdfInfo { pages })
}

/// 页面框(MediaBox)尺寸,考虑可继承属性(向父节点回溯)。
fn page_size(doc: &Document, page_id: ObjectId) -> Option<(f32, f32)> {
    let media = inherited(doc, page_id, b"MediaBox")?;
    let arr = media.as_array().ok()?;
    if arr.len() != 4 {
        return None;
    }
    let n = |o: &Object| -> f32 {
        o.as_f32()
            .or_else(|_| o.as_i64().map(|v| v as f32))
            .unwrap_or(0.0)
    };
    let x0 = n(&arr[0]);
    let y0 = n(&arr[1]);
    let x1 = n(&arr[2]);
    let y1 = n(&arr[3]);
    Some(((x1 - x0).abs(), (y1 - y0).abs()))
}

/// 页面旋转(Rotate),可继承。
fn page_rotation(doc: &Document, page_id: ObjectId) -> i64 {
    inherited(doc, page_id, b"Rotate")
        .and_then(|o| o.as_i64().ok())
        .map(|r| r.rem_euclid(360))
        .unwrap_or(0)
}

/// 取页面字典上某个可继承属性:本页没有就沿 /Parent 向上找。
fn inherited(doc: &Document, page_id: ObjectId, key: &[u8]) -> Option<Object> {
    let mut cur = page_id;
    for _ in 0..32 {
        let dict = doc.get_object(cur).ok()?.as_dict().ok()?;
        if let Ok(v) = dict.get(key) {
            // 解引用间接对象。
            return Some(resolve(doc, v).unwrap_or_else(|| v.clone()));
        }
        match dict.get(b"Parent").ok().and_then(|p| p.as_reference().ok()) {
            Some(parent) => cur = parent,
            None => break,
        }
    }
    None
}

fn resolve(doc: &Document, o: &Object) -> Option<Object> {
    match o.as_reference() {
        Ok(id) => doc.get_object(id).ok().cloned(),
        Err(_) => Some(o.clone()),
    }
}

/// 按清单组装输出 PDF。见模块文档。
pub fn assemble(docs: &[&[u8]], asm: &Assembly) -> Result<Vec<u8>> {
    if asm.pages.is_empty() {
        return Err(PdfError::Empty);
    }

    // 1) 载入所有源文档,重编号到同一 id 空间,避免对象 id 冲突。
    let mut loaded: Vec<Document> = Vec::with_capacity(docs.len());
    let mut page_ids: Vec<Vec<ObjectId>> = Vec::with_capacity(docs.len());
    let mut next_id = 1u32;
    for bytes in docs {
        let mut d = Document::load_mem(bytes)?;
        d.renumber_objects_with(next_id);
        next_id = d.max_id + 1;
        let ids: Vec<ObjectId> = d.get_pages().into_values().collect();
        page_ids.push(ids);
        loaded.push(d);
    }

    // 2) 给输出的 Pages / Catalog 预留 id。
    let pages_id: ObjectId = (next_id, 0);
    let catalog_id: ObjectId = (next_id + 1, 0);

    // 3) 汇集所有源对象到输出;记录被选中页的新 id 并设置旋转 + Parent。
    let mut out = Document::with_version("1.5");
    for d in &loaded {
        for (id, obj) in &d.objects {
            out.objects.insert(*id, obj.clone());
        }
    }

    let mut kids: Vec<Object> = Vec::with_capacity(asm.pages.len());
    for spec in &asm.pages {
        let src = page_ids.get(spec.doc).ok_or(PdfError::DocRange(spec.doc))?;
        let page_id = *src.get(spec.page).ok_or(PdfError::PageRange {
            doc: spec.doc,
            page: spec.page,
        })?;
        // 该页可能有可继承属性(如 MediaBox / Resources)挂在原 Pages 节点上;
        // 我们换了 Parent,得把这些继承属性固化到页字典本身,避免丢失。
        materialize_inherited(&mut out, &loaded[spec.doc], page_id);
        if let Ok(page_dict) = out.get_object_mut(page_id).and_then(|o| o.as_dict_mut()) {
            let r = spec.rotate.rem_euclid(360);
            page_dict.set("Rotate", Object::Integer(r));
            page_dict.set("Parent", Object::Reference(pages_id));
        }
        kids.push(Object::Reference(page_id));
    }

    // 4) 新的 Pages 节点与 Catalog。
    let count = kids.len() as i64;
    let mut pages_dict = Dictionary::new();
    pages_dict.set("Type", Object::Name(b"Pages".to_vec()));
    pages_dict.set("Kids", Object::Array(kids));
    pages_dict.set("Count", Object::Integer(count));
    out.objects.insert(pages_id, Object::Dictionary(pages_dict));

    let mut catalog = Dictionary::new();
    catalog.set("Type", Object::Name(b"Catalog".to_vec()));
    catalog.set("Pages", Object::Reference(pages_id));
    out.objects.insert(catalog_id, Object::Dictionary(catalog));

    out.trailer.set("Root", Object::Reference(catalog_id));
    out.max_id = catalog_id.0;

    // 5) 清理未被引用的对象(被删掉的页及其资源),压缩后输出。
    out.prune_objects();
    out.renumber_objects();
    out.compress();

    let mut buf = Vec::new();
    out.save_to(&mut buf)
        .map_err(|e| PdfError::Save(e.to_string()))?;
    Ok(buf)
}

/// 给每页叠加页码(Helvetica 标准字体,无需嵌入)。返回新 PDF 字节。
///
/// 处理:页面 `/Rotate` 旋转(数字始终按可见方向正立)、内容流图形状态隔离
/// (用 q/Q 包裹,不受原内容残留状态影响)、`/Resources` 继承的固化。
pub fn add_page_numbers(bytes: &[u8], opts: &PageNumbers) -> Result<Vec<u8>> {
    let mut doc = Document::load_mem(bytes)?;
    let size = if opts.size > 0.0 { opts.size } else { 12.0 };
    let margin = if opts.margin > 0.0 { opts.margin } else { 24.0 };
    let fmt = if opts.format.is_empty() {
        "{n}"
    } else {
        opts.format.as_str()
    };

    // 一个共享的标准字体对象。
    let mut font = Dictionary::new();
    font.set("Type", Object::Name(b"Font".to_vec()));
    font.set("Subtype", Object::Name(b"Type1".to_vec()));
    font.set("BaseFont", Object::Name(b"Helvetica".to_vec()));
    let font_id = doc.add_object(Object::Dictionary(font));

    let page_ids: Vec<ObjectId> = doc.get_pages().into_values().collect();
    for (i, page_id) in page_ids.iter().enumerate() {
        let num = opts.start + i as i64;
        let text = fmt.replace("{n}", &num.to_string());

        let (w, h) = page_size(&doc, *page_id).unwrap_or((595.0, 842.0));
        let rotate = page_rotation(&doc, *page_id);
        // 旋转后可见尺寸:90/270 时宽高互换。
        let (vis_w, vis_h) = if rotate == 90 || rotate == 270 {
            (h, w)
        } else {
            (w, h)
        };
        // Helvetica 平均字宽约 0.5em,估算文本宽度用于居中 / 右对齐。
        let text_w = text.chars().count() as f32 * size * 0.5;
        let (top, right, center) = pos_flags(opts.position);
        let x = if center {
            (vis_w - text_w) / 2.0
        } else if right {
            vis_w - margin - text_w
        } else {
            margin
        };
        let y = if top { vis_h - margin - size } else { margin };

        let cm = rotation_cm(rotate, w, h);
        let esc = escape_pdf_text(&text);
        let content = format!("Q q {cm} BT /Fnb {size} Tf {x:.2} {y:.2} Td ({esc}) Tj ET Q");
        let lead_id = doc.add_object(lopdf::Stream::new(Dictionary::new(), b"q".to_vec()));
        let tail_id = doc.add_object(lopdf::Stream::new(Dictionary::new(), content.into_bytes()));

        append_contents(&mut doc, *page_id, lead_id, tail_id);
        ensure_font(&mut doc, *page_id, font_id);
    }

    let mut buf = Vec::new();
    doc.save_to(&mut buf)
        .map_err(|e| PdfError::Save(e.to_string()))?;
    Ok(buf)
}

/// 给每页加文字水印(Helvetica,半透明,可旋转,可平铺)。返回新 PDF 字节。
pub fn add_watermark(bytes: &[u8], wm: &Watermark) -> Result<Vec<u8>> {
    if wm.text.is_empty() {
        return Ok(bytes.to_vec());
    }
    let mut doc = Document::load_mem(bytes)?;
    let size = if wm.size > 0.0 { wm.size } else { 48.0 };
    let opacity = wm.opacity.clamp(0.05, 1.0);
    let gray = wm.gray as f32 / 255.0;
    let theta = wm.angle.to_radians();
    let (c, s) = (theta.cos(), theta.sin());

    // 共享字体 + 透明度 ExtGState。
    let mut font = Dictionary::new();
    font.set("Type", Object::Name(b"Font".to_vec()));
    font.set("Subtype", Object::Name(b"Type1".to_vec()));
    font.set("BaseFont", Object::Name(b"Helvetica".to_vec()));
    let font_id = doc.add_object(Object::Dictionary(font));

    let mut gs = Dictionary::new();
    gs.set("Type", Object::Name(b"ExtGState".to_vec()));
    gs.set("ca", Object::Real(opacity));
    gs.set("CA", Object::Real(opacity));
    let gs_id = doc.add_object(Object::Dictionary(gs));

    let esc = escape_pdf_text(&wm.text);
    let text_w = wm.text.chars().count() as f32 * size * 0.5;

    let page_ids: Vec<ObjectId> = doc.get_pages().into_values().collect();
    for page_id in &page_ids {
        let (w, h) = page_size(&doc, *page_id).unwrap_or((595.0, 842.0));
        let rotate = page_rotation(&doc, *page_id);
        let (vis_w, vis_h) = if rotate == 90 || rotate == 270 {
            (h, w)
        } else {
            (w, h)
        };

        // 平铺网格,或页面居中一处。半宽/半高偏移让文字中心落在锚点。
        let anchors: Vec<(f32, f32)> = if wm.tile {
            let step_x = (text_w + size * 2.0).max(size * 4.0);
            let step_y = size * 5.0;
            let mut v = Vec::new();
            let mut y = step_y * 0.5;
            while y < vis_h {
                let mut x = step_x * 0.3;
                while x < vis_w {
                    v.push((x, y));
                    x += step_x;
                }
                y += step_y;
            }
            v
        } else {
            vec![(vis_w / 2.0, vis_h / 2.0)]
        };

        // 文字矩阵的旋转分量:cos sin -sin cos。
        let ns = -s;
        let mut body = String::new();
        for (ax, ay) in anchors {
            // 文字中心对齐到锚点:沿文字方向回退半宽,再垂直回退约 0.35em。
            let sx = ax - (text_w / 2.0) * c + (0.35 * size) * s;
            let sy = ay - (text_w / 2.0) * s - (0.35 * size) * c;
            body.push_str(&format!(
                "{c:.4} {s:.4} {ns:.4} {c:.4} {sx:.2} {sy:.2} Tm ({esc}) Tj "
            ));
        }

        let cm = rotation_cm(rotate, w, h);
        let content = format!("Q q {cm}/GSnb gs {gray:.3} g BT /Fnb {size} Tf {body}ET Q");
        let lead_id = doc.add_object(lopdf::Stream::new(Dictionary::new(), b"q".to_vec()));
        let tail_id = doc.add_object(lopdf::Stream::new(Dictionary::new(), content.into_bytes()));
        append_contents(&mut doc, *page_id, lead_id, tail_id);
        ensure_font(&mut doc, *page_id, font_id);
        ensure_resource(&mut doc, *page_id, b"ExtGState", b"GSnb", gs_id);
    }

    let mut buf = Vec::new();
    doc.save_to(&mut buf)
        .map_err(|e| PdfError::Save(e.to_string()))?;
    Ok(buf)
}

/// (top, right, center) 布尔标志。
fn pos_flags(p: NumberPos) -> (bool, bool, bool) {
    use NumberPos::*;
    match p {
        TopLeft => (true, false, false),
        TopCenter => (true, false, true),
        TopRight => (true, true, false),
        BottomLeft => (false, false, false),
        BottomCenter => (false, false, true),
        BottomRight => (false, true, false),
    }
}

/// 把「可见方向坐标」映射回页面基坐标的 CTM(cm 操作符串,含末尾空格)。
fn rotation_cm(rotate: i64, w: f32, h: f32) -> String {
    match rotate {
        90 => format!("0 1 -1 0 {w} 0 cm "),
        180 => format!("-1 0 0 -1 {w} {h} cm "),
        270 => format!("0 -1 1 0 0 {h} cm "),
        _ => String::new(),
    }
}

/// PDF 字面字符串转义:`\`、`(`、`)`。
fn escape_pdf_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '(' => out.push_str("\\("),
            ')' => out.push_str("\\)"),
            _ => out.push(c),
        }
    }
    out
}

/// 把「前置 q」与「叠加文字」两个流并入页面 /Contents(包裹隔离原内容状态)。
fn append_contents(doc: &mut Document, page_id: ObjectId, lead: ObjectId, tail: ObjectId) {
    let existing: Vec<Object> = match doc
        .get_object(page_id)
        .ok()
        .and_then(|o| o.as_dict().ok())
        .and_then(|d| d.get(b"Contents").ok())
    {
        Some(Object::Reference(r)) => vec![Object::Reference(*r)],
        Some(Object::Array(a)) => a.clone(),
        _ => Vec::new(),
    };
    let mut kids = Vec::with_capacity(existing.len() + 2);
    kids.push(Object::Reference(lead));
    kids.extend(existing);
    kids.push(Object::Reference(tail));
    if let Ok(dict) = doc.get_object_mut(page_id).and_then(|o| o.as_dict_mut()) {
        dict.set("Contents", Object::Array(kids));
    }
}

/// 确保页面 /Resources 里有我们的字体 /Fnb。
fn ensure_font(doc: &mut Document, page_id: ObjectId, font_id: ObjectId) {
    ensure_resource(doc, page_id, b"Font", b"Fnb", font_id);
}

/// 在页面 /Resources 的某个类别子字典(Font / ExtGState / …)里登记 `key → id`。
/// 固化继承的 Resources 到本页,避免污染共享对象。
fn ensure_resource(
    doc: &mut Document,
    page_id: ObjectId,
    category: &[u8],
    key: &[u8],
    id: ObjectId,
) {
    // 取本页有效的 Resources(inline / 引用 / 继承),克隆成独立字典。
    let mut res = match inherited(doc, page_id, b"Resources") {
        Some(Object::Dictionary(d)) => d,
        _ => Dictionary::new(),
    };
    // 取或建类别子字典(可能是引用)。
    let mut sub = match res.get(category) {
        Ok(Object::Dictionary(d)) => d.clone(),
        Ok(Object::Reference(r)) => doc
            .get_object(*r)
            .ok()
            .and_then(|o| o.as_dict().ok())
            .cloned()
            .unwrap_or_default(),
        _ => Dictionary::new(),
    };
    sub.set(key.to_vec(), Object::Reference(id));
    res.set(category.to_vec(), Object::Dictionary(sub));
    if let Ok(dict) = doc.get_object_mut(page_id).and_then(|o| o.as_dict_mut()) {
        dict.set("Resources", Object::Dictionary(res));
    }
}

/// 把挂在祖先 Pages 节点上的可继承属性(MediaBox / CropBox / Resources / Rotate)
/// 复制到页字典本身,这样换掉 Parent 后不会丢。
fn materialize_inherited(out: &mut Document, src: &Document, page_id: ObjectId) {
    const KEYS: [&[u8]; 4] = [b"MediaBox", b"CropBox", b"Resources", b"Rotate"];
    let mut to_set: Vec<(&[u8], Object)> = Vec::new();
    for key in KEYS {
        let has = out
            .get_object(page_id)
            .ok()
            .and_then(|o| o.as_dict().ok())
            .map(|d| d.has(key))
            .unwrap_or(false);
        if has {
            continue;
        }
        if let Some(v) = inherited(src, page_id, key) {
            to_set.push((key, v));
        }
    }
    if let Ok(dict) = out.get_object_mut(page_id).and_then(|o| o.as_dict_mut()) {
        for (key, v) in to_set {
            dict.set(key.to_vec(), v);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 造一个 N 页、每页尺寸不同的最小 PDF,用于测试。
    fn make_pdf(sizes: &[(i64, i64)]) -> Vec<u8> {
        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let mut kids = Vec::new();
        for (w, h) in sizes {
            let content_id = doc.add_object(lopdf::Stream::new(Dictionary::new(), b"".to_vec()));
            let mut page = Dictionary::new();
            page.set("Type", Object::Name(b"Page".to_vec()));
            page.set("Parent", Object::Reference(pages_id));
            page.set(
                "MediaBox",
                Object::Array(vec![0.into(), 0.into(), (*w).into(), (*h).into()]),
            );
            page.set("Contents", Object::Reference(content_id));
            let id = doc.add_object(Object::Dictionary(page));
            kids.push(Object::Reference(id));
        }
        let count = kids.len() as i64;
        let mut pages = Dictionary::new();
        pages.set("Type", Object::Name(b"Pages".to_vec()));
        pages.set("Kids", Object::Array(kids));
        pages.set("Count", Object::Integer(count));
        doc.objects.insert(pages_id, Object::Dictionary(pages));
        let mut cat = Dictionary::new();
        cat.set("Type", Object::Name(b"Catalog".to_vec()));
        cat.set("Pages", Object::Reference(pages_id));
        let catalog_id = doc.add_object(Object::Dictionary(cat));
        doc.trailer.set("Root", Object::Reference(catalog_id));
        let mut buf = Vec::new();
        doc.save_to(&mut buf).unwrap();
        buf
    }

    #[test]
    fn info_reports_pages_and_sizes() {
        let pdf = make_pdf(&[(200, 300), (400, 500)]);
        let got = info(&pdf).unwrap();
        assert_eq!(got.pages.len(), 2);
        assert_eq!(got.pages[0].width as i64, 200);
        assert_eq!(got.pages[0].height as i64, 300);
        assert_eq!(got.pages[1].width as i64, 400);
    }

    #[test]
    fn delete_and_reorder() {
        let pdf = make_pdf(&[(100, 100), (200, 200), (300, 300)]);
        // 只保留第 3、第 1 页(删掉第 2 页并倒序)。
        let asm = Assembly {
            pages: vec![
                PageSpec {
                    doc: 0,
                    page: 2,
                    rotate: 0,
                },
                PageSpec {
                    doc: 0,
                    page: 0,
                    rotate: 0,
                },
            ],
        };
        let out = assemble(&[&pdf], &asm).unwrap();
        let got = info(&out).unwrap();
        assert_eq!(got.pages.len(), 2);
        assert_eq!(got.pages[0].width as i64, 300);
        assert_eq!(got.pages[1].width as i64, 100);
    }

    #[test]
    fn rotate_is_absolute() {
        let pdf = make_pdf(&[(100, 200)]);
        let asm = Assembly {
            pages: vec![PageSpec {
                doc: 0,
                page: 0,
                rotate: 90,
            }],
        };
        let out = assemble(&[&pdf], &asm).unwrap();
        let got = info(&out).unwrap();
        assert_eq!(got.pages[0].rotate, 90);
    }

    #[test]
    fn merge_two_docs() {
        let a = make_pdf(&[(100, 100)]);
        let b = make_pdf(&[(200, 200), (300, 300)]);
        let asm = Assembly {
            pages: vec![
                PageSpec {
                    doc: 0,
                    page: 0,
                    rotate: 0,
                },
                PageSpec {
                    doc: 1,
                    page: 1,
                    rotate: 0,
                },
                PageSpec {
                    doc: 1,
                    page: 0,
                    rotate: 0,
                },
            ],
        };
        let out = assemble(&[&a, &b], &asm).unwrap();
        let got = info(&out).unwrap();
        assert_eq!(got.pages.len(), 3);
        assert_eq!(got.pages[0].width as i64, 100);
        assert_eq!(got.pages[1].width as i64, 300);
        assert_eq!(got.pages[2].width as i64, 200);
    }

    #[test]
    fn empty_is_rejected() {
        let pdf = make_pdf(&[(100, 100)]);
        assert!(matches!(
            assemble(&[&pdf], &Assembly { pages: vec![] }),
            Err(PdfError::Empty)
        ));
    }

    #[test]
    fn page_numbers_produce_valid_pdf() {
        let pdf = make_pdf(&[(300, 400), (300, 400), (300, 400)]);
        let opts = PageNumbers {
            start: 1,
            position: NumberPos::BottomCenter,
            size: 12.0,
            margin: 24.0,
            format: "第 {n} 页".into(),
        };
        let out = add_page_numbers(&pdf, &opts).unwrap();
        // 仍是有效 PDF、页数不变、尺寸不变。
        let got = info(&out).unwrap();
        assert_eq!(got.pages.len(), 3);
        assert_eq!(got.pages[0].width as i64, 300);
        // 输出应包含我们写入的字体基名与文本绘制操作符。
        let s = String::from_utf8_lossy(&out);
        assert!(s.contains("Helvetica"));
    }

    #[test]
    fn watermark_produces_valid_pdf() {
        let pdf = make_pdf(&[(400, 600), (400, 600)]);
        let wm = Watermark {
            text: "CONFIDENTIAL".into(),
            size: 48.0,
            opacity: 0.2,
            angle: 45.0,
            gray: 128,
            tile: true,
        };
        let out = add_watermark(&pdf, &wm).unwrap();
        let got = info(&out).unwrap();
        assert_eq!(got.pages.len(), 2);
        assert_eq!(got.pages[0].width as i64, 400);
        let s = String::from_utf8_lossy(&out);
        assert!(s.contains("ExtGState"));
    }

    #[test]
    fn compress_keeps_pages_and_reports_sizes() {
        let pdf = make_pdf(&[(300, 400), (300, 400)]);
        let (out, orig, new) = compress(&pdf).unwrap();
        assert_eq!(orig, pdf.len());
        assert_eq!(new, out.len());
        // 压缩后仍是有效 PDF、页数不变。
        let got = info(&out).unwrap();
        assert_eq!(got.pages.len(), 2);
        assert_eq!(got.pages[0].width as i64, 300);
    }

    #[test]
    fn extract_text_returns_one_entry_per_page() {
        // make_pdf 的页无文字层,应得到与页数一致的空串向量。
        let pdf = make_pdf(&[(200, 300), (200, 300), (200, 300)]);
        let pages = extract_text(&pdf).unwrap();
        assert_eq!(pages.len(), 3);
        assert!(pages.iter().all(|p| p.is_empty()));
    }

    #[test]
    fn watermark_empty_text_is_noop() {
        let pdf = make_pdf(&[(200, 200)]);
        let wm = Watermark {
            text: String::new(),
            size: 40.0,
            opacity: 0.3,
            angle: 0.0,
            gray: 128,
            tile: false,
        };
        let out = add_watermark(&pdf, &wm).unwrap();
        assert_eq!(out, pdf);
    }
}
