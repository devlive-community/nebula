---
title: 加桶级配置时,"不支持"比"假装支持"更重要
date: 2026-08-03
author: Nebula Team
description: CORS 规则和静态网站托管,骨架和生命周期规则完全一样。真正花心思的地方不是怎么实现,而是怎么诚实地标注谁不支持——R2 支持 CORS 但不支持网站托管,MinIO 社区版两个都不支持,这些"不对称"如果含糊过去,用户会在真实账号上撞见一个查不到文档的 403。
tags: ['开发', '对象存储', '架构', '签名']
---

Nebula 这次给 Bucket 加了 **CORS 规则** 和 **静态网站托管** 两项配置,连同已经做完的生命周期规则、版本控制,"存储治理"这条线基本收尾。骨架和之前完全一样(trait 默认方法 + 能力位 + 四份 SDK 实现),这篇不重复讲骨架,只讲这次真正花时间的地方:**如何诚实地标注"谁不支持"**。

## 骨架照抄,新东西只有数据形状

CORS 规则、网站托管的 trait 方法和 `Capabilities` 字段,和生命周期规则、版本控制长得一模一样:

```rust
async fn bucket_cors(&self, _bucket: &str) -> Result<Vec<CorsRule>> {
    Err(ProviderError::Unsupported("bucket cors".into()))
}
async fn bucket_website(&self, _bucket: &str) -> Result<Option<WebsiteConfig>> {
    Err(ProviderError::Unsupported("bucket website".into()))
}
```

`CorsRule`(来源 / 方法 / 请求头 / 暴露头 / 缓存时间的组合)、`WebsiteConfig`(首页文档 + 错误页文档)两个模型,四份 XML 读写(`s3-core` 覆盖 AWS,阿里云 OSS、华为云 OBS、腾讯云 COS 各一份),阿里云/华为云的签名白名单照例加两个新键(`cors`、`website`)——这些都是"把上次的模子重新刻一遍",没有新坑。

真正值得说的是一个容易被忽略的细节:CORS 规则内部会有多个 `<AllowedOrigin>`、多个 `<AllowedMethod>`,和版本历史那次踩过的"`Version`/`DeleteMarker` 交错导致 `duplicate field`"表面很像,但**不是同一个问题**。版本历史的坑在于两种**不同类型**的元素按时间交错(拿到手的顺序是 `Version, DeleteMarker, Version, ...`),quick_xml 的 `Vec<T>` 反序列化受不了"同名元素被别的元素打断"。CORS 规则里的 `<AllowedOrigin>` 全部扎堆出现,不会被 `<AllowedMethod>` 打断——这是**同一种类型的字段自己重复**,而不是"两种类型交错",所以直接用 `#[serde(rename = "AllowedOrigin")] Vec<String>` 就够,不需要再搬一遍事件流手动扫描那一套。同一个库的同一个限制,踩坑的条件是"哪些元素交错",不是"有没有重复元素"。

## 这次真正的工作量:填对能力矩阵,而不是简单打勾

七家云里,S3/OSS/OBS/COS 四家 CORS 和网站托管都支持,直接照抄生命周期规则的判断就行。真正需要认真调研、而不是顺手标 `true` 的是另外三家:

| Provider | CORS | 网站托管 | 原因 |
|---|---|---|---|
| Cloudflare R2 | ✅ 支持 | ❌ 不支持 | 官方文档明确写了标准 CORS API;网站托管这个概念官方压根没有,建议用 Worker 代替 |
| MinIO(社区版) | ❌ 不支持 | ❌ 不支持 | bucket 级 CORS API 只在商业版,社区版加这个 API 的 PR 在 2024 年被官方关闭;网站托管官方文档直接建议前面挂 nginx/caddy |
| 七牛云 Kodo | ❌ 不支持 | ❌ 不支持 | 比生命周期规则更彻底——生命周期好歹有管理台专有 API,CORS/网站托管连专有 API 都没有,只能控制台点 |

R2 和 MinIO 都走同一个 `s3-core` 门面,`s3-core` 里 `get_bucket_cors`/`get_bucket_website` 这些函数是**真实存在、能编译通过的**,如果我在 `provider-r2`/`provider-minio` 里偷懒把 `Capabilities` 全填 `true`,代码一样能跑、测试一样能过——直到用户在真实的 R2 或 MinIO 账号上点开"静态网站托管",收到一个查不到文档的报错。这种"能力位撒谎"式的 bug 是最难受的一类:本地看起来完全正常,只在生产环境的某个具体厂商上炸,而且炸的方式还不统一(R2 可能返回某个 API-not-found 错误,MinIO 可能直接连接被拒)。

解法是每个 provider 适配层**独立决定要不要覆盖某个 trait 方法**,和底层 SDK 技术上能不能调用是两回事:

```rust
// provider-r2:只覆盖 CORS,网站托管完全不写覆盖,吃 trait 默认的 Unsupported
async fn bucket_cors(&self, bucket: &str) -> Result<Vec<CorsRule>> {
    let rules = self.client.get_bucket_cors(bucket).await.map_err(map_err)?;
    Ok(rules.into_iter().map(cors_from_sdk).collect())
}
// 没有 bucket_website 的覆盖实现
```

```rust
fn capabilities(&self) -> Capabilities {
    Capabilities {
        // ...
        bucket_cors: true,
        // R2 没有静态网站托管的概念(官方建议用 Worker 代替),不覆盖对应 trait 方法。
        bucket_website: false,
    }
}
```

这和七牛云不覆盖生命周期规则、版本控制是同一个套路,这次只是第一次遇到"同一个厂商,两个能力不对称"的情况(CORS 支持、网站托管不支持)——也是当初把 `bucket_cors` 和 `bucket_website` 设计成两个独立的 `Capabilities` 字段而不是合并成一个 `bucket_extras` 的原因。

## 拆出去的第三块:跨区域复制

路线图里这一项原本写的是"CORS / 静态网站托管 / 跨区域复制"三合一,这次调研完把复制单独拆出去,不是漏做,是判断这三个东西根本不该放一起:

- CORS 和网站托管都是"改一份 bucket 配置,立刻生效,能自己发个请求验证效果"——和已经做完的生命周期规则、版本控制是同一类"读写一份 bucket 级 XML 配置",适合塞进一个轻量的桶设置弹窗。
- 跨区域复制需要用户**先在云控制台建好一个 IAM/RAM 角色**授权给复制服务,Nebula 一没法帮用户创建这个角色,二没法验证角色权限对不对;复制本身是异步持续同步,配错了不会立刻报错,只会几小时后发现数据没同步过去——这种"配置对不对要等很久才知道"的东西,不适合塞进一个"改完就该生效"的桶设置面板里,用户体验上会很割裂。
- 更关键的是,尽管名字很像("跨区域复制" vs 已有的"跨云迁移复制"),这俩功能架构上毫无关系:已有的跨云迁移复制是 Nebula 客户端一次性把对象从 A 账号下载再上传到 B 账号,而"跨区域复制"是云厂商服务端持续自动同步,不需要 Nebula 进程参与——没有一行代码可以复用。

真做的话应该先从"只读展示某个桶现有的复制规则"起步,而不是一上来就做创建流程——但这次判断优先级不够高,先记在路线图里当长尾项。

## 小结

这次没有新的技术难题,`quick_xml`、签名白名单、SDK/适配层分离全是抄现成的。唯一需要慢下来、认真查文档而不是顺手复制粘贴的地方,是给 R2 和 MinIO 填能力矩阵——"这家云到底支不支持"比"这个功能怎么实现"更容易被跳过,但恰恰是决定用户会不会在生产环境撞见一个诡异错误的关键一步。
