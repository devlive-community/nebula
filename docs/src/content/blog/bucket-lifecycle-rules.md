---
title: 加一个新的云端操作,大部分工作量都在"续上现成的骨架"
date: 2026-08-03
author: Nebula Team
description: 给 Bucket 加生命周期规则(到期删除 / 转归档),七个厂商听起来是七份工作。实际去做才发现:trait 默认方法 + 能力位的骨架早就在那儿了,真正要小心的只有阿里云和华为云签名里一张"子资源白名单"。
tags: ['开发', '对象存储', '架构', '签名']
---

Nebula 这次加了 **Bucket 生命周期规则**:按前缀匹配对象,多少天后转存储类型、多少天后直接删除——对象存储管理器少不了的存储治理能力。七家云,一开始以为要写七份实现,真正做完发现:骨架早就在,新工作量其实很集中。

## 骨架:trait 默认方法 + 能力位

`set_storage_class` / `restore` / `set_object_acl` 这些"厂商各异的操作"在 Nebula 里都是同一个形状:

```rust
async fn bucket_lifecycle(&self, _bucket: &str) -> Result<Vec<LifecycleRule>> {
    Err(ProviderError::Unsupported("bucket lifecycle".into()))
}
async fn set_bucket_lifecycle(&self, _bucket: &str, _rules: &[LifecycleRule]) -> Result<()> {
    Err(ProviderError::Unsupported("bucket lifecycle".into()))
}
```

trait 默认返回"不支持",支持的适配层覆盖,同时在 `Capabilities` 里加一个 `bucket_lifecycle: bool`。生命周期规则要做的,就是照着这个模子刻一遍——没有新发明任何抽象,新增的只是"这一种能力具体怎么实现"。

## 真正的工作量在四份 SDK,不是七份

Nebula 支持的七家云里,AWS / Cloudflare R2 / MinIO / 七牛云 Kodo 共用同一个 `s3-core`(它们都走 S3 协议 + SigV4,只是 endpoint 和 region 处理不同)。所以生命周期规则的 XML 构造与解析只需要写**四份**:`s3-core`(覆盖前三家)、阿里云 OSS、华为云 OBS、腾讯云 COS。七牛云虽然也走 `s3-core` 门面,但它真实的生命周期规则是管理台专有 API,不是 S3 兼容的 `?lifecycle`——这次先老实标成不支持(`capabilities().bucket_lifecycle = false`),和其它"某厂商暂未实现"的能力位一样处理,不硬凑。

四份实现的请求形状几乎一样:`GET/PUT/DELETE /?lifecycle`,手写 XML 构造 + `quick_xml` 反序列化——和已经写过的 `PutObjectTagging`/`RestoreObject`/`CreateBucketConfiguration` 是同一套技术栈,不是新东西:

```rust
fn build_lifecycle_xml(rules: &[LifecycleRule]) -> String {
    let mut body = String::from("<LifecycleConfiguration>");
    for r in rules {
        body.push_str("<Rule><ID>");
        body.push_str(&xml_escape(&r.id));
        body.push_str("</ID><Filter><Prefix>");
        body.push_str(&xml_escape(&r.prefix));
        body.push_str("</Prefix></Filter><Status>");
        body.push_str(if r.enabled { "Enabled" } else { "Disabled" });
        body.push_str("</Status>");
        for (days, class) in &r.transitions {
            body.push_str(&format!(
                "<Transition><Days>{days}</Days><StorageClass>{class}</StorageClass></Transition>"
            ));
        }
        // ...Expiration...
        body.push_str("</Rule>");
    }
    body.push_str("</LifecycleConfiguration>");
    body
}
```

空规则列表要发 `DELETE /?lifecycle` 而不是 `PUT` 一个空的 `<LifecycleConfiguration></LifecycleConfiguration>`——服务端不接受没有任何 `Rule` 的配置,这点在四家实现里是一致的。

## 真正要小心的坑:阿里云 / 华为云的子资源白名单

阿里云 OSS 和华为云 OBS 的签名(HMAC-SHA1,`CanonicalizedResource` 风格)只把**白名单里的**查询参数计入签名,不在名单里的参数会被过滤掉——但请求 URL 上这个参数还在,于是服务端拿完整 URL 算出的签名,和客户端只拿"过滤后的"URL 算出的签名对不上,直接 403 `SignatureDoesNotMatch`。这两个文件里其实早就留了话:

```rust
// crates/aliyun-oss/src/sign.rs(修改前)
/// 后续需要 `acl`、`lifecycle` 等再补。
const SUBRESOURCE_KEYS: &[&str] = &["uploads", "uploadId", "partNumber", "tagging", "restore"];
```

之前实现标签(`tagging`)和归档取回(`restore`)的时候,注释已经点名"以后要加 lifecycle"——这次终于用上,把 `"lifecycle"` 加进两家的白名单。**这是唯一一步"不做就会在真实账号上炸,但本地编译和单元测试完全看不出来"的改动**:签名逻辑本身没错,少一个白名单项而已,cargo test 测的是"XML 构造/解析对不对",测不出"请求会不会被服务端拒收"。

对照之下,腾讯云 COS 和 SigV4(AWS/R2/MinIO)家族的签名是"把实际发出的整个查询串都计入签名",没有白名单这一层,新增 `?lifecycle` 直接就能签对——四种签名方案里,只有阿里云和华为云这一种"手动维护参数白名单"的设计会有这个坑。

## 复用已有的"带子资源的签名请求"构造器

阿里云和华为云的分片上传(`uploads`/`uploadId`/`partNumber`)本来就有一个通用的私有构造器,专门处理"bucket 或对象 + 一组子资源参数"这类请求:

```rust
pub(crate) struct PartRequest<'a> {
    pub(crate) method: Method,
    pub(crate) key: &'a str,   // 空串即 bucket 级请求
    pub(crate) subresources: &'a [(&'a str, Option<&'a str>)],
    // ...
}
```

`key` 传空串,就是 bucket 级请求(生命周期规则不挂在某个对象上,是桶级配置)——不用新写一套签名拼接逻辑,只是把这个构造器的可见性从模块私有放宽到 `pub(crate)`,让 `bucket.rs` 也能调用 `multipart.rs` 里的这个函数。

## SDK 层不认识 App 的类型

Nebula 的四家"纯 SDK"crate(`s3-core`、`aliyun-oss`、`huawei-obs`、`tencent-cos`)是独立的、面向 crates.io 发布的库,不依赖 `nebula-provider`——这意味着它们不能直接用 App 层的 `LifecycleRule`。每个 SDK crate 都定义了自己的本地版本(字段完全一样),真正的转换发生在**适配层**(`provider-aliyun` 这类"只在 App 内部用、不发布"的胶水 crate):

```rust
fn rule_from_sdk(r: aliyun_oss::LifecycleRule) -> nebula_provider::LifecycleRule { ... }
fn rule_to_sdk(r: &nebula_provider::LifecycleRule) -> aliyun_oss::LifecycleRule { ... }
```

看着像重复定义了五次同一个结构体(四个 SDK + 一个 App 层),但这正是"可独立发布的 SDK"必须付出的代价——如果 `aliyun-oss` 直接引用 `nebula_provider::LifecycleRule`,它就不再是一个独立的 OSS SDK,而是绑死在这个 App 的私有抽象上了。

## 小结

这次最大的收获不是"学会了生命周期规则的 XML 格式",而是再一次验证:一个设计良好的适配层(trait 默认方法 + 能力位 + SDK/适配层分离)能把"接入第 N 种云端操作"的工作量压得很低——四份 XML 读写、七个 provider 各两行覆盖,真正需要多想一步的只有阿里云和华为云那张容易漏掉的子资源白名单。骨架搭得好,新功能才不会一次比一次难加。
