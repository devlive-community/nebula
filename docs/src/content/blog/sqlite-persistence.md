---
title: 告别 localStorage:把配置全搬进本地 SQLite,并按用途分表
date: 2026-07-18
author: Nebula Team
description: 桌面应用里用浏览器 localStorage 存配置,既不可靠也不好备份。这篇讲 Nebula 怎么把账号、收藏、最近访问、界面偏好统一迁进本地 SQLite 并分表,以及前端「先默认渲染、挂载后回填、回填完才回写」的水合套路。
tags: ['开发', '持久化', '架构']
---

Nebula 是个 Tauri 桌面应用,前端跑在 webview 里。早期一些配置(主题、视图模式、语言、侧栏宽度、收藏夹)图省事存在了浏览器的 `localStorage`。这有几个问题:它绑在 webview 的存储里、不好备份、和后端已有的 SQLite 各存一半,数据是割裂的。

于是我们做了一次收敛:**所有配置统一迁进本地 SQLite,并按用途分表。**

## 分表,而不是一张大 KV

账号早就在 SQLite 里了。这次把剩下的也搬进去,但**没有**全塞进一张 `settings` 表,而是按语义分开:

```sql
CREATE TABLE bookmarks ( account, path, created_at, PRIMARY KEY (account, path) );
CREATE TABLE recent_locations ( account, path, seq, PRIMARY KEY (account, path) );
CREATE TABLE ui_prefs ( key TEXT PRIMARY KEY, value TEXT );  -- 主题 / 视图 / 语言 / 侧栏宽
```

`ui_prefs` 是界面偏好的 KV;`bookmarks` / `recent_locations` 是各自的集合表;应用配置(分享有效期、并发、限速)仍在原来的 `settings` 表。分表让每类数据的读写、修剪、迁移互不干扰——收藏是长期集合,最近访问是自动流水,硬塞一起迟早打架。

## 前端的水合套路:默认 → 回填 → 才回写

配置搬到后端后,前端就不能再同步读到值了(取偏好是一次异步 IPC)。如果处理不好,会出现「先用默认值渲染 → effect 立刻把默认值回写 → 覆盖掉用户真实偏好」的自毁。

套路是加一个 `hydrated` 标志:

```tsx
const hydrated = useRef(false);
useEffect(() => {                       // 挂载后加载并回填
  getPref("theme").then(v => v && setTheme(v)).finally(() => { hydrated.current = true; });
}, []);
useEffect(() => {                       // 变化时回写,但回填完成前不写
  if (hydrated.current) setPref("theme", theme);
}, [theme]);
```

先用默认值渲染 → 挂载后从 SQLite 读真实值回填 → **回填完成前绝不回写**。这样启动时短暂的默认态不会污染已存的偏好。

## 收获

「桌面应用别拿 localStorage 当数据库」不是新道理,但真正动手时,价值在两点:一是**单一数据源**——所有状态都在一个可备份的 `.db` 里;二是**分表的纪律**——按语义拆表,比往一张万能表里塞 key 更经得起以后加功能。而前端那个「默认 → 回填 → 才回写」的水合顺序,是所有「配置从同步存储换成异步后端」的场景都会遇到的坑,值得记一下。
