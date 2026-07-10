---
title: 详情里的"类型"一直是猜的:读出并修正真实 Content-Type
date: 2026-07-11
author: Nebula Team
description: 对象详情里显示的"类型"其实是从文件名猜的,不是对象真实的 Content-Type。这篇讲怎么读出真实值、以及怎么改它——附一个"列举拿不到、HEAD 才有"的小坑。
tags: ['开发', '对象存储', '元数据', '架构']
---

一直有个小谎言藏在对象详情里:那一行"类型",显示的是**从文件名猜**的("图片""视频"),而不是对象**真实的 `Content-Type`**。多数时候猜得对,但 `Content-Type` 存错了正是常见的坑——比如一个 PDF 被传成了 `application/octet-stream`,浏览器打开就变成下载而不是预览。你想看真实值、更想**改**它,而这两件事以前都做不到。

这次补上:详情里显示真实的 `Content-Type`,旁边一个笔形按钮,点一下就能改。

## 坑一:列举拿不到 Content-Type,HEAD 才有

第一步是"读出真实值",但这里有个容易忽略的事实:**列举对象的响应里没有 `Content-Type`**。`ListObjects` 每个对象只给 key、大小、ETag、修改时间、存储类型——**没有** content-type。要拿它,得对单个对象发一次 `HEAD` 请求(`stat`),从响应头里读。

各家 SDK 的 `head_object` 本来就解析了它(`ObjectMeta.content_type`),只是到统一的 `Entry` 模型这层没往上带——和之前的存储类型一模一样的情况。所以:

1. `Entry` 加一个可选 `content_type` 字段。
2. 各适配层的 `stat` 把 `meta.content_type` 填进去。
3. 前端打开详情时,**先用列表数据即时显示,再异步 `stat` 补上真实 content-type**:

```ts
const openDetails = async (entry) => {
  setDetailsEntry(entry);                        // 立即显示(列表数据,type 先按名字猜)
  if (entry.kind !== "file") return;
  const full = await api.statPath(current, entry.path);   // HEAD 拿真实 content-type
  setDetailsEntry((cur) =>
    cur?.path === entry.path ? { ...cur, content_type: full.content_type } : cur);
};
```

这里刻意**合并**而不是整个替换——列表条目带着存储类型(`stat` 不返回),`stat` 结果带着 content-type(列表不返回),两边各有对方没有的字段,得拼起来。

## 坑二:改 Content-Type = 带 REPLACE 的自我复制

对象存储没有"改一下这个对象的 content-type"这种直接操作。改法和改存储类型如出一辙:**把对象复制到它自己**,但这次元数据指令用 **REPLACE**(而不是 COPY),并带上新的 `Content-Type` 头:

```rust
// s3-core:PUT 到同一个 key
content_type: Some(content_type),               // 新的 Content-Type
amz_headers: &[
    ("x-amz-copy-source", format!("/{bucket}/{key}")),
    ("x-amz-metadata-directive", "REPLACE"),    // 用请求里的元数据,而非复制源的
],
```

`COPY` 是"元数据原样保留"(改存储类型时用),`REPLACE` 是"用我这次请求带的元数据覆盖"(改 content-type 时用)——一字之差,语义相反。复用的还是各 SDK 已经验证过的 copy 签名路径,只是把指令从 COPY 换成 REPLACE、多签一个 `Content-Type` 头。

## 老规矩:trait 默认 + 能力位

`set_content_type` 是 trait 上的新方法,默认"不支持",支持的适配层覆盖并置 `metadata_ops` 能力位。七家全实现;以后新接入的云不实现也安全回退。

## 诚实的测试边界

和存储类型转换一样,改 content-type 是**写操作**,真实效果只能连真云验证,`cargo test` 里断言不了。我测的是 trait 默认在未实现时安全报错;签名正确性则靠**复用**已测的 copy REPLACE 路径获得信心——没新写签名逻辑。读取侧(`stat` 带出 content-type)倒是能测。

## 小结

详情里那行"类型"终于名副其实:读——靠 `HEAD`(列举拿不到,还得和列表数据合并);写——靠带 `REPLACE` 指令的自我复制(和改存储类型就差 COPY/REPLACE 一个词)。传错了 content-type 的对象,右键详情、点笔、改对,浏览器就能好好预览了。
