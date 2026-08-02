---
title: 文本预览加行号,别再用一个 `<pre>` 打天下
date: 2026-07-25
author: Nebula Team
description: 文本/代码预览原来是一整块 `<pre>{text}</pre>`,看日志、配置、代码都数不清行号。这篇讲怎么不引入代码高亮库,只靠拆行和 CSS 就补上行号、自动换行开关和一键复制。
tags: ['开发', '前端', 'UI']
---

Nebula 能预览文本 / 代码 / 配置文件,但原来的实现只是把内容整块塞进一个 `<pre>`——看日志报错在第几行、复制片段配置,都得自己数、自己选中。这次给文本预览加上**行号**、**自动换行开关**和**一键复制全部**,没有引入任何代码高亮依赖。

## 行号靠拆行,不是靠 CSS 计数器

最直接的做法是把内容按行拆开,每行渲染成一个两列的 flex 行(行号 + 内容):

```tsx
const lines = kind === "text" ? (text ?? "").split("\n") : [];
...
<div className={`preview__code ${wrap ? "preview__code--wrap" : ""}`}>
  {lines.map((line, i) => (
    <div className="preview__codeline" key={i}>
      <span className="preview__ln">{i + 1}</span>
      <span className="preview__lc">{line || " "}</span>
    </div>
  ))}
</div>
```

空行要渲染成 `" "` 而不是空字符串——空 `<span>` 在某些排版下高度会塌陷,行号和内容对不齐。这里没有用 CSS `counter-reset`/`counter-increment` 配 `::before`,是因为拆行渲染同时也顺手解决了"自动换行开关"和"复制不带行号"这两个需求:换行只是给 `.preview__code--wrap` 切换 `white-space`,复制走的是原始 `text` 字符串,天然不含行号,不需要额外过滤。

## 复制走原始文本,不走 DOM

复制按钮直接读组件外部传入的原始 `text`,而不是从渲染出的 DOM 里拼——这样保证复制出来的内容和源文件字节一致,不受行号、换行显示这些纯展示层的影响:

```ts
const copy = async () => {
  try {
    await navigator.clipboard.writeText(text ?? "");
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  } catch { /* 忽略剪贴板失败 */ }
};
```

## 预览本来就有体积上限,行数不会失控

这个实现按行渲染 DOM 节点,大文件理论上会有性能问题——但文本预览走的是已有的**流式读取 + 256 KB 截断**(见《预览文本文件,为什么要绕一圈走后端》),单次预览的行数天然有上限,不需要为这个功能单独引入虚拟滚动。

## 小结

行号、换行、复制这三个看似要上一个代码编辑器组件才能做的事,在"预览"这个场景下(只读、体积有上限)靠一次 `split("\n")` 加两列布局就够了——没必要为了小体积的预览引入大体积的编辑器依赖。
