---
title: 换肤不用切主题文件,改一个 CSS 变量就够
date: 2026-07-28
author: Nebula Team
description: 给应用加可选强调色,不需要为每种颜色写一份完整的主题样式表——深色/浅色主题本身就是靠 CSS 变量撑起来的,强调色只要在运行时覆盖其中一个变量即可,恢复默认时把覆盖去掉就行。
tags: ['开发', '前端', 'UI']
---

Nebula 的深色 / 浅色主题一直是靠一组 CSS 自定义属性(`--primary`、`--bg` 等)驱动的。这次在设置页加一排强调色色板(六个预设),选一个,按钮、高亮、进度条这些用到 `--primary` 的地方全部换色——不用碰主题本身。

## 运行时覆盖变量,而不是切样式表

强调色的实现只有两个函数:设置和清除:

```ts
export function applyAccent(accent: Accent) {
  const root = document.documentElement;
  if (accent === "default") {
    root.style.removeProperty("--primary");       // 清除覆盖,回到主题自带的蓝色
    root.style.removeProperty("--primary-hover");
    return;
  }
  const a = ACCENTS.find((x) => x.id === accent);
  root.style.setProperty("--primary", a.color);
  root.style.setProperty("--primary-hover", a.hover);
}
```

关键在于 `removeProperty`:内联样式(`element.style`)的优先级高于样式表里定义的 CSS 变量,所以 `setProperty` 就能覆盖主题原本的 `--primary`;而"恢复默认"不需要知道主题原来的蓝色是什么,只要把内联覆盖**删掉**,层叠自然落回样式表里的原值。六个预设色只是六组"主色 + 悬停色",没有牵扯任何其他样式。

## 小结

只要一个主题体系本身是靠 CSS 变量搭起来的,"局部换色"就不需要新写样式、也不需要在多套主题文件间选择——运行时 `setProperty`/`removeProperty` 一对操作就能做完覆盖和恢复,恢复时甚至不用记住原始值是什么。
