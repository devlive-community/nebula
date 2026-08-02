---
title: 「跟随系统」存的不该是深色还是浅色,而是「跟随系统」这四个字
date: 2026-07-25
author: Nebula Team
description: 给主题加一个"跟随系统"选项,第一反应是系统当前是深色就存深色——但这样存下的是推导结果,不是用户的真实偏好。这篇讲为什么要持久化偏好本身,以及怎么用 matchMedia 响应系统日夜切换。
tags: ['开发', '前端', '持久化']
---

Nebula 原来的主题只有深色 / 浅色两个显式值,这次加第三个选项:**跟随系统**。第一版实现很容易掉进一个坑:用户选了"跟随系统",这一刻系统是深色,于是把 `"dark"` 存进偏好——下次启动时,读到的只是"深色",用户当初选的"跟随系统"这个**意图**已经丢了。

## 存偏好,不存推导结果

正确的模型是拆成两层:**偏好**(用户选的)和**生效主题**(算出来的):

```ts
const [themePref, setThemePref] = useState<"dark" | "light" | "system">("system");
const [systemDark, setSystemDark] = useState(
  () => window.matchMedia("(prefers-color-scheme: dark)").matches,
);
const theme: "dark" | "light" =
  themePref === "system" ? (systemDark ? "dark" : "light") : themePref;
```

持久化的是 `themePref`,不是 `theme`。这样重启后,"跟随系统"的用户永远重新推导一次当前系统色,而不是被上次启动时的系统色钉住。

## 系统日夜切换要能实时响应

`matchMedia` 不只能读一次,还能监听变化——用户改系统外观(比如到点自动切换深色模式)时,应用要跟着变,不用重启:

```ts
useEffect(() => {
  const mq = window.matchMedia("(prefers-color-scheme: dark)");
  const on = (e: MediaQueryListEvent) => setSystemDark(e.matches);
  mq.addEventListener("change", on);
  return () => mq.removeEventListener("change", on);
}, []);
```

## 切换按钮:在生效主题上翻转,而不是在偏好上翻转

工具栏的主题切换按钮和命令面板的"切换主题"命令还留着,行为改成:在**当前生效的主题**基础上翻成一个**显式**偏好——即使当前是"跟随系统"推出来的深色,点一下切换也会变成明确的"浅色",而不是在三个值之间打转:

```ts
const toggleTheme = () => setThemePref(theme === "dark" ? "light" : "dark");
```

独立的图片 / PDF 编辑窗口是各自的 Tauri 窗口,也各自解析一次系统色,保持和主窗口一致。

## 小结

"跟随系统"类选项的通用教训:持久化的对象应该是**用户的选择**,不是选择在某一刻算出来的**结果**——一旦把两者混为一谈,重启就会悄悄丢掉用户的真实意图。
