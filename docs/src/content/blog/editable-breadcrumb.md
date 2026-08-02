---
title: 面包屑一层层点太慢,不如直接粘一条路径进去
date: 2026-07-25
author: Nebula Team
description: 深层目录里,面包屑要一层层点才能跳转。这篇讲怎么给面包屑加一个"编辑模式":点铅笔切换成输入框,直接粘一条完整路径回车跳转,顺带处理用户输入路径时常见的多余斜杠。
tags: ['开发', '前端', '交互']
---

面包屑(`bucket / a / b / c /`)一直只能逐层点击——从别处复制来一条深层路径,得点好几次才能跳过去,或者干脆没法直接跳。这次给面包屑加一个编辑模式:点铅笔图标,面包屑变成一个预填当前路径的输入框,直接输入或粘贴,回车跳转,Esc 取消。

## 组件内部切一个"只读 / 编辑"分支

`Breadcrumb` 本来是纯展示组件,这次加一个本地 `editing` state,分支渲染:

```tsx
const [editing, setEditing] = useState(false);
const [draft, setDraft] = useState(path);

useEffect(() => {
  if (editing) { inputRef.current?.focus(); inputRef.current?.select(); }
}, [editing]);
```

进入编辑态时自动 focus 并全选输入框内容——粘贴路径不需要先手动清空。

## 提交前归一化,而不是校验

用户粘进来的路径经常带首尾空格、多余的斜杠(复制自地址栏或终端时尤其常见)。与其报错让用户自己改,不如直接**归一化**后再跳转:

```ts
const commit = () => {
  const p = draft.trim().replace(/^\/+/, "").replace(/\/+$/, ""); // 去空白、去首尾多余斜杠
  setEditing(false);
  if (p !== path) onNavigate(p);
};
```

`onNavigate` 复用的是原来"点击某一层面包屑"同一个回调——编辑模式只是拼出了一个更任意的目标路径,导航逻辑本身没有变化。

## 小结

这是个很小的交互补丁,但思路值得记一下:遇到"用户会粘贴/输入路径"的输入框,处理首尾空白和多余分隔符应该是**默认行为**而不是校验失败——用户的意图很清楚,归一化比报错更省事。
