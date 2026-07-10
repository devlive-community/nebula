---
title: 这个对象是不是归档了?跨云统一显示存储类型
date: 2026-07-10
author: Nebula Team
description: 标准、低频、归档、深度归档——每家云给存储层起的名字都不一样。这篇讲我们怎么把各家五花八门的存储类型统一显示在对象详情里,以及数据其实一直都在。
tags: ['开发', '对象存储', '存储类型', '架构']
---

对象存储都有**存储类型**(storage class / 存储层):热数据放**标准**层,读得少的放**低频**省钱,几乎不读的丢进**归档**层更便宜(代价是取回要先解冻)。用久了你一定会想知道:某个对象现在到底在哪一层?是不是已经归档、直接下载会不会失败?

这次先把这件事的**读**侧做了:对象详情里新增一行**存储类型**,一眼看清每个对象所在的层。

## 数据其实一直都在

有意思的是,实现这个功能几乎没碰网络请求——因为**列举对象的响应里本来就带存储类型**。S3 的 `ListObjectsV2` 每个对象都有 `<StorageClass>`,OSS / COS / OBS 的列举也一样。我们各家的 SDK 早就把它解析进了内部结构:

```rust
pub struct ObjectSummary {
    pub key: String,
    pub size: u64,
    pub etag: String,
    pub last_modified: String,
    pub storage_class: String,   // ← 一直在解析,只是没往上传
}
```

只是到了统一的 `Entry` 模型这一层,它被丢掉了——`Entry` 里没有这个字段。所以真正要做的,是给统一模型加一个可选字段,让适配层把已经解析好的值透传上来:

```rust
pub struct Entry {
    // ...
    pub storage_class: Option<String>,
}
```

七家适配层各加一句 `.with_storage_class(obj.storage_class)`,数据就通了。没有存储层概念的后端(或某对象没返回),字段留空即可——空字符串归一化成 `None`,不用每家单独判空。

## 坑:每家的名字都不一样

统一显示时撞上一个小麻烦:**同一个概念,每家云的字符串都不同**。

- AWS S3:`STANDARD` / `STANDARD_IA` / `GLACIER` / `DEEP_ARCHIVE` / `INTELLIGENT_TIERING`
- 阿里云 OSS:`Standard` / `IA` / `Archive` / `ColdArchive`
- 腾讯云 COS:`STANDARD` / `STANDARD_IA` / `ARCHIVE` / `DEEP_ARCHIVE`
- 华为云 OBS:`STANDARD` / `WARM` / `COLD`

直接把 `DEEP_ARCHIVE`、`WARM` 这种词甩给用户不友好。所以前端做一层**友好标签映射**:把常见值翻成"标准 / 低频访问 / 归档 / 深度归档 / 智能分层……",没收录的原样显示(不丢信息),字段为空时默认"标准"。

```ts
const storageLabel = (sc: string | null): string =>
  sc ? (STORAGE_LABELS[sc.toUpperCase()] ?? sc) : "标准";
```

## 接下来:从"看得见"到"管得了"

这一步是**读**——把存储类型显示出来。下一步是**写**:

- **转换存储类型**:把对象从标准转到低频 / 归档以省钱(通过带存储类型头的服务端复制实现)。
- **归档取回(restore)**:归档 / 深度归档的对象不能直接下载,得先发一个解冻请求等它取回。

这两个是写操作,要按各家的请求头和 `POST ?restore` 接口逐一实现,是更大的一块。先把"看得见"做扎实——毕竟你得先知道对象在哪一层,才谈得上管理它。
