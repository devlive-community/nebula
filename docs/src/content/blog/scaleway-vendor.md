---
title: 第四家 S3 兼容云，终于遇到一个"全都支持"的
date: 2026-08-04
author: Nebula Team
description: Backblaze B2、Wasabi、DigitalOcean Spaces 三家 S3 兼容云，每家都在能力矩阵上缺点什么——标签、ACL、CORS、网站托管、存储类型转换，总有那么几项不支持或有额外限制。这次的 Scaleway Object Storage 是个例外：官方文档逐项查下来，Nebula 需要的能力它全都支持。
tags: ['开发', '对象存储', '架构']
---

前三轮 S3 兼容云接下来,能力矩阵调研的规律几乎是"总有缺口":B2 不支持标签、ACL、CORS、网站托管;
Wasabi 不支持 CORS 配置接口和网站托管;DigitalOcean Spaces 的存储类型转换这条路直接走不通。这次
接入 **Scaleway Object Storage**(法国云厂商,`fr-par`/`nl-ams`/`pl-waw`/`it-mil` 四个欧洲区域),
逐项核对官方文档后发现:标签、生命周期规则、版本控制、CORS、静态网站托管、存储类型转换——Nebula
现有能力位覆盖的这几项,它**全部支持**。四家 S3 兼容云接下来,终于遇到一个不用在文档里写"官方明确
不支持"的。

## endpoint 形状:又是巧合的一次

Scaleway 的 endpoint 是 `s3.{region}.scw.cloud`(比如 `s3.fr-par.scw.cloud`),和 AWS 的
`s3.{region}.amazonaws.com` 同形状。`crates/s3-core/src/client.rs` 里现成的 `parse_region`:

```rust
fn parse_region(endpoint: &str) -> String {
    endpoint
        .strip_prefix("s3.")
        .and_then(|rest| rest.split('.').next())
        .unwrap_or("")
        .to_string()
}
```

`strip_prefix("s3.")` 剥掉前缀得到 `fr-par.scw.cloud`,取第一个 `.` 之前的片段得到 `fr-par`——
不用改一行代码。门面 crate `scaleway-object-storage` 因此和 `aws-s3`/`backblaze-b2`/`wasabi-s3`
一样,是纯粹的 `pub use` 重导出,不需要 DigitalOcean Spaces 那种专用构造函数。四家里已经有三家是
这个形状,只有 Spaces 是例外——但这不代表以后可以跳过验证,只是这次运气好。

## 存储类型:三档,和 AWS 同名但不是子集意义上的兼容

Scaleway 支持 `STANDARD`、`ONEZONE_IA`、`GLACIER` 三档存储类型,命名直接照抄 AWS 的
`x-amz-storage-class` 语义,`storage_class_ops` 能力位可以直接打开,复用现有的"自我复制换存储
类型"实现路径,不用像 DigitalOcean Spaces 那样另外碰壁。唯一的限制是 `GLACIER` 只在 `fr-par` 和
`nl-ams` 两个区域可用——但 Nebula 拿不到"当前 bucket 所在区域是否支持该存储类型"这层信息,不在
客户端做额外校验,交给服务端在真正设置时报错,这和已有的"不做无法验证的提前拦截"原则一致。

## ACL:协议层支持,但官方语义比 AWS 简化

官方文档描述的"对象可见性"模型只有 `public`/`private` 两态,没有强调 `public-read-write`、
`authenticated-read` 等更细粒度的 canned ACL——但协议层接受标准的 `x-amz-acl` 头,和
Nebula 现有 `set_object_acl(path, public: bool)` 只发送 `public-read`/`private` 两个值的
二态模型天然对齐,和 DigitalOcean Spaces 那次"限制刚好卡在我们边界上"是同一类幸运,不是巧合
第一次出现。

## 小结

四家 S3 兼容云接下来,`s3-core` 复用的部分完全没有增量工作量,这次连"能力矩阵要不要打折扣"这道
题都省了——官方文档逐项核实下来,该支持的全支持。但省的是这次调研的结论,不是调研这个动作本身:
每接一家新云,endpoint 格式和能力矩阵还是要重新查一遍文档,不能因为形状像 AWS、或者前几家大多
有缺口,就对下一家有预设立场。这次的"全都支持"和上次 Spaces 的"ACL 刚好对齐"一样,都是核实完
才知道的结果,不是能提前预判的规律。
