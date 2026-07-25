---
title: 给对象贴标签:一个子资源,四套签名,一份 XML
date: 2026-07-13
author: Nebula Team
description: 对象标签是驱动生命周期、权限、成本分摊的元数据基石。这篇讲怎么把 GetObjectTagging / PutObjectTagging 接进七家云——关键在于它和归档取回共用同一套"子资源签名"机制。
tags: ['开发', '对象存储', '元数据', '架构']
---

对象标签(Object Tagging)是给对象挂一组 key-value,比如 `env=prod`、`team=data`。它不像 Content-Type 那样影响内容,却是很多云端能力的**基石**:生命周期规则按标签自动转档 / 过期、访问策略按标签授权、账单按标签分摊。这次给 Nebula 补上标签的读写。

用起来是详情抽屉里一个「标签」按钮,弹出一个键值编辑器:加载现有标签、增删行、整套保存(存空即清空)。

## 关键:标签就是又一个"子资源"

对象存储把「对某个对象的某种附属操作」表达成 URL 上的一个**子资源(sub-resource)**——`?restore`、`?uploads`、`?tagging` 都是。它们的共同点是:**这个 query 参数必须参与签名**,否则请求被拒。

Nebula 之前做归档取回(`?restore`)时,已经为三家自有签名 SDK(OSS / OBS / COS)铺好了「子资源签名」的路:

```rust
// 归档取回:POST /{key}?restore,restore 计入签名的 CanonicalizedResource
self.build_part_request(bucket, PartRequest {
    method: Method::POST,
    key,
    subresources: &[("restore", None)],   // ← 子资源
    content_type: Some("application/xml"),
    body: Some(body),
    ..
})
```

所以标签几乎是**免费**的——换一个子资源名、换个方法就成:

```rust
// 读标签:GET /{key}?tagging
subresources: &[("tagging", None)]        // method: GET
// 写标签:PUT /{key}?tagging + XML body
subresources: &[("tagging", None)]        // method: PUT
```

S3 系那四家(AWS / R2 / MinIO / 七牛,共用 `s3-core`)也一样,把 `tagging` 放进 SigV4 的 canonical query 即可。**一处子资源机制,复用到第二个功能**——这正是当初把它抽出来的回报。

## 一份 XML,四份实现

标签的报文是 S3 定义的一小段 XML,七家通用:

```xml
<Tagging><TagSet>
  <Tag><Key>env</Key><Value>prod</Value></Tag>
</TagSet></Tagging>
```

读时解析、写时拼装。拼装要**转义** `&<>"'`——标签值里一个裸 `&` 就能让整份 XML 失效:

```rust
fn build_tagging_xml(tags: &[(String, String)]) -> String { … xml_escape(k) … }
```

这段 parse / build / escape 在四个 SDK 里各有一份。有人会皱眉:重复。但每家 SDK 是**独立可发布**的 crate,刻意不互相依赖、也不共享私有工具——自包含胜过省几十行。这是这个项目一贯的取舍:SDK 的独立性是硬约束,DRY 是软偏好,冲突时前者优先。

## 可测:解析与拼装是纯函数

标签的 XML parse / build / escape 不碰网络,是**纯函数**,于是能扎实地测。`s3-core` 里加了四条:解析带值的 TagSet、解析空集、转义保留字符、以及 **build→parse 往返一致**:

```rust
let original = vec![("env".into(), "prod".into()), ("owner".into(), "a&b".into())];
let parsed = parse_tagging(&build_tagging_xml(&original)).unwrap();
assert_eq!(parsed, original);   // 拼进去能原样读回来,转义也不丢
```

真正发请求那一半(签名是否被云接受)只能连真账号,但报文这一半的正确性在 `cargo test` 里就锁死了。

## 老规矩:trait 默认 + 对每家云

`object_tags` / `set_object_tags` 是 trait 上的新方法,默认「不支持」,由 `object_tagging` 能力位控制,支持的适配层覆盖。七家都实现了。前端不额外查能力——和 Content-Type 一样,不支持的云在调用时自然报错,而不是预先灰掉按钮。

## 批量打标签

单个对象能打标签之后,批量就是自然的下一步:选中一批对象,给它们打上同一组标签(比如给一整批文件加 `project=x`,好让生命周期规则统一命中)。批量提供两种语义:

- **合并**(默认):读每个对象的现有标签,upsert 传入的键、保留其余——这样不会因为「顺手加个标签」把对象原有的其它标签清掉;
- **替换**:把每个对象的标签整体设成传入的这组。

实现上就是对选中项逐个调 `set_object_tags`(合并模式先 `object_tags` 读一遍再合),带进度与取消,单个失败记下继续。没有新的签名或报文——批量只是把已有的单对象操作套了个循环和一个「合并 vs 替换」的开关。

## 小结

对象标签看着是新功能,落到底层却是「又一个子资源」——和归档取回共用同一套签名路径,换个名字、换个方法而已。真正新写的只有那份七家通用的 XML 报文(以及它的转义),而它是纯函数,顺手就测了。一处机制铺好,第二个功能几乎白捡——这就是当初把子资源签名抽出来的复利。批量打标签更是白捡:单对象能力铺好,循环一下就成了。
