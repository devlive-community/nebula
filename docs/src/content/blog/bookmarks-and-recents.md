---
title: 收藏夹与最近访问:把常去的桶 / 前缀变成一次点击
date: 2026-07-18
author: Nebula Team
description: 深层前缀每次都逐级点进去太费事。这篇讲 Nebula 的收藏夹(手动钉常去位置)与最近访问(自动记录去过的地方),以及为什么它们各用一张独立的 SQLite 表、最近访问为什么用逻辑序号而不是时间戳排序。
tags: ['开发', '效率', '持久化']
---

对象存储里,你真正天天打开的往往就那么几个深层前缀:某个桶的 `logs/2026/`、某个项目的 `assets/`。每次从桶列表逐级点进去很烦。Nebula 用两件小工具解决:**收藏夹**(手动钉)和**最近访问**(自动记)。

## 收藏夹:手动钉常去位置

头部一个书签下拉:在任意桶 / 前缀点「收藏当前位置」,之后从下拉一键跳回。收藏按 `账号 + 路径` 存,所以同名前缀在不同账号下互不干扰。

## 最近访问:自动记录去过的地方

你不需要手动收藏也能快速回到刚才的位置——每次进入一个桶 / 前缀,后台自动记一笔,保留最近 20 条,并在命令面板空输入时优先展示。

前端记录做了**防抖**:在某个目录停留够久(800ms)才写一次,快速穿行目录不会疯狂刷库。

## 为什么各用一张独立的表

Nebula 把所有配置从浏览器 localStorage 全部迁进了本地 SQLite,并且**按用途分表**。收藏和最近访问各占一张:

```sql
CREATE TABLE bookmarks (
    account TEXT, path TEXT, created_at INTEGER,
    PRIMARY KEY (account, path)
);
CREATE TABLE recent_locations (
    account TEXT, path TEXT, seq INTEGER,
    PRIMARY KEY (account, path)
);
```

分表不是洁癖:它们语义不同(收藏是用户显式集合、要长期保留;最近访问是自动流水、要修剪),混在一张 KV 表里迟早互相干扰。

## 一个小坑:最近访问别用时间戳排序

最直觉的做法是给每条记一个 `visited_at` 时间戳,按它倒序。但在快速操作或测试里,**同一毫秒内多次访问**会并列,排序不确定——我们的测试就因此偶发失败过一次。

改法是用一个**单调递增的逻辑序号 `seq`**:每次访问把该行的 `seq` 顶成「当前最大值 + 1」,列表按 `seq DESC`。逻辑序号严格递增、绝不并列,排序永远确定:

```sql
INSERT INTO recent_locations (account, path, seq)
VALUES (?1, ?2, (SELECT COALESCE(MAX(seq), 0) + 1 FROM recent_locations))
ON CONFLICT(account, path) DO UPDATE
   SET seq = (SELECT COALESCE(MAX(seq), 0) + 1 FROM recent_locations);
```

「用逻辑序号而非墙钟时间来定序」是个很常见的小经验——凡是「谁更靠前」比「具体几点」更重要的场景,都值得想一下。
