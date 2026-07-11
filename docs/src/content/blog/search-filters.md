---
title: 找出该归档的那些大文件:给搜索加大小与类型过滤
date: 2026-07-11
author: Nebula Team
description: 按名字搜只能回答"叫什么",答不了"哪些大、哪些是视频"。这篇给递归搜索加上大小和扩展名过滤,顺便把它和归档省钱的工作流接上。
tags: ['开发', '对象存储', '搜索', '成本']
---

Nebula 的桶内递归搜索一直只能按**名字**匹配。但真正做成本管理时,你问的往往不是"叫什么",而是"**哪些大**、**哪些是视频**"——因为决定"归档谁"靠的是大小和类型,不是名字。所以这次给搜索加上两个过滤:**最小大小**和**扩展名**。

现在你可以一句话找出"这个桶里所有大于 100 MB 的 `.mp4`",然后右键批量转归档。搜索和上一批做的存储类型转换,就这么接上了。

## 过滤发生在遍历途中,不是拿回来再筛

实现上有个选择:是把所有命中拉回来、再在前端筛掉不合条件的,还是在遍历的时候就筛?对超大桶来说,前者会把大量用不上的对象也传回来。所以过滤下沉到**遍历途中**——每扫到一个文件,名字、大小、扩展名一起判断,不合条件的根本不放进结果:

```rust
} else if (needle.is_empty() || entry.name.to_lowercase().contains(&needle))
    && filter.accepts(&entry)   // ← 大小 + 扩展名,就地判断
{
    results.push(entry);
    if results.len() >= max_results { break 'walk; }
}
```

`filter.accepts` 很朴素:

```rust
fn accepts(&self, entry: &Entry) -> bool {
    if let Some(min) = self.min_size {
        if entry.size < min { return false; }
    }
    if let Some(ext) = &self.ext {
        let want = ext.trim().trim_start_matches('.').to_lowercase();
        if !want.is_empty() && !entry.name.to_lowercase().ends_with(&format!(".{want}")) {
            return false;
        }
    }
    true
}
```

它复用的还是搜索原本那套"逐页下降、`max_results` 凑够就停、`SEARCH_SCAN_LIMIT` 兜底"的遍历——过滤只是往里多塞了一个判断,该有的防超大桶卡死的保护一个没丢。

## 前端:结果头上的两个控件

搜索结果视图的头部多了两样:一个**最小大小**下拉(不限 / >1MB / >10MB / >100MB / >1GB)和一个**扩展名**输入框。改任意一个,就用当前关键词重跑搜索。关键词管"叫什么",这两个管"多大、什么类型",三者叠加。

## 小结

搜索从"按名字"扩到了"按名字 + 大小 + 类型",而且过滤在遍历途中就地生效、不浪费带宽拉无用对象,复用了原有的分页遍历与兜底。更重要的是它把"发现"和"处置"连了起来:先搜出该归档的大文件,再批量转归档——省钱这件事,终于能从"找"一路做到"转"。
