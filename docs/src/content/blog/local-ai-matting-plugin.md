---
title: 把「AI 去背景」做成一个可下载的插件:本地抠图与三平台运行时的那些坑
date: 2026-07-18
author: Nebula Team
description: 给云端图片编辑器加一键去背景,不想让安装包平白胖几十 MB,也不想联网把用户的图传给第三方。这篇讲 Nebula 怎么把 ONNX Runtime + u2netp 做成「用时才下载」的插件、为什么运行时要懒加载、以及在 macOS / Windows / Linux 三个平台的发布包里精准挑出「真正那一个」动态库时踩的一串同名文件坑。
tags: ['开发', 'AI', '对象存储', '图片', '跨平台']
---

Nebula 的图片编辑器已经能裁剪、调色、打码、标注。下一个自然的需求是**一键去背景**——把主体抠出来、背景变透明。要做得体面,有两条硬约束:

1. **不能让默认安装包变胖。** 抠图要跑神经网络,ONNX Runtime 的动态库在 macOS 上就有 70 MB,直接打进包里,所有不用这个功能的人都得白背这份体积。
2. **不能把图传出去。** 这是个对象存储管理器,用户的图往往是私有资产。调云端 API 去背景,等于把原图交给第三方——不可接受。

结论很清楚:**本地推理 + 按需下载**。功能默认不存在,用户在设置里启用后才下载模型和运行时,像装一个插件。

## 插件式:默认不含运行时,用时才下载

模型选了 [u2netp](https://github.com/xuebinqin/U-2-Net)——一个 4.6 MB 的显著性分割网络,专门输出「哪些像素是前景」的掩码,正好拿来当 alpha 通道。运行时用 ONNX Runtime,通过 Rust 的 [`ort`](https://ort.pyke.io/) crate 调用。

关键在**怎么链接运行时**。`ort` 默认会在编译期把 ONNX Runtime 静态进二进制,这就把体积摊给了所有人,还要求 CI 环境准备好这套 C++ 库。我们用的是另一种模式:

```toml
ort = { version = "=2.0.0-rc.10", default-features = false, features = ["load-dynamic", "ndarray"] }
```

`load-dynamic` 表示**运行时才用 `dlopen` 去加载动态库**,编译期只生成 FFI 绑定。好处:

- 默认构建里没有 ONNX Runtime,安装包不变胖;
- CI 不需要任何 ONNX 依赖就能编译;
- 动态库由「插件安装」流程在运行时下载到用户数据目录。

抠图本身是一个不依赖任何云的纯函数 crate `nebula-matting`:输入图片字节 + 模型字节,输出透明 PNG 字节。

```rust
pub fn remove_background(model_bytes: &[u8], img_bytes: &[u8]) -> Result<Vec<u8>, MattingError> {
    let mut session = ort::session::Session::builder()?.commit_from_memory(model_bytes)?;
    let img = image::load_from_memory(img_bytes)?;
    // 前处理:缩到 320²、ImageNet 标准化、布局成 NCHW
    let tensor = ort::value::Tensor::from_array(([1usize, 3, 320, 320], input))?;
    let outputs = session.run(ort::inputs![tensor])?;
    // 后处理:d0 掩码归一化 → 缩回原图尺寸 → 当 alpha 贴回去 → 编码 PNG
    ...
}
```

设置里的插件卡片负责下载模型 + 平台对应的运行时包,合并成一条进度条,完成后提示重启生效。整套下来约 20 MB,只下一次。

## 懒加载:可选插件绝不能让应用打不开

第一版我们在**应用启动时**就初始化运行时——如果插件装了,就 `dlopen` 那个库。这是个错误,而且是差点让人删库的那种错误。

`dlopen` 一个 ABI 不匹配或损坏的动态库,可能不是干净地返回错误,而是**直接段错误杀掉整个进程**。这不是 Rust 的 panic,`catch_unwind` 抓不住。后果:用户在设置里点了「启用」、下载完重启,**应用一启动就崩,再也打不开**——一个可选功能把整个软件带走了。

修法是把加载推迟到**真正用到去背景那一刻**:

```rust
pub async fn remove_background(&self, account: &str, path: &str) -> Result<Vec<u8>> {
    self.ensure_matting_init()?;      // 用时才 dlopen,不在启动路径上
    ...
}
```

原则:**可选插件的失败,最多只能影响那个功能本身,绝不能影响应用启动。** 现在就算下载的库有问题,也只是那一次抠图报错,应用照常打开。

## 版本必须和 `ort` 对齐,否则 Session 创建会「卡死」

启用后点去背景,结果一直转圈——不报错,就是不返回。加了阶段日志才定位到:卡在 `Session::builder().commit_from_memory()`。

原因是运行时版本填错了。`ort` 通过 `OrtGetApiBase()->GetApi(ORT_API_VERSION)` 向动态库要指定版本的 API 结构体。翻 `ort-sys` 的 `build.rs`:

```rust
const ONNXRUNTIME_VERSION: &str = "1.22.0";   // ORT_API_VERSION = 22
```

而我们最初下载的是 1.16.3(API 版本 16),**差了六个大版本**。老库给不出 API 22,握手就卡在那儿。把下载版本对齐到 **1.22.0** 后这一步就通了。经验:**运行时库的版本不是随便挑的,必须和 `ort-sys` 里写死的那个严格一致**,升级 `ort` 时要同步改。

## 最深的坑:三平台发布包里,怎么挑出「真正那一个」库

版本对了,还是卡。这次问题在**解压提取**——我们要从官方发布包里挑出主运行时库,写到固定路径给 `dlopen`。第一版的判断很天真:

```rust
// 名字里有 onnxruntime + 动态库扩展名,就当是它 —— 太天真了
name.contains("onnxruntime") && (name.ends_with(".dll") || name.contains(".dylib") || name.contains(".so"))
```

问题是,**每个平台的发布包里都藏着和主库同名的干扰项**,这个条件会误中:

| 平台 | 真实主库 | 同名干扰项(会被误抓) |
|---|---|---|
| **macOS** | `libonnxruntime.1.22.0.dylib` | `.dSYM` 调试包里的 `…/DWARF/libonnxruntime.1.22.0.dylib` |
| **Windows** | `onnxruntime.dll`(12 MB) | `onnxruntime.pdb`(**357 MB** 调试符号) |
| **Linux** | `libonnxruntime.so.1.22.0` | `libonnxruntime.so` / `.so.1`(软链,读出 0 字节) |

抓到 dSYM 里的调试 dylib、或读软链得到的 0 字节坏文件,`dlopen` 都会以各种方式失败或卡住——这正是「一直处理中」的真身。而 Windows 那个 357 MB 的 `.pdb`,一旦误抓,不光加载失败,连下载解压都离谱。

修正后的判断分了几层:

```rust
fn is_runtime_lib(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    if n.contains(".dsym") || n.contains("dwarf") || n.ends_with(".pdb") {
        return false; // 排除调试符号
    }
    let base = n.rsplit(['/', '\\']).next().unwrap_or(&n);
    if !base.starts_with("onnxruntime") && !base.starts_with("libonnxruntime") {
        return false;
    }
    if base.contains("providers") {
        return false; // 排除 provider 插件库
    }
    base.contains(".dll") || base.contains(".dylib") || base.contains(".so")
}
```

再叠两道保险:

- **跳过软链。** 解压时 tar / zip 的软链条目,`read` 出来是 0 字节;只提取常规文件(`is_file`),Linux 那几个 `.so` 软链就自动让位给带完整版本号的真实文件。
- **体积兜底。** 提取完校验文件大小,小于 1 MB 直接判为坏文件报错——真实库都远大于此。宁可明确报「库损坏」,也不要留个坏库让用户卡在转圈里。

这套逻辑用三平台官方 1.22.0 发布包的**真实文件清单**写了单元测试锁死,不用起 GUI、不用下载,`cargo test` 就能保证每个平台都只挑中那一个对的文件。

## 前端:结果就是一张透明 PNG

编辑器工具栏里的剪刀按钮**只在插件装好后出现**(挂载时查一次安装状态)。点击后调 Rust 抠图,拿到透明 PNG 字节,直接作为当前预览显示(棋盘背景能看到透明区),保存时把这些字节原样写回云端或本地——因为要保留透明通道,格式强制 PNG。它独立于常规的调色/裁剪管线,点「重置」即可退回普通编辑。

## 手动精修:魔术棒与橡皮擦

自动抠图再准也会有边角残留,而且不是每次都想"抠整个主体"——有时只想**点掉某一片背景**。所以我们又加了一对纯前端、不依赖任何模型的局部擦除工具,和 AI 去背景**共用同一条透明 PNG 保存路径**:

- **魔术棒**:点一下,从该点向外泛洪,把颜色相近且相连的像素设成透明。就是一个按 RGB 曼哈顿距离判定的连通域填充,配一个「容差」滑块控制吃多大范围——点纯色背景一键就没。
- **橡皮擦**:按住涂抹,笔刷划过的像素用 canvas 的 `destination-out` 合成擦成透明,精修边角。

```js
// 魔术棒:从点击点泛洪,相近颜色置为透明
const limit = (tolerance / 100) * 765;      // RGB 曼哈顿距离上限
const stack = [sy * w + sx];
while (stack.length) {
  const p = stack.pop();
  const i = p * 4;
  if (seen[p] || d[i + 3] === 0) continue;
  const dist = Math.abs(d[i] - tr) + Math.abs(d[i+1] - tg) + Math.abs(d[i+2] - tb);
  if (dist > limit) continue;
  d[i + 3] = 0;                             // alpha 置 0
  seen[p] = 1;
  /* 四邻域入栈 */
}
```

两个关键点:一是**在全分辨率画布上操作**——进入擦除模式时,已抠过的用现有透明字节、否则取应用了当前编辑的全分辨率图,画到一张和原图等大的 `<canvas>`,屏幕上按 CSS 缩放显示,保存不降质;二是**撤销**——每次落笔前把整张 `getImageData` 压栈(限深 12),`putImageData` 即可回退。完成时 `canvas.toBlob('image/png')` 出透明字节,接管为当前结果。

于是「去背景」有了三种打法,还能叠用:AI 一键去大片 → 魔术棒补点漏网的背景 → 橡皮擦精修边缘。

## 小结

- **插件式 + `load-dynamic`**:重量级运行时按需下载,默认包不变胖,CI 零依赖。
- **本地推理**:用户的图不出本机。
- **懒加载**:可选功能的加载失败绝不牵连应用启动。
- **版本对齐**:运行时库版本必须和 `ort-sys` 写死的一致。
- **跨平台提取**:发布包里同名的调试符号(dSYM / pdb)和软链是主要陷阱,靠精确的名字过滤 + 跳软链 + 体积兜底 + 三平台测试挡住。

一个「一键去背景」按钮,背后是一条从安装包体积、隐私、进程健壮性到跨平台打包细节的完整取舍链。做扎实了,它才配只是工具栏上的一把剪刀。
