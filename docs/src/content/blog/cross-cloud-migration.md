---
title: 「多云」到底有什么用?给 Nebula 加一条跨账号迁移
date: 2026-07-10
author: Nebula Team
description: 接了 7 家云,却只能一家一家单独用,数据搬家还得先下到本地再传上去——那"多云"就只是个空壳。这篇讲我们怎么加"A 云 → B 云"的一键迁移,以及一个绕不过的坑:服务端复制根本跨不了账号。
tags: ['开发', '多云', '对象存储', '架构']
---

Nebula 到今天接了 7 家对象存储(阿里云 OSS、华为云 OBS、腾讯云 COS、七牛云 Kodo、AWS S3、Cloudflare R2、MinIO)。但有个尴尬的事实:在加这个功能之前,"多云"其实是个**空壳**——你能在一个 App 里切换 7 家云,却只能一家一家单独用。想把阿里云某个桶里的文件搬到 AWS?先下载到本地,再手动上传上去。多云工具最该解决的"跨云搬家",反而得靠人肉。

所以这次加的是**跨账号 / 跨云迁移**:文件右键 → 迁移到其他账号 → 选目标账号 → 进目标桶 → 确认。A 云的对象直接落到 B 云,源对象保留。

看着是个小功能,但落地时撞上一个绕不过的坑。

## 坑:服务端复制根本跨不了账号

对象存储都有**服务端复制**:你发一个 PUT,带上 `x-amz-copy-source`(S3)或 `x-cos-copy-source`(COS)头,指向源对象,服务端就自己把数据复制过去,**一个字节都不经过你的客户端**。同桶、跨桶复制,用它又快又省流量。Nebula 同账号内的"复制/移动"走的正是这条路。

于是第一直觉是:跨账号迁移不也这样?错。

`x-amz-copy-source` 的前提是**同一套凭证签名**——请求用目标端的 AccessKey 签名,而服务端要求源对象也在**这套凭证的权限范围内**。跨账号(哪怕是同一家云的两个不同账号)、跨云(阿里云 → AWS),目标服务端**根本不认识**源那边的对象,也没有源的凭证去读它。服务端复制这条路,在跨账号场景下直接不存在。

结论很朴素:跨账号迁移**只能**"下载源 → 上传目标",数据必须过一趟客户端。没有捷径。

## 实现:同账号抄近路,跨账号走中转

所以 `copy_across` 分两条路——能抄近路就抄:

```rust
pub async fn copy_across_with_progress(
    &self,
    src_account: &str,
    src_path: &str,
    dst_account: &str,
    dst_path: &str,
    progress: ProgressFn<'_>,
) -> Result<()> {
    // 同账号:服务端复制,不下载数据。
    if src_account == dst_account {
        let provider = self.provider(src_account)?;
        provider.copy(src_path, dst_path).await?;
        let total = provider.stat(dst_path).await.map(|e| e.size).unwrap_or(0);
        progress(total, total);
        return Ok(());
    }
    // 跨账号 / 跨云:服务端复制无能为力,下载源 → 上传目标。
    let src = self.provider(src_account)?;
    let dst = self.provider(dst_account)?;
    let data = src.read(src_path).await?;
    dst.write_with_progress(dst_path, data, None, progress).await?;
    Ok(())
}
```

第一直觉可能是"这功能就是跨云的,同账号分支多余"。但用户完全可能不小心把源和目标选成同一个账号——这时白白下载再上传一遍就很蠢。留着这个近路,顺手而已。

## 真正的回报:那段代码里没有一个 `if vendor ==`

这个功能最舒服的地方,是上面那段跨账号分支里**没有任何厂商判断**。它不关心 `src` 是阿里云、`dst` 是 AWS 还是 R2,它对着的只是两个 `Arc<dyn StorageProvider>`:

```rust
let data = src.read(src_path).await?;      // 源怎么下载,是源那家 SDK 的事
dst.write_with_progress(...).await?;        // 目标怎么上传,是目标那家 SDK 的事
```

`read` 内部是 OSS 的 V2 签名还是 AWS 的 SigV4,`write` 内部要不要分片、怎么分片,全被各自的 provider 适配层吃掉了。迁移逻辑站在 `StorageProvider` 这层统一抽象之上,于是 **7 × 7 = 49 种"从某云搬到某云"的组合,一行都不用单独写**,天然全覆盖。以后再接第 8 家云,迁移功能自动多出 15 种新组合(旧 7 家 ↔ 新 1 家,双向)——不需要动这里一行代码。

这就是三层架构(`cloud-core → SDK → provider 适配层 → app`)+ 统一 trait 的回报:**把"每家云不一样"的复杂度,死死摁在适配层里**,让上面的业务逻辑活在一个干净的、只有 `read`/`write`/`list` 的世界里。

## 还没做完的:内存中转

老实说,v1 的跨账号迁移是把整个对象**整块读进内存**再上传的(`src.read()` 返回完整 `Bytes`)——这和 Nebula 现有的上传路径一致(上传也是先把本地文件读进内存)。搬几百 MB 没问题,搬几个 GB 就会吃内存。

真正的解法是**边下边传的流式中转**:源的分块流直接喂给目标的分片上传,内存里只留一个滑动窗口。但那需要把 provider 的 `write` 从"吃 `Bytes`"改成"吃一个 `Stream`",是一次更大的手术。留给下一篇。

先让"多云"这两个字,从一个下拉框里的摆设,变成真能把数据在云之间搬来搬去的东西。
