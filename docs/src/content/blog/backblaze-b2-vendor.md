---
title: 接入第八家云,真正的工作量在查文档,不在写代码
date: 2026-08-04
author: Nebula Team
description: Backblaze B2 是目前接入最省事的一家云——endpoint 格式和 AWS 一样,现有代码不用改一行就能解析对。但"省事"仅限于写代码这一步：真正花时间的是逐项核实它的标签、ACL、版本控制到底支不支持，而不是把 R2 或 MinIO 的能力位抄一份改个名字。
tags: ['开发', '对象存储', '架构']
---

Nebula 这次接入第八家云:**Backblaze B2**。七家云里已经有三家(AWS S3、Cloudflare R2、MinIO)
共用同一个 `s3-core` 门面,新增一家纯 S3 兼容的云厂商,理论上应该是最省事的一类工作——这次确实
是,但省事的地方和一开始猜的不太一样。

## endpoint 长得像 AWS,现成代码不用改一行

R2 和 MinIO 接入的时候,都需要一个专门的 `new_client()` 构造函数:R2 的 SigV4 region 得强制写死
成 `"auto"`,MinIO 得给一个默认 region(`us-east-1`)并处理 `http://` 明文连接的场景。这是因为
它们的 endpoint 格式不满足 `s3-core` 现有的 region 自动解析规则:

```rust
/// 从 `s3.{region}.qiniucs.com` 解析 region;解析不出返回空串。
fn parse_region(endpoint: &str) -> String {
    endpoint
        .strip_prefix("s3.")
        .and_then(|rest| rest.split('.').next())
        .unwrap_or("")
        .to_string()
}
```

B2 的 S3 兼容 endpoint 是 `s3.<region>.backblazeb2.com`(比如
`s3.us-west-002.backblazeb2.com`)——和 AWS 的 `s3.{region}.amazonaws.com` 是同一种形状,这个
函数直接就能解析对,不用改一行。连门面 crate 都不需要写额外的构造函数,直接照抄 `aws-s3` 那种
"纯重导出"的门面就够了:

```rust
pub use s3_core::{bucket, client, error, multipart, object};
pub use s3_core::{
    BucketSummary, CorsRule, LifecycleRule, ListEntry, ObjectMeta, ObjectSummary, ObjectVersion,
    Result, S3Client, S3Error, WebsiteConfig, MIN_PART_SIZE,
};
```

这一步确实是"接入新厂商里最省事的一次"。但这只是整个工作量里最小的一部分。

## 真正的工作量:逐项核实能力矩阵,不是继承

Nebula 给每个 provider 一个 `Capabilities` 结构体,记录这家云到底支持哪些高级功能(生命周期规则、
版本控制、CORS、静态网站托管、对象标签、对象级 ACL……)。新增一家 S3 兼容的云,最容易犯的懒是:
"反正都是 S3 兼容,把 R2 或者 MinIO 的 `Capabilities` 复制一份,改个厂商名就完事"。这次专门去查了
Backblaze 的官方文档,发现这个懒偷不得——B2 在好几项上和 R2、MinIO 都不一样:

| 能力 | AWS | R2 | MinIO | B2 |
|---|---|---|---|---|
| 对象标签 | ✅ | ✅ | ✅ | ❌ |
| 对象级 ACL 写入 | ✅ | ✅ | ✅ | ❌ |
| 静态网站托管 | ✅ | ❌ | ❌ | ❌ |
| 版本控制(开关) | ✅ | ✅ | ✅ | ❌(语义不同,见下） |
| CORS | ✅ | ✅ | ❌ | ✅ |

如果直接抄了 R2 的能力位,B2 账号上点"设置对象标签"或者"设为公开读"会在用户的真实账号上得到一个
莫名其妙的报错——本地完全测不出来,因为这几行代码语法上都是对的,只有对着真账号跑才会露馅。这正是
Nebula 这几轮"能力矩阵"专门去查官方文档、而不是凭经验写代码的原因:一个能力位标错,不是编译器能
帮你抓的错误。

## 版本控制那一项,查完发现"做不了半个功能"

最有意思的一项是版本控制。B2 的桶**默认永远开启版本**,没有"启用/暂停"这个开关——这和 S3/OSS/
OBS/COS 那种"默认关闭、用户可以选择开启"的模型完全不是一回事。Nebula 的 `versioning` 能力位把
"开关"和"读版本历史/恢复旧版本/删除某个版本"绑在一起当成一个整体,而 B2 只能提供后半段(版本历史
天然存在,能读能恢复),提供不了前半段(没有"开关"可以切换)。

选择是:要么硬做后半段、把前半段的"启用/暂停版本控制"按钮做成一个点了也没反应的摆设,要么老实标
`versioning: false`。选了后者——半个能力比没有能力更容易让用户困惑,尤其是这个按钮看起来能点、
点了却什么都不会发生。如果以后真有用户想看 B2 的版本历史,需要先把这个能力位拆成"开关"和"读历史"
两个独立的位,这是改 trait 设计的事,不是"接入一个新厂商"这种量级的改动该顺手做的。

## 小结

这次的教训不新鲜,但值得再说一遍:**"两家云都用 S3 协议"不等于"两家云支持一样的功能"**。协议兼容
只保证请求格式能被正确解析,不保证服务端真的实现了对应的业务逻辑。骨架搭得好(门面 + 适配层 + 能力
位),接入一家新云的工作量能压得很低——这次真的只改了几个文件、复制了几个模式——但压不掉的是"这家
云到底支持什么"这道题,答案永远得去查文档,不能靠"看起来都差不多"猜。
