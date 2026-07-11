---
title: 分享链接的反面:让别人无需密钥往你桶里传文件
date: 2026-07-11
author: Nebula Team
description: 预签名下载链接让别人无需密钥下载你的对象。这篇做它的反面——预签名上传链接,别人凭一个链接就能 PUT 上传,以及把签名逻辑从"只签 GET"推广到"签任意方法"。
tags: ['开发', '对象存储', '预签名', '架构']
---

Nebula 一直支持**预签名下载链接**:给一个对象生成带签名的临时 URL,别人无需你的密钥就能下载,过期自动失效。这次补上它的**反面**——**预签名上传链接**:生成一个 PUT 链接,别人凭它就能往你桶里的某个路径**上传**一个文件,同样无需密钥、同样会过期。

用起来就一句 `curl`:

```bash
curl -X PUT --upload-file ./report.pdf "https://…?X-Amz-Signature=…"
```

收集别人的文件、让协作者上传产物,都不用再发临时凭证了。

## 关键:签名逻辑本来就只差一个"方法"

预签名 URL 的本质,是把请求的关键信息(方法、路径、过期时间…)用你的密钥签个名,拼进 URL 的 query 里。下载链接签的是 `GET`,上传链接签的是 `PUT`——**除了 HTTP 方法这一处,其余完全一样**。

之前的实现却把 `GET` 写死了。比如共享的 SigV4 预签名函数:

```rust
// 之前:方法写死
pub fn presigned_get_url(params, canonical_uri, expires_in, now) -> String {
    // ... canonical_request("GET", ...) ...
}
```

所以这次的第一步不是"写上传",而是把它**推广成"签任意方法"**:

```rust
pub fn presigned_url(params, method: &str, canonical_uri, expires_in, now) -> String { … }
// 旧的 presigned_get_url 变成一行包装:
pub fn presigned_get_url(p, uri, exp, now) -> String { presigned_url(p, "GET", uri, exp, now) }
```

各家自有签名 SDK(OSS / OBS / COS)也一样——它们的预签名 StringToSign 里本来就有 HTTP 方法那一格,原先填死 `GET`,现在把它变成参数,再各加一个 `presign_put` 调用 `PUT`(COS 的方法会自动转小写,`get`/`put` 皆可)。改动小、风险低,因为签名的核心一行没动,只是把常量变成了参数。

## 可测:预签名 URL 是确定性的

预签名 URL 的一个好处是**纯本地签名、不发网络请求**,所以它**可以确定性地测**——给定固定的密钥、时间、路径,签出来的 URL 是定死的。各 SDK 早就有 GET 预签名的测试,这次顺手加了一条:同样的对象,PUT 和 GET 签出来的 URL **必然不同**(因为 canonical request 里的方法不同),以此确认方法确实参与了签名:

```rust
let get = c.build_presigned_url("GET", "b", "k", 3600, at(t));
let put = c.build_presigned_url("PUT", "b", "k", 3600, at(t));
assert_ne!(get, put);   // 方法进了签名 → URL 不同
```

不像那些写操作只能连真云验证,预签名这块的正确性能在 `cargo test` 里扎扎实实测到。

## 老规矩:trait 默认 + 对每家云

`presign_put` 是 trait 上的新方法,默认"不支持",支持预签名的适配层覆盖。七家全实现了。前端:文件右键新增「上传链接」,生成后在弹窗里展示 URL 并附一行 `curl` 提示。

## 小结

上传链接是分享链接的镜像:同一套签名,方法从 `GET` 换成 `PUT`。真正做的是把"只会签 GET"推广成"签任意方法",然后各加一个 `presign_put`——改动小、可测、对每家云一致。收文件不用再发密钥了。
