---
title: 文件传完了,你怎么知道它没坏?用 ETag 做完整性校验
date: 2026-07-10
author: Nebula Team
description: 下载、迁移都做完了,但字节到底有没有在半路损坏?这篇讲我们怎么用对象存储自带的 ETag 做一次不花钱、对每家云都通用的完整性校验,以及分片上传埋的那个坑。
tags: ['开发', '对象存储', '完整性', '架构']
---

Nebula 已经能流式下载、跨云迁移、并发分片上传。但这些功能都默默假设了一件事:**传过去的字节和源头一模一样**。真是这样吗?网络抖动、磁盘写坏、代理截断——任何一环出问题,你拿到的都是一个"看起来正常、其实缺了几个字节"的文件。而对象存储不会主动告诉你这件事。

所以这次加的是**内容完整性校验**:右键任意文件 → 校验完整性 → 立刻知道它和远端是否一致。关键是,这个功能**不需要额外存任何校验信息**,也**不需要为每家云单独写代码**。原因藏在一个你天天见、却很少细看的字段里:ETag。

## ETag 其实就是 MD5

列目录、`stat` 一个对象,响应里都会带一个 `ETag`。多数人只把它当成"缓存用的版本标识",但对**整对象上传**(一次 PUT 传完、没有分片)的文件,S3 及所有兼容实现的约定是:

> ETag = 对象内容的 MD5,十六进制,通常带一对引号。

也就是说,远端已经**免费**替我们存了一份内容指纹。校验一个文件是否完整,根本不用预先算好 checksum 存到某个数据库里——把内容下下来,本地算个 MD5,和 ETag 比一下就行:

```rust
pub fn verify_bytes(data: &[u8], etag: Option<&str>) -> Integrity {
    let Some(etag) = etag else {
        return Integrity::Unverifiable { reason: "对象未返回 ETag".into() };
    };
    let Some(expected) = etag_as_md5(etag) else {
        return Integrity::Unverifiable { reason: "分片对象的 ETag 非整体 MD5".into() };
    };
    let actual = md5_hex(data);
    if actual == expected {
        Integrity::Verified
    } else {
        Integrity::Mismatch { expected, actual }
    }
}
```

三种结果:**通过**、**不一致(文件可能损坏)**、**无法校验**。第三种不是偷懒——它对应一个绕不过的坑。

## 坑:分片上传的 ETag 不是 MD5

大文件走的是**分片上传**(multipart)。这种对象的 ETag **不是**整个文件的 MD5,而是"每个分片 MD5 再拼起来取一次 MD5",后面还跟一个 `-N` 表示分片数,长这样:

```
"d41d8cd98f00b204e9800998ecf8427e-12"
```

你要是老老实实对整个文件算 MD5 去比,**永远对不上**——不是文件坏了,是算法根本不同。更麻烦的是,分片 ETag 依赖上传时的**分片大小**,而这个值各家客户端、各家 SDK 都不一样,没法在客户端稳定复现。

所以判定逻辑很简单但必须严格:只有 ETag 去掉引号后是**纯 32 位十六进制、且没有 `-` 后缀**,才当它是可信的整体 MD5;否则一律返回 `Unverifiable`,并如实告诉用户"这是分片对象,MD5 校验不适用",而不是谎报一个"失败"。诚实的"我不知道"比错误的"它坏了"有用得多。

```rust
fn etag_as_md5(etag: &str) -> Option<String> {
    let e = etag.trim().trim_matches('"');
    (e.len() == 32 && e.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| e.to_ascii_lowercase())
}
```

## 为什么这个功能对"每一家云"自动生效

Nebula 的一条设计原则是:新增一家厂商,不应该逼着你回头去改一堆已有功能。完整性校验天然符合这一点——因为它**只依赖 ETag 语义,不碰任何厂商的签名、endpoint 或私有字段**。

无论是国内的阿里云 OSS / 腾讯云 COS / 华为云 OBS,国际的 AWS S3 / Cloudflare R2,还是自建的 MinIO,它们的 `stat` 都会把 ETag 填进统一的 `Entry.etag`。校验逻辑活在 App 层,拿到的是这个统一字段,**根本不知道背后是哪家云**:

```rust
pub async fn verify(&self, account: &str, path: &str) -> Result<Integrity> {
    let provider = self.provider(account)?;
    let meta = provider.stat(path).await?;   // 任意厂商,统一 Entry
    let data = provider.read(path).await?;
    Ok(integrity::verify_bytes(&data, meta.etag.as_deref()))
}
```

结果是:**以后新接入的任意一家云,不写一行校验相关的代码,就自动拥有完整性校验**。这正是"统一抽象 + 只依赖公共语义"想要的复利——功能一次写好,厂商越加越多,它覆盖得越广。

## 小结

完整性校验没有引入新的存储、新的协议、新的每厂商适配,只是把对象存储早就给你的 ETag 用起来了:

- 整对象文件:本地 MD5 对比 ETag,一键判断是否损坏;
- 分片对象:识别 `-N` 形态,如实标为"不可校验",绝不谎报;
- 逻辑只吃统一的 `Entry.etag`,对现有与未来的每一家云一视同仁。

下次下载或迁移完一个重要文件,右键校验一下——花一次下载的钱,买一个"它确实完好"的确定性。
