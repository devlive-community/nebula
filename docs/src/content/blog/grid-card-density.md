---
title: 网格卡片大小可调,靠的是一个数字而不是三套样式
date: 2026-07-28
author: Nebula Team
description: 给网格视图加"小/中/大"三档卡片密度,不需要写三份 CSS——只要把卡片的最小宽度当成一个变量,交给 CSS Grid 的 auto-fill 去算列数就行。
tags: ['开发', '前端', 'UI']
---

Nebula 的网格视图（缩略图卡片）密度是固定的,这次加一个工具栏按钮,在小 / 中 / 大三档密度间循环切换,并记住上次的选择。

## 密度只是一个数字

网格布局用的是 `grid-template-columns: repeat(auto-fill, minmax(...))`,列数由浏览器根据容器宽度和"每列最小宽度"自动算出来。这意味着"卡片密度"根本不需要写三套不同的 CSS——只要把这个最小宽度参数化,变成一个由父组件传下来的 `cardMin`:

```tsx
<div
  className="grid"
  style={{ gridTemplateColumns: `repeat(auto-fill, minmax(${cardMin}px, 1fr))` }}
>
```

```ts
const cardMin = gridSize === "s" ? 110 : gridSize === "l" ? 190 : 140;
```

小 / 中 / 大三档对应三个数字,工具栏一个按钮循环切换 `gridSize`,持久化进 `ui_prefs`(复用应用里已有的偏好水合套路),下次打开记住上次选的密度。按钮只在网格视图下显示——列表视图没有这个概念。

## 小结

界面密度这一类"看起来要出三套样式"的需求,先看看有没有一个连续参数能表达它——CSS Grid 的 `auto-fill`/`minmax` 本身就是为"响应式列数"设计的,密度切换只是在喂给它不同的数字,不需要额外的布局分支。
