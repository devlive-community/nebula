---
title: 云端 PDF 页面编辑器:用纯 Rust(lopdf)在后端组装,前端只画缩略图
date: 2026-07-20
author: Nebula Team
description: 给云上的 PDF 加一个既能阅读、又能删页 / 重排 / 旋转 / 合并 / 提取的编辑器。这篇讲为什么解析与改写放 Rust(lopdf,无原生依赖)、一个统一的 assemble 怎么把五种操作收敛成一次调用、跨文档合并时对象 id 重编号与可继承属性固化的坑,以及前端 pdf.js 如何只承担渲染而不碰文档结构。
tags: ['开发', '对象存储', 'PDF', 'Rust', '跨平台']
---

图片有了完整的浏览器 / 编辑器之后,下一个高频的云上文档就是 PDF。需求很朴素:**打开能读**,并且能做最常用的页面级整理——**删页、重排、旋转、合并、提取**。

一个诱惑是全用前端做:pdf.js 能渲染,再找个 JS 的 PDF 写库拼一拼。但我们定了相反的方向:**解析和改写全部放后端 Rust,前端只负责把页面画出来。** 原因有三:

1. **大文件不该拖垮界面。** 几百页、上百 MB 的 PDF,在 JS 里解析对象树、重写交叉引用表,足够让 webview 卡死。Rust 在后台线程干这些,界面始终跟手。
2. **正确性。** PDF 的对象模型(间接对象、可继承属性、交叉引用)细节很多,用一个成熟的 Rust 库(lopdf)比手搓 JS 靠谱得多。
3. **无原生依赖。** lopdf 是纯 Rust,不像某些方案要链接 native 的 pdfium。CI 直接编译,三平台一致。

## 一个 `assemble` 收敛五种操作

页面级编辑看着有五种功能,但它们本质是同一件事:**从若干源 PDF 里,挑出一些页,按某个顺序、各自带一个旋转角,拼成新文档。** 于是核心就一个函数:

```rust
pub struct PageSpec {
    pub doc: usize,    // 来自第几个源文档(0 = 主文档,1.. = 合并进来的)
    pub page: usize,   // 该文档里第几页(0 起)
    pub rotate: i64,   // 最终绝对旋转角 0/90/180/270
}
pub struct Assembly { pub pages: Vec<PageSpec> }

pub fn assemble(docs: &[&[u8]], asm: &Assembly) -> Result<Vec<u8>>;
```

映射关系:

- **删除** = 清单里不写这一页;
- **重排** = 清单顺序就是输出顺序;
- **旋转** = 每页带 `rotate`;
- **合并** = 清单里引用 `doc > 0` 的页;
- **提取 / 拆分** = 只取某个源文档的一个子集。

前端无论怎么拖拽、旋转、删、合并,最后都归约成一份 `Assembly` 交给后端。一个数据结构,五个功能,测试也好写。

另配一个 `info()` 读出页数、每页尺寸与当前旋转,供前端布局缩略图。

## 旋转为什么用「绝对角」

一个容易埋雷的决定:`rotate` 存**绝对角**,而不是「再转多少」。PDF 页面本身可能带 `/Rotate`,前端 `info()` 拿到的就是这个初始值;用户每点一次「向右转」,前端把它 `(r + 90) % 360`,发下来的永远是最终该显示的角度。后端直接 `set("Rotate", r)`,不做加法。

好处是**前后端对同一个数达成一致**:前端缩略图用 CSS `rotate(${r}deg)`、阅读视图用 pdf.js 的 `getViewport({ rotation: r })`,后端写进 PDF 的也是同一个 `r`。任何一端都不需要知道「原来转了多少、这次又加了多少」,避免了旋转被重复累加的经典 bug。

## 合并的真正难点:对象 id 重编号

单文档内删页 / 重排 / 旋转不难,难的是**合并**——把两个独立 PDF 的页拼到一起。PDF 内部是一堆「间接对象」,每个有个 id(如 `12 0 R`)。两个文档各自从 1 开始编号,直接混在一起 **id 会撞车**:A 的第 12 号和 B 的第 12 号会互相覆盖。

解法是载入后**把每个源文档整体重编号到一段不重叠的 id 空间**:

```rust
let mut next_id = 1u32;
for bytes in docs {
    let mut d = Document::load_mem(bytes)?;
    d.renumber_objects_with(next_id);   // 把这个文档的所有对象 id 平移到 next_id 起
    next_id = d.max_id + 1;             // 下一个文档接着排
    page_ids.push(d.get_pages().into_values().collect());
    loaded.push(d);
}
```

重编号之后,所有源对象搬进一个输出文档的对象表,再新建一个 `Pages` 树,`Kids` 按清单顺序引用选中页,`Count` 设对,最后挂一个新 `Catalog`。因为每个页对象引用的资源(字体、图像、内容流)都随重编号一起搬了过来,重新指一下 `Kids` 就能正确工作。

收尾三件事很关键:

```rust
out.prune_objects();     // 删掉没被引用的对象(被删的页及其独占资源)
out.renumber_objects();  // 压实 id
out.compress();          // 压缩流
```

`prune_objects` 让「删页」真的减小体积,而不是把删掉的页当孤儿留在文件里。

## 可继承属性:换了爹就得先把家当带走

PDF 有个坑叫「可继承属性」:`MediaBox`(页面尺寸)、`Resources`、`Rotate` 这些,页字典里可以不写,而是挂在祖先 `Pages` 节点上,由子页继承。

我们合并时给每页换了新的 `/Parent`(指向新建的 `Pages`)。如果原来的尺寸是从旧父节点继承的,**换爹之后就继承不到了**——页面会丢失尺寸,渲染成默认 A4 甚至出错。

所以在重指 `Parent` 之前,先把这些继承来的属性**固化到页字典本身**:

```rust
const KEYS: [&[u8]; 4] = [b"MediaBox", b"CropBox", b"Resources", b"Rotate"];
for key in KEYS {
    if page_dict_has(key) { continue; }         // 自己有就不动
    if let Some(v) = inherited(src, page_id, key) {
        page_dict.set(key, v);                  // 从祖先取来,写到自己身上
    }
}
```

`inherited` 沿 `/Parent` 向上回溯查找。这样无论原来属性挂在哪一层,换爹后都不丢。

## 前端:pdf.js 只画,不碰结构

前端一行 PDF 写操作都没有。它只做两件渲染:

- **缩略图网格**:每页用 pdf.js 以 0.4 缩放渲染成小图,`getViewport({ rotation: 0 })` 拿无旋转底图,再用 CSS `rotate` 施加当前角度——这样旋转是即时的,不必重渲。
- **阅读视图**:双击进入,按页面旋转角、乘设备像素比高清渲染到 canvas,支持翻页(方向键 / 按钮)与缩放(`+`/`-`)。这里直接用 `getViewport({ rotation })` 出正确朝向,文字锐利。

合并本地 PDF 时,前端读取文件字节、用 pdf.js 打开取缩略图,同时把这些字节作为「合并源」随 `Assembly` 一起发给后端。主文档仍由后端直接从云端读取。保存时后端组装完写回云端(覆盖 / 另存)或让你下载到本地。

## 叠加内容:页码与水印

页面级重组之外,还有两个高频需求是**往每页上叠东西**:页码和水印。它们本质相同——给每页追加一段绘制文字的内容流。这里有三个 PDF 的坑要过。

**一、内容流的图形状态会串味。** PDF 的 `/Contents` 可以是一个流,也可以是一组流,渲染时按顺序拼接成一条指令流。如果原页面内容改了坐标系(`cm`)或颜色却没用 `q`/`Q` 复原,你在后面追加的绘制就会继承这些残留状态,画歪画错。解法是**用 q/Q 把追加内容裹起来,并在最前面塞一个只含 `q` 的流**:

```
[ 前置流 "q" ]  [ ……原页面内容…… ]  [ 追加流 "Q q …绘制… Q" ]
```

前置 `q` 存下初始干净状态,原内容跑完后追加流开头的 `Q` 把状态**强制复位**,再 `q … Q` 在干净状态里画我们的东西。无论原内容多脏都不影响。

**二、页面旋转要归一。** 页面带 `/Rotate 90` 时,可见方向和基坐标系差 90°。如果直接按基坐标画页码,它会跟着页面一起转,变成侧躺的。所以先按 `/Rotate` 施加一个 `cm` 变换,把「可见方向坐标」映射回基坐标,再在可见坐标系里定位——页码 / 水印始终**正立朝向读者**。

**三、字体与透明度要挂进 Resources,还不能污染共享对象。** 画文字要有字体(用标准 Helvetica,无需嵌入),半透明水印要有一个 `ExtGState`(`/ca`、`/CA` 设 alpha)。这些都得登记到页面的 `/Resources`。但 `/Resources` 常常是多页**共享**的对象,直接改会波及别的页。所以先把本页有效的(可能继承来的)Resources 克隆成独立字典,加上我们的 `/Font /Fnb` 与 `/ExtGState /GSnb`,再作为 inline 字典挂回本页:

```rust
fn ensure_resource(doc, page_id, category, key, id) {
    let mut res = /* 本页有效 Resources 的克隆 */;
    let mut sub = /* res[category] 的克隆,或新建 */;
    sub.set(key, Reference(id));      // 如 /Font /Fnb → 字体对象
    res.set(category, sub);
    page.set("Resources", res);       // inline 挂回本页,不动共享对象
}
```

水印的旋转用**文字矩阵** `Tm`(`cosθ sinθ -sinθ cosθ tx ty`)直接编码任意角度,平铺则在可见页面上按网格重复绘制;透明度靠 `/GSnb gs` 引用那个 ExtGState 生效。页码则按六个方位(上下 × 左中右)计算锚点,用估算的文本宽度做居中 / 右对齐。

值得一提:**这两个功能都作用于「组装后的当前工作集」**,而不是云端原文件——即先跑一遍 `assemble`(你在编辑器里的重排 / 删除 / 合并结果),再往上叠页码 / 水印。所见即所得。

## 提取文本:把文字层抠出来

PDF 不只是像素,大多数(非扫描件)带一层**可提取的文字**。lopdf 的 `extract_text` 能按页把它抠出来:

```rust
pub fn extract_text(bytes: &[u8]) -> Result<Vec<String>> {
    let doc = Document::load_mem(bytes)?;
    let mut nums: Vec<u32> = doc.get_pages().into_keys().collect();
    nums.sort_unstable();                       // get_pages 的键是 1 起页码
    nums.iter()
        .map(|n| Ok(doc.extract_text(&[*n]).unwrap_or_default().trim().to_string()))
        .collect()
}
```

按页返回而不是糊成一大坨,前端就能带页码展示、复制或导出成 txt。一个要向用户讲清楚的点:**扫描件(纯图片)没有文字层,抽出来是空的**——这不是 bug,是这份 PDF 本来就没有文字可提。面板会把这种页明确标注为「本页无文字层」,而不是让人以为出错了。

要真正读出扫描件的文字得上 OCR(光学字符识别),那是另一条重得多的路(需要模型 + 渲染成图),不在这个纯 lopdf 的轻量功能范围里。

## 压缩瘦身:省的是云存储费

作为对象存储管理器,PDF 体积直接对应**存储与流量费**。很多 PDF 其实很「胖」:内容流没压缩、编辑历史留下一堆没人引用的孤儿对象、交叉引用表是老式明文。lopdf 能一次收拾干净:

```rust
pub fn compress(bytes: &[u8]) -> Result<(Vec<u8>, usize, usize)> {
    let mut doc = Document::load_mem(bytes)?;
    // 1) 压缩所有尚未带 Filter 的流(FlateDecode)
    for (_id, obj) in doc.objects.iter_mut() {
        if let Object::Stream(s) = obj { let _ = s.compress(); }
    }
    // 2) 剪掉没被引用的孤儿对象
    doc.prune_objects();
    doc.renumber_objects();
    // 3) 用对象流 + 交叉引用流保存,把大量小对象打包压缩
    let opts = SaveOptions::builder()
        .use_object_streams(true).use_xref_streams(true)
        .compression_level(9).build();
    doc.save_with_options(&mut buf, opts)?;
    ...
}
```

三招叠加:**流压缩**减小内容体积、**剪枝**去掉冗余对象、**对象流 / 交叉引用流**(PDF 1.5+ 特性)把成百上千个小间接对象打包进一个压缩流,省掉每个对象的头尾开销。

一个诚实的兜底:对**已经高度优化**的 PDF,加对象流的固定开销可能反而让文件变大。所以我们比较瘦身结果与普通保存,**取较小者**;若压不动就如实告诉用户「已是最优」,并把压缩前后的体积对比显示出来,省了多少一目了然。

## 数据流小结

```
双击/拖拽/旋转/删除/合并 (前端 pdf.js 渲染 + 交互)
        │  归约成
        ▼
  Assembly { pages: [{ doc, page, rotate }] }  +  合并源字节
        │  IPC
        ▼
  app-core: 从云端读主文档
        │
        ▼
  nebula-pdf::assemble  (重编号 → 固化继承属性 → 重建 Pages → 剪枝压缩)
        │
        ▼
  写回云端 / 下载本地
```

一句话:**前端负责「看得见的」,Rust 负责「靠得住的」。** 页面怎么拖、怎么转是交互问题,交给 pdf.js;文档结构怎么正确拼起来是工程问题,交给 lopdf。各司其职,大文件不卡、跨平台一致、结果可靠。
