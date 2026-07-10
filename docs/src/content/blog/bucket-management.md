---
title: 能浏览 Bucket 却不能新建?一个藏在抽象层里的能力
date: 2026-07-11
author: Nebula Team
description: 补上新建 / 删除 Bucket——但有意思的是,这个功能几乎不用写代码:各家 SDK 早就实现好了,只是没从统一 trait 暴露出来。聊聊分层抽象怎么"藏"住能力。
tags: ['开发', '对象存储', 'Bucket', '架构']
---

Nebula 一直能在根层级浏览你的所有 Bucket,但有个尴尬:**你没法新建一个 Bucket**,也删不掉空桶。想开个新桶,还得跑去各家云的控制台。这次补上——右键 Bucket 删除、根层级「新建 Bucket」按钮。

但实现这个功能的过程,反而暴露了分层抽象的一个有意思的现象:**能力其实早就有了,只是被藏在了抽象层下面。**

## 打开一看,SDK 早写好了

Nebula 的存储访问分三层:各家云的原生 SDK(`s3-core` / `aliyun-oss` / …)→ 统一的 `StorageProvider` trait → App 层。加功能一般要从底层一路往上写。

可当我准备实现 `create_bucket` 时,翻开各家 SDK 一看——`create_bucket` 和 `delete_bucket` **全都已经写好了**:

```
s3-core:      pub async fn create_bucket / delete_bucket  ✓
aliyun-oss:   pub async fn create_bucket / delete_bucket  ✓
huawei-obs:   pub async fn create_bucket / delete_bucket  ✓
tencent-cos:  pub async fn create_bucket / delete_bucket  ✓
```

连 S3 那个最麻烦的、非 us-east-1 区域要带 `LocationConstraint` 的建桶细节都处理好了。这些方法当初写 SDK 时顺手就实现了,却因为统一的 `StorageProvider` trait **没有开这两个口子**,一直没被 App 用上。桶列表能看,是因为 trait 有 `list`;桶不能建,只是因为 trait 没有 `create_bucket`。

## 于是这功能几乎没有"新代码"

所以补这个功能,底层一行没动。真正做的只是把已有能力**接上统一抽象**:

1. `StorageProvider` trait 加两个方法(默认"不支持",老规矩),加一个 `bucket_ops` 能力位。
2. 七个适配层各加一句转发——`self.client.create_bucket(bucket)`。
3. App 层两个薄封装 + 两个 Tauri 命令。
4. 前端:根层级一个「新建 Bucket」按钮、Bucket 右键一个「删除 Bucket」。

底层签名逻辑(尤其建桶的区域处理)是现成且早已存在的,我只是把水管接通。

## 抽象的两面

这件事挺能说明分层抽象的两面性。

**好的一面**:因为底层 SDK 是按"完整的对象存储客户端"来写的、而不是"只写 App 现在用得到的",所以当 App 需要新能力时,底层往往已经准备好了——补功能变成了"接线"而不是"从头造"。这正是我们坚持"SDK 保持厂商原名、独立完整"的回报。

**要小心的一面**:统一 trait 是一道**闸门**——它决定了底层的哪些能力能被上层看见。SDK 有 `create_bucket`,但只要 trait 不开这个方法,整个 App 就当它不存在。抽象在统一接口的同时,也会不经意地"藏"住能力。所以隔一阵子对照一下"SDK 能做什么"和"trait 暴露了什么",常能捡到这种几乎白送的功能。

## 小结

新建 / 删除 Bucket 上线了,而它几乎没有新代码——各家 SDK 早把桶操作(含 S3 的区域建桶)实现好了,只是统一 trait 一直没开这个口子。补功能有时不是"写新代码",而是发现底层已有的能力、把它接到统一抽象上。顺手学到的:抽象层既暴露能力,也会藏住能力,值得时不时对一对账。
