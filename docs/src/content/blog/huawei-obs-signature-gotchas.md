---
title: 实战复盘:手写华为云 OBS SDK 时踩过的签名坑
date: 2026-07-08
author: Nebula Team
description: 华为云 OBS 的签名和阿里云 OSS 同宗(都是 S3 V2 风格),照着抄本该很快。但官方文档一个多余的换行、一个被打码的 SK,让"逐字节验证签名"这条铁律差点没法落地。这是我们怎么绕过去的复盘。
tags: ['开发', 'SDK', '华为云', '签名']
---

> 配套阅读:[手把手:为 Nebula 新增一个云厂商 SDK](/blog/build-a-provider-sdk) 是通用流程,本文是给华为云 OBS 落地时的实战复盘,专讲那些"照文档抄会出错"的地方。

Nebula 每家云都是从零手写的独立 Rust SDK,不用任何聚合库。轮到华为云 OBS 时,我们本以为很轻松——它的签名和已经打样过的阿里云 OSS 几乎同构:都是 `base64(HMAC-SHA1(SK, StringToSign))` 的 V2 风格 Header 签名,StringToSign 结构也一样:

```text
VERB + "\n" + Content-MD5 + "\n" + Content-Type + "\n" + Date + "\n"
     + CanonicalizedHeaders + CanonicalizedResource
```

差异看起来只有三处表面文章:canonical 头前缀 `x-obs-`(OSS 是 `x-oss-`)、授权词 `OBS`(OSS 是 `OSS`)、endpoint 域名 `obs.{region}.myhuaweicloud.com`。

结果真正花时间的,是两个文档层面的坑。

## 坑一:官方文档的 StringToSign 多了一个换行

华为官方《在头域中携带签名》把签名串写成了这样:

```text
... + Date + "\n" + CanonicalizedHeaders + "\n" + CanonicalizedResource
```

注意 `CanonicalizedHeaders` 和 `CanonicalizedResource` 之间那个 `"\n"`。如果照抄,当请求带 `x-obs-*` 头时,你拼出来的串会比服务端期望的多一个换行,签名直接对不上,报 `SignatureDoesNotMatch`。

问题是文档自己都不自洽——同一份文档的不同小节,这个换行时有时无。**这种时候不要信文档,信官方 SDK 源码。** 我们翻了华为官方的 OBS Python SDK,它的 canonical 串是这么拼的:

- 每个 `x-obs-` 头单独成行,**行尾自带 `\n`**;
- 头块拼完,**直接**接 CanonicalizedResource,中间**没有**额外换行。

也就是和 AWS S3 V2 / 阿里云 OSS 完全一致。文档那个 `"\n"` 是排版时的笔误。定稿的实现里,我们把这个结论直接写进了注释,免得下一个人再踩:

```rust
/// ⚠ 华为官方文档把 StringToSign 写成 `...CanonicalizedHeaders + "\n" + CanonicalizedResource`
/// (头块与资源间多一个换行),但官方 OBS Python/Java SDK 里每个 x-obs- 头行自带行尾 `\n`、
/// 头块与资源间无额外换行(与 OSS/S3 V2 一致)。这里以 SDK 行为为准。
pub fn string_to_sign(/* ... */) -> String {
    format!("{method}\n{md5}\n{ctype}\n{date}\n{canonical_headers}{canonical_resource}")
}
```

## 坑二:官方示例把 SK 打码了,怎么"逐字节"验证签名

我们的 SDK 开发手册有一条铁律:**签名步骤必须用官方文档的示例向量写一个单测,验证签名逐字节相等**。这是唯一能离线证明签名正确的手段,不许跳过。

华为文档确实给了个建桶签名示例:

```text
Authorization: OBS UDSIAMSTUBTEST000254:ydH8ffpcbS6YpeOMcEZfn0wE90c=
```

但——SK 被"为了安全"打码了。没有 SK,就没法复现 `ydH8ff...` 这个签名。铁律眼看要落空。

我们的破法是**拆成两个官方向量,各证一半**:

**① 用华为示例证"格式"。** 签名 = `base64(HMAC-SHA1(SK, StringToSign))`,SK 缺失只是让最后一步 HMAC 算不出,但 **StringToSign 这个字符串本身是可以逐字节断言的**。华为示例给了完整请求,我们就断言拼出来的串一字不差:

```rust
let sts = string_to_sign("PUT", "", "", "Fri, 06 Jul 2018 03:45:51 GMT",
    &canonicalized_obs_headers([("x-obs-acl", "private"),
                                ("x-obs-storage-class", "STANDARD")]),
    "/newbucketname2/");
assert_eq!(sts,
    "PUT\n\n\nFri, 06 Jul 2018 03:45:51 GMT\n\
     x-obs-acl:private\nx-obs-storage-class:STANDARD\n/newbucketname2/");
```

这证明了 OBS 特有的格式(`x-obs-` 头怎么规整、资源怎么拼、坑一那个换行到底有没有)。

**② 用 AWS S3 V2 公开向量证"字节"。** OBS 的签名算法和 AWS S3 Signature V2 是**同一个**,而 AWS 官方《Signing REST Requests》的示例 **SK 是公开的**。用它就能逐字节验证 `HMAC-SHA1 + base64` 这套加密原语拼装无误:

```rust
const SK: &str = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY";
let sts = string_to_sign("GET", "", "", "Tue, 27 Mar 2007 19:36:42 +0000",
    "", "/johnsmith/photos/puppy.jpg");
assert_eq!(signature(SK, &sts), "bWq2s1WEIj+Ydj0vQ697zp+IXMU=");
```

两个向量合起来:①锁死了 OBS 特有的串结构,②锁死了签名字节。铁律以另一种形式落地了,而且离线可跑、CI 每次都验。

## 三个和 OSS 不一样、抄不得的地方

签名搞定后,数据面基本能照搬 OSS。但有三处行为差异,是抄代码时会被坑的:

| 点 | 阿里云 OSS | 华为云 OBS |
|----|-----------|-----------|
| 列举 bucket | GET Service 带 marker 分页 | **一次返回全部,不分页** |
| 建桶(非默认区域) | 直接 PUT 即可 | 需在请求体带 `<CreateBucketConfiguration><Location>` |
| 列举对象响应 | 带 `ExtranetEndpoint` 等字段 | 无,字段更精简 |

第一点尤其隐蔽:OBS 的 `GET Service` 一次性把所有桶都返回,没有 `IsTruncated`/`NextMarker`。为了让上层适配层的接口和 OSS 保持一致(都是"桶的流"),我们把这一次性结果**包成一个单页流**——取过一次后游标即 `None`,流自然终止:

```rust
pub fn list_buckets(&self) -> impl Stream<Item = Result<BucketSummary>> + '_ {
    paginate(false, move |fetched: bool| async move {
        if fetched { return Ok(Page { items: vec![], next: None }); }
        Ok(Page { items: self.list_buckets_once().await?, next: Some(true) })
    })
}
```

建桶那一点也值得记一句:OBS 非默认区域必须在 body 里声明 Location,而 region 可以直接从 endpoint(`obs.{region}.myhuaweicloud.com`)解析出来。好在 V2 签名不对 body 取哈希,所以带不带这个 body 都不影响签名计算,加起来很安全。

## 收尾

整个 SDK 严格按增量顺序走:骨架 → 签名(官方向量单测)→ client → error → 对象操作 → 列举分页 → 桶管理 → 分片上传 → 真账号冒烟。每一步都 `fmt` / `clippy -D warnings` / `test` 三件套全绿才提交。离线单测把能离线验证的都覆盖了,最后用真实华为云账号跑通 `list_buckets → put → head → list → get → 分片上传` 全链路。

适配层写完往 `app-core` 一注册,华为云 OBS 就和阿里云 OSS 走同一套浏览/上传/下载/分享逻辑,**App 界面一行没改**。

两条经验留给下一个接手的人:

1. **签名对不上时,先怀疑文档,去读官方 SDK 源码**——文档会有排版误差,SDK 不会骗你。
2. **官方向量不完整时,别放弃"逐字节验证"**——拆成"证格式"和"证字节"两个可复现的向量,一样能锁死正确性。
