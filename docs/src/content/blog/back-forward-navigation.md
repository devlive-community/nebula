---
title: 像浏览器一样前进后退,但历史栈不能放进 React state
date: 2026-07-25
author: Nebula Team
description: 给文件管理器加前进/后退导航,听起来只是记一串访问过的路径。真正的麻烦在于:导航本身也会触发"记录当前路径"的逻辑,不小心就会在后退的时候把后退动作自己也记成一条新历史。
tags: ['开发', '前端', '交互']
---

Nebula 的每次切换账号或目录都会改 `current` / `path` 这两个 state。给它加前进 / 后退,直觉做法是拿一个 `useState` 数组当历史栈,每次 `current`/`path` 变化就 `push`。这里有个死循环等着你:**后退本身也会改 `current`/`path`,如果后退动作也被当成"新的一次导航"记录进去,历史栈会不断把后退动作又摞成新的一条前进记录**,后退键完全不可用。

## 用 ref 存历史栈,而不是 state

历史栈的写入不需要触发渲染(渲染由 `current`/`path` 自己的 state 负责),所以用 `useRef` 存,配一个手动的重渲染信号:

```ts
const navHistory = useRef<{ account: string | null; path: string }[]>([]);
const navIdx = useRef(-1);
const navBack = useRef(false);      // 本次变化是"导航动作"引起的,别再入栈
const [navVer, setNavVer] = useState(0);  // 只用来强制刷新 canBack/canForward
```

## 用一个标志位打断循环

`goHistory` 在跳转前先把 `navBack.current` 置位;监听 `current`/`path` 的 effect 一看到这个标志就直接消费掉、不入栈:

```ts
useEffect(() => {
  if (navBack.current) { navBack.current = false; return; }  // 这次是后退/前进本身,跳过
  const cut = navHistory.current.slice(0, navIdx.current + 1);
  const last = cut[cut.length - 1];
  if (last && last.account === current && last.path === path) return; // 去重
  cut.push({ account: current, path });
  navHistory.current = cut;
  navIdx.current = cut.length - 1;
  setNavVer((v) => v + 1);
}, [current, path]);

const goHistory = (delta: number) => {
  const target = navIdx.current + delta;
  if (target < 0 || target >= navHistory.current.length) return;
  navIdx.current = target;
  navBack.current = true;                 // 打断上面的入栈逻辑
  const e = navHistory.current[target];
  if (e.account !== current) setCurrent(e.account);
  setPath(e.path);
  setNavVer((v) => v + 1);
};
```

跳到某个历史节点之后再手动导航到别处,会先 `slice(0, navIdx + 1)` 截断掉"未来"的分支——和浏览器历史的行为一致:后退几步再点别的链接,原来的前进历史就没了。

## 入口:工具栏按钮 + 快捷键

`Alt/⌘ + ←/→` 绑定到 `goHistory(-1)` / `goHistory(1)`,工具栏也给了一对图标按钮,`canBack`/`canForward` 由 `navIdx` 相对栈的位置算出,用来控制按钮的禁用态。

## 小结

「记一串状态、状态变化时自动入栈」这个模式一旦遇到"程序自己触发的状态变化不该被记录"的场景,就得有一个标志位来分辨"这次变化是用户的新动作"还是"这次变化是回放历史的结果"——historyStack 用 ref 存也是同一个道理:它是**导航层的账本**,不该被当成渲染要用的数据。
