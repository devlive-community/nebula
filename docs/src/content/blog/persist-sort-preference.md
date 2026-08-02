---
title: 记住上次的排序方式,复用的是同一套"先默认、后回填"套路
date: 2026-07-27
author: Nebula Team
description: 文件列表按名称/大小/修改时间排序,重启后一直会回到默认排序。这次让它记住上次选的列和方向——没有新发明,直接照搬应用里已有的偏好持久化套路多存两个键。
tags: ['开发', '持久化']
---

Nebula 的文件列表可以按名称、大小、修改时间排序,升序或降序,但这个选择重启后不会保留,每次都要重新点。这次把 `sortKey`(排序列)和 `sortDir`(升降序)也存进 `ui_prefs`。

## 没有新逻辑,只是多存两个键

Nebula 的界面偏好(主题、侧栏宽度、视图模式……)早就统一走「启动时批量读取 → 回填 state → 回填完成后才允许回写」这一套水合流程(参见《告别 localStorage》)。排序偏好直接接入同一条流水线,启动时的批量读取多两个键、多两行校验:

```ts
const [sw, v, th, sc, sk, sd] = await Promise.all([
  api.getPref("sidebar_width"), api.getPref("view"), api.getPref("theme"),
  api.getPref("shortcuts"), api.getPref("sort_key"), api.getPref("sort_dir"),
]);
if (sk === "name" || sk === "size" || sk === "modified") setSortKey(sk);
if (sd === "asc" || sd === "desc") setSortDir(sd);
```

回写侧同理,变化时写回,回填完成前不写(`prefsHydrated.current` 守卫):

```ts
useEffect(() => {
  if (prefsHydrated.current) {
    api.setPref("sort_key", sortKey).catch(() => {});
    api.setPref("sort_dir", sortDir).catch(() => {});
  }
}, [sortKey, sortDir]);
```

## 小结

这类"再记住一个界面选择"的需求,一旦应用里已经有一套成熟的偏好持久化管线,真正的工作量就只是"接进去",不需要每次都重新考虑水合时序的坑——这也是当初把配置从零散的 `localStorage` 收敛成统一管线的价值:后面加什么新偏好都是同一套模板。
