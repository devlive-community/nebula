---
title: Shift 多选看着是个 checkbox 小事,其实是"当前视图顺序"的事
date: 2026-07-25
author: Nebula Team
description: 给文件列表加 Shift 范围多选,真正的难点不是监听 Shift 键,而是同一个功能要在列表、网格、搜索结果三种不同顺序的视图里都选出"看起来对"的那一段。
tags: ['开发', '前端', '交互']
---

Nebula 的多选一直只能一个个点 checkbox。这次补上大家在文件管理器里习惯的操作:点一个,按住 Shift 点另一个,中间整段都选中。

## 先记住"锚点",而不是"上一次点的"

Shift 多选需要一个基准点——"从哪到哪"。用一个 `ref` 记录上次点击(非 Shift)的路径当锚点:

```ts
const selAnchor = useRef<string | null>(null);
const toggleSelect = (p: string, shift?: boolean) => {
  ...
  if (!shift) selAnchor.current = p;
};
```

## 难点:范围是相对"当前视图顺序"的,不是相对某个固定顺序

同一批文件在**列表视图**、**网格视图**、**搜索结果**里的排列顺序可能完全不同(排序列、搜索命中顺序都会打乱原始顺序)。"从 A 到 B 之间"这句话必须相对**当前正在渲染的那份有序数组**求值,不能有一份全局固定顺序:

```ts
const ordered = (
  search ? search.results.filter((e) => e.kind === "file") : visibleFiles
).map((f) => f.path);

if (shift && selAnchor.current) {
  const a = ordered.indexOf(selAnchor.current);
  const b = ordered.indexOf(p);
  if (a !== -1 && b !== -1) {
    const [lo, hi] = a < b ? [a, b] : [b, a];
    setSelected((prev) => {
      const next = new Set(prev);
      for (let i = lo; i <= hi; i++) next.add(ordered[i]);
      return next;
    });
    return;
  }
}
```

锚点如果不在当前视图里(比如切了目录),`indexOf` 返回 `-1`,直接跳过范围逻辑、退化成普通单选——不会因为找不到锚点而选出一段无意义的范围。

## Checkbox 要把 Shift 键状态转发出来

`Checkbox` 组件原来的 `onChange` 是无参的;要让上层知道这次点击是否按了 Shift,得在组件内部读原生事件的 `shiftKey` 并转发：

```tsx
onChange: (shift?: boolean) => void;
// ...
onClick={(e) => { e.stopPropagation(); onChange(e.shiftKey); }}
```

列表、网格、搜索结果三处调用点都要跟着把这个参数接住、传给同一个 `toggleSelect`——功能本身只写一遍,三个视图共用。

## 小结

Shift 多选表面是"记两个点、选中间"的简单逻辑,真正的复杂度都在"中间"是相对谁而言——一旦一个功能要在多个排序不同的视图里保持一致的用户预期,「当前可见顺序」就必须是一份现算的、贴着当前渲染结果的数组,而不是某处缓存的固定顺序。
