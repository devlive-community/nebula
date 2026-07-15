---
title: 那些没传完的大文件,正在悄悄给你账单加码
date: 2026-07-14
author: Nebula Team
description: 分片上传中断后,已上传的分片不会自己消失——它们成了「未完成的分片上传」,继续占着存储、继续计费,却在对象列表里看不见。这篇讲怎么把它们列出来、一键清掉。
tags: ['开发', '对象存储', '成本', '架构']
---

分片上传有个容易被忽略的副作用:**中断后,已经传上去的分片不会自动消失**。

大文件走分片上传,一片片传。如果中途失败、断网、或你主动取消——那些**已上传的分片**会留在服务端,组成一个「未完成的分片上传(incomplete multipart upload)」。它:

- **继续占存储、继续计费**——按已上传分片的大小算钱;
- 却**不出现在对象列表里**——`ListObjects` 看不到它,因为对象还没 complete;
- 会**越攒越多**——每次失败的大文件上传都可能留下一个。

这对 Nebula 用户尤其真实:Nebula 的**断点续传是故意不在失败时中止的**——失败保留分片,才能下次接着传。代价就是,那些最终没续传成功的,会变成残留分片。所以我们得给个「清理」的口子。

## 列出来:一个看不见的列表

残留分片藏在另一个 API 后面:**`ListMultipartUploads`**(`GET /?uploads`),它专门列这些「已初始化但未完成」的上传,给出每个的 `Key` / `UploadId` / 发起时间。

```rust
pub async fn list_multipart_uploads(&self, bucket: &str) -> Result<Vec<IncompleteUpload>> {
    // GET /{bucket}?uploads → 解析 <ListMultipartUploadsResult><Upload>…
}
```

七家云都支持这个 API(S3 系走 s3-core,阿里 / 华为 / 腾讯各自 SDK),响应结构基本一致:`<Upload>` 里是 `Key` + `UploadId` + `Initiated`。解析是纯 XML,直接单测钉死。清理则复用**早就有的** `AbortMultipartUpload`——列出来、逐个 abort 即可,不用新写中止逻辑。

## 清理:列 + 逐个中止

App 层就是「列一遍、挨个 abort」:

```rust
pub async fn clean_incomplete_uploads(&self, account, bucket) -> Result<u64> {
    let uploads = provider.list_incomplete_uploads(bucket).await?;
    for u in &uploads {
        provider.abort_multipart(&format!("{bucket}/{}", u.key), &u.upload_id).await?;
    }
    Ok(uploads.len() as u64)
}
```

界面上,桶右键多了一项「清理未完成上传」:点开先**列出来**让你看清有哪些、什么时候发起的(而不是盲删),再一键「全部清理」。没有残留时就告诉你「一切干净」。

## 顺带白捡的:跨区域也对

这个功能落地时,正好赶上前不久做的**按桶区域路由**——`list_multipart_uploads` 走的是和列对象同一条「按桶解析 endpoint / 区域」的路径,所以清理跨区域的桶也天然连对区域,一行额外代码都不用写。这就是把区域路由做进底层客户端、而不是散在各个方法里的好处:新功能自动继承。

## 小结

分片上传的残留是**看不见的持续成本**——列表里没有,账单里却有。补法不复杂:用 `ListMultipartUploads` 把它们捞出来(七家通用、纯 XML 可测),复用已有的 abort 逐个清掉,界面上先列后清、让你看明白再动手。断点续传「失败不中止」换来了续传能力,那就配一把「清理」的扫帚,把代价收回来。
