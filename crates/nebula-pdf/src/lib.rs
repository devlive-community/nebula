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
}
