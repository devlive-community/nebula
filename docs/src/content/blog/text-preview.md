---
title: 预览文本文件,为什么要绕一圈走后端
date: 2026-07-13
author: Nebula Team
description: 给对象存储管理器加文本 / 代码 / 配置文件预览,看似前端 fetch 一下就行。但云端 GET 默认不给 CORS 头——于是正确答案是让 Rust 后端去读,顺便把体积截断在上限内。
tags: ['开发', '对象存储', '前端', '架构']
---

Nebula 一直能预览图片和视频:点一下,用**预签名链接**塞进 `<img>` / `<video>` 就显示了。这次把预览扩展到**文本、代码、配置文件**——`.json`、`.log`、`.rs`、`.yaml`、`Dockerfile`…… 这些才是日常在云上翻得最多的东西。

看着是顺手的活:既然图片能用预签名链接加载,文本 `fetch()` 一下那个链接,拿到字符串塞进 `<pre>` 不就完了?

**并不能。** 这里藏着一个 CORS 的坑。

## `<img>` 能加载,`fetch()` 却会被拦

浏览器对待「加载媒体」和「读取内容」是两套规则:

- `<img src=…>` / `<video src=…>` 是 **no-cors** 的媒体加载——浏览器把字节交给渲染器**显示**,但脚本读不到,所以**不要求**目标返回 CORS 头。
- `fetch(url)` 要把响应体交给 **JavaScript 读**,这就触发同源策略:跨源响应必须带 `Access-Control-Allow-Origin`,否则整个请求被拦。

而对象存储的**预签名 GET 响应默认不带 CORS 头**——除非你在桶上专门配了 CORS 规则。也就是说:同一个预签名链接,`<img>` 显示得好好的,`fetch()` 读文本却会失败。指望每个用户去给每个桶配 CORS,不现实。

## 正确答案:让后端去读

Nebula 有 Rust 后端(Tauri),那就别在 webview 里 `fetch` 了——**让后端去读**。后端发的是普通 HTTP 请求,根本没有同源策略这回事。顺带还能解决第二个问题:**体积**。文本预览没必要把一个几百 MB 的日志整个拉下来,读**前 256 KB** 足矣。

于是新增一个 app-core 方法,流式读、累积到上限就停:

```rust
pub async fn read_preview(&self, account, path, max_bytes) -> Result<TextPreview> {
    let (_, mut stream) = self.provider(account)?.read_stream(path).await?;
    let mut buf = Vec::new();
    let mut truncated = false;
    while let Some(chunk) = stream.next().await {
        if preview::accumulate(&mut buf, &chunk?, max_bytes) {
            truncated = true;   // 缓冲已满、还有剩 → 截断
            break;              // 不再拉后续分块
        }
    }
    Ok(TextPreview { text: preview::decode(&buf), truncated })
}
```

复用已有的 `read_stream`(下载就用它),读到 256 KB 就 `break`——**后续分块根本不会从网络拉下来**。前端只多传一个上限、多收一个 `truncated` 标记,超限时在预览底部标一行「仅预览前 256 KB」。

## 顺手可测的两块纯逻辑

把「累积到上限」和「解码」抽成纯函数,就能不碰网络地测:

```rust
/// 累积一个分块,最多到 max 字节;返回是否发生截断。
pub(crate) fn accumulate(acc: &mut Vec<u8>, chunk: &[u8], max: usize) -> bool { … }
/// UTF-8 (lossy) 解码,非法字节替换为 U+FFFD,不会失败。
pub(crate) fn decode(bytes: &[u8]) -> String { String::from_utf8_lossy(bytes).into_owned() }
```

四条测试:上限内全收、跨上限的分块只收得下的部分、已满时再来仍报截断、非法 UTF-8 不 panic。`decode` 用 lossy 是有意的——预览要的是「尽量看清」,遇到二进制或半个多字节字符也别炸,替换字符照显。

## 什么归文本,什么归图片

判断预览类型的 `previewKind` 扩了一大张文本扩展名表(代码 / 配置 / 数据 / 纯文本),并对无扩展名的 `Dockerfile` / `Makefile` 之类按文件名兜底。有意思的是 `.svg`:它既是图片又是文本,这里让**图片优先**——SVG 能直接渲染成图,比看源码有用。

## 小结

「前端 fetch 一下」在纯 Web 里成立,在「桌面应用 + 对象存储」里不成立——云端 GET 的 CORS 缺席把这条路堵死了。绕一圈走后端反而是更短的路:一举绕开 CORS,又能在服务端把体积截在 256 KB,读到就停、不拉多余字节。有后端可用时,别什么都往 webview 里塞。
