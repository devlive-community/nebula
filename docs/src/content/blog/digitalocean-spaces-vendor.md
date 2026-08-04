---
title: 第三家"S3 兼容"的云，第一次连 endpoint 都不按套路来
date: 2026-08-04
author: Nebula Team
description: B2 和 Wasabi 的 endpoint 都是 s3.{region}.xxx.com 这个形状，现成的 region 解析代码直接就能用。DigitalOcean Spaces 打破了这个巧合——endpoint 不带 s3. 前缀，得照抄 R2/MinIO 那套"专用构造函数"的老办法。但这次也有个意外的好消息：它的对象 ACL 限制恰好和 Nebula 现有实现完全对齐。
tags: ['开发', '对象存储', '架构']
---

接连三轮加了三家 S3 兼容的云:Backblaze B2、Wasabi、这次的 **DigitalOcean Spaces**。前两家的
endpoint 都是 `s3.<region>.xxx.com` 这个形状,和 AWS 一模一样,现成的 `parse_region` 函数不用
改一行就能解析对。这次不一样——第三家终于打破了这个巧合。

## endpoint 不按套路来,得回到 R2/MinIO 那套办法

DigitalOcean Spaces 的 endpoint 是 `{region}.digitaloceanspaces.com`(比如
`nyc3.digitaloceanspaces.com`)——**没有 `s3.` 前缀**。而 `crates/s3-core/src/client.rs` 里的
`parse_region` 函数是这么写的:

```rust
fn parse_region(endpoint: &str) -> String {
    endpoint
        .strip_prefix("s3.")
        .and_then(|rest| rest.split('.').next())
        .unwrap_or("")
        .to_string()
}
```

`strip_prefix("s3.")` 在 Spaces 的 endpoint 上直接失败,region 解析出来是空串。前两轮(B2、
Wasabi)因为 endpoint 形状凑巧和 AWS 一样,门面 crate 可以简化成纯粹的 `pub use` 重导出;这次
又要回到 R2、MinIO 那套"包一层 `new_client()` 构造函数"的老办法:

```rust
pub fn new_client(
    access_key: impl Into<String>,
    secret_key: impl Into<String>,
    endpoint: impl Into<String>,
) -> S3Client {
    let client = S3Client::new(access_key, secret_key, endpoint);
    let region = client.endpoint().split('.').next().unwrap_or("").to_string();
    client.with_region(region)
}
```

和 R2、MinIO 不一样的地方是:R2 的 region 固定写死成 `"auto"`,MinIO 默认写死成
`"us-east-1"`,这次的 region 是从 endpoint 动态取出来的(取第一个 `.` 之前的片段)——同一个
"包一层构造函数"的模式,这是第三种变体。复用 `S3Client::endpoint()` 这个已经做过协议前缀剥离、
去尾斜杠处理的方法,不用自己重新写一遍字符串处理逻辑。

这个教训其实很朴素:接了两家"巧合形状一样"的云之后,很容易把"endpoint 是 `s3.{region}.xxx`
这个形状"当成默认假设。第三家一来就打破了这个假设——每接一家新云,endpoint 格式本身也要重新确认,
不能因为前两次都一样就想当然。

## 意外的好消息:ACL 限制刚好和我们对齐

前两轮的能力矩阵调研大多是"发现某个功能不支持,或者支持得有各种限制,要老实标注"。这次有一项难得
的例外:DigitalOcean Spaces 的对象 ACL **只有两种 canned 值**——`private` 和 `public-read`。
这恰好是 Nebula 现在 `set_object_acl(path, public: bool)` 唯一会发送的两个值:

```rust
async fn set_object_acl(&self, path: &str, public: bool) -> Result<()> {
    let (bucket, key) = require_object(path)?;
    let acl = if public { "public-read" } else { "private" };
    self.client.set_object_acl(bucket, key, acl).await.map_err(map_err)
}
```

Wasabi 那轮虽然也支持这两个 canned 值,但额外有一层"公开访问效果可能被账号等级(免费/试用 vs
付费)限制"的复杂度,调用可能语法成功但实际不生效。Spaces 这边没有这层额外限制——它的能力边界
恰好卡在 Nebula 现有实现的边界上,不多不少。这算是"厂商限制反而和我们的设计天然吻合"的一次
正面案例,不是每次调研都能这么顺。

## 存储类型转换:这次连"转换"这个动作的对象都不对

B2 不支持存储类型转换是因为没查到对应的头;Wasabi 是因为只有一种存储类型,没有可转换的目标。
Spaces 这次的原因更进一步——它确实有两种"存储类型"(Standard 和 Cold Storage),但这是**建桶
时的选择**,是桶级属性,不是 S3 那种"对象随时可以用 `x-amz-storage-class` 转换"的对象级操作。
更关键的是,官方文档明确写了 `CopyObject` 在 Standard 和 Cold Storage 两种桶之间**不通**——而
Nebula 的 `set_storage_class` 正是靠"带新存储类型头的自我复制"实现的,这条实现路径在 Spaces 上
完全走不通,不是"支持但没测",是"这条路本身就断了"。三次踩到同一个能力位不支持,原因却一次比一次
不一样,这本身就说明"存储类型转换"这个概念在不同云厂商那里的建模方式差异有多大。

## 小结

三家 S3 兼容的云接下来,`s3-core` 复用的部分(签名、请求组装、XML 解析)完全没有增量工作量——
这正是这套架构设计要达到的效果。真正每次都要重新做的两件事是:**endpoint 格式要不要专用构造
函数**(这次学到:不能想当然),和**能力矩阵要不要逐项查文档**(这次学到:限制的具体原因值得
细看,有时候会有意外的好消息)。这两件事加起来才是"接一家新云"真实的工作量,协议兼容负责把这个
数字压到最低,但压不到零。
