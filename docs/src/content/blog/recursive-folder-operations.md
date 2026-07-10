---
title: 对象存储里没有文件夹,那"下载整个文件夹"怎么做?
date: 2026-07-10
author: Nebula Team
description: 对象存储只有扁平的 key,所谓"文件夹"不过是共享前缀。这篇讲我们怎么在这套扁平模型上做出整目录的递归下载 / 迁移 / 删除,以及一个复用已有能力的分页遍历。
tags: ['开发', '对象存储', '文件夹', '架构']
---

Nebula 已经能下载、迁移、删除单个对象。但真实用起来,你很少只想动一个文件——你想把 `photos/2026/` 整个搬到另一朵云,或者把一个废弃前缀整个删掉。这次补上的就是**文件夹级递归操作**:右键任意目录 → 下载 / 迁移 / 删除整个文件夹。

听起来是把单文件操作套个循环。但"文件夹"这个概念,在对象存储里其实**根本不存在**。

## 对象存储没有文件夹,只有前缀

本地文件系统里,目录是实打实的一种 inode。但对象存储(S3 及所有兼容实现)是一个**扁平的 key-value 表**:`photos/2026/cat.jpg` 不是"photos 目录下 2026 目录下的 cat.jpg",它就是一个**完整的 key**,那两个 `/` 只是普通字符。

所谓"文件夹",是列举接口用 `delimiter=/` 现算出来的**公共前缀**。你请求 `prefix=photos/2026/&delimiter=/`,服务端把这个前缀下的对象归拢,遇到下一级 `/` 就折叠成一个"公共前缀"返回——那就是 GUI 里看到的"子文件夹"。它没有实体,删掉前缀下所有对象,这个"文件夹"就自动消失了。

这意味着文件夹级操作的本质是:**枚举某个前缀下的所有对象,再逐个处理**。而枚举本身要分页、要能下钻,还得防着某个前缀下有几十万对象把内存和时间拖垮。

## 复用已经打磨好的分页遍历

好在这套"分页 + 逐层下钻 + 扫描量兜底"的遍历,我们在做**桶内递归搜索**时已经写过、也踩过坑(一次性拉完整层会卡死)。所以这里没有重造,而是把它抽成一个共用的 `walk_dir`:

```rust
async fn walk_dir(
    provider: &Arc<dyn StorageProvider>,
    root: &str,
) -> Result<(Vec<Entry>, Vec<String>)> {
    let mut files = Vec::new();
    let mut dirs = Vec::new();
    let mut queue = std::collections::VecDeque::new();
    queue.push_back(root.to_string());
    while let Some(dir) = queue.pop_front() {
        let mut cursor = None;
        loop {
            let page = provider.list_page(&dir, cursor).await?;
            for entry in page.entries {
                if entry.is_dir() {
                    dirs.push(entry.path.clone());
                    queue.push_back(entry.path);   // 下钻
                } else {
                    files.push(entry);
                }
            }
            // 超大目录兜底,防止无限扫描
            if /* scanned 触顶 */ false { return Ok((files, dirs)); }
            match page.cursor {
                Some(next) => cursor = Some(next),  // 翻页
                None => break,
            }
        }
    }
    Ok((files, dirs))
}
```

它同时收集**文件**(拿来下载 / 迁移)和**目录前缀**(拿来删除时清理占位对象)。三个操作都建在它之上。

## 三个操作,各自复用一条已有能力

**下载整个文件夹**:遍历出所有文件,逐个流式下到 `本地目录/{文件夹名}/{相对路径}`,保留层级。每个文件复用已有的流式下载(边下边写,大文件不占内存)。

**迁移整个文件夹**:把源前缀下每个对象复用 `copy_across`——同账号走服务端复制、跨账号走边下边传的流式中转——落到目标目录下、以源文件夹名作为子目录,保留相对结构:

```rust
let base = ensure_trailing_slash(src_root);
let dst_base = format!("{}{}/", ensure_trailing_slash(dst_dir), folder_name(src_root));
for file in &files {
    let rel = file.path.strip_prefix(&base).unwrap_or(&file.path);
    self.copy_across(src_account, &file.path, dst_account, &format!("{dst_base}{rel}"))
        .await?;
}
```

**删除整个文件夹**:先删所有文件对象,再删目录占位 key。这里有个小心思:对象存储的 DELETE 是**幂等**的,删一个不存在的合成前缀也返回成功,所以可以放心地把"新建文件夹"留下的零字节 `/` 占位对象一并清掉,不会因为它其实不存在而报错。

## 一个通用性红利

因为遍历只依赖统一的 `list_page`、操作只依赖已抽象好的 `read_stream` / `copy_across` / `delete`,这三个文件夹操作**没有一行厂商相关代码**。哪家云的账号都能右键下载 / 迁移 / 删除整个目录,以后新接入的云也一样——又一次,把能力建在公共抽象上,厂商越多,复利越大。

进度呢?文件夹操作按**文件数**汇报(已完成 N / 共 M),进度直接汇入传输面板,和单文件任务并列。删除前会明确提示这是**递归**操作、不可恢复——毕竟删的是一整棵前缀树。
