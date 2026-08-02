---
title: 打开就白屏半秒:启动页要在 JS 跑起来之前就画出来
date: 2026-07-25
author: Nebula Team
description: 桌面应用打开的第一帧是空的——webview 要等 HTML 解析、JS 下载执行、React 挂载才有画面。这篇讲怎么用一段内联 HTML/CSS 在 JS 跑之前就画出品牌启动页,再在首帧渲染后淡出。
tags: ['开发', '前端', '跨平台']
---

Nebula 打开的一瞬间,窗口曾经是**纯白**的一小段时间——Tauri 的 webview 要先解析 `index.html`、再下载执行 JS bundle、再等 React 挂载出 `#root`,这中间有一段实打实的空白。窗口越晚出现内容,应用看起来就越"卡"。

## 关键:启动页不能等 JS

如果启动页是 React 组件,那它和真正的界面一样,得等 JS 跑起来才画得出来——等于没解决问题。**启动页必须是纯 HTML/CSS**,写在 `index.html` 里,浏览器解析到就画,不依赖任何脚本:

```html
<body>
  <div id="splash">
    <img src="/logo.svg" alt="Nebula" width="88" height="88" />
    <div id="splash-name">Nebula</div>
    <div id="splash-bar"><span></span></div>
  </div>
  <div id="root"></div>
  <script type="module" src="/src/main.tsx"></script>
</body>
```

`#splash` 是 `position: fixed; inset: 0; z-index: 9999`,`<body>` 背景也提前设成深色——这样在 `#splash` 出现之前的那一刻(HTML 刚解析、CSS 还没应用完)也不会闪一下白色,首帧直接是品牌深色。

## 何时移除:等首帧画完,而不是等挂载调用

React 挂载完成不等于**浏览器已经画出那一帧**。如果挂载后立刻移除启动页,可能会看到"启动页消失 → 短暂空白 → 真实界面出现"的二次闪烁。解法是用两层 `requestAnimationFrame`——第一层等当前帧排队完,第二层等下一帧真正画完:

```ts
const started = performance.now();
const hideSplash = () => {
  const wait = Math.max(0, SPLASH_MIN_MS - (performance.now() - started));
  setTimeout(() => {
    splashEl.classList.add("hide");   // CSS transition 淡出
    setTimeout(() => splashEl.remove(), 400);
  }, wait);
};
requestAnimationFrame(() => requestAnimationFrame(hideSplash));
```

同时给一个**最短展示时间**(500ms):如果应用启动特别快,启动页一闪而过反而显得廉价;设个下限,让品牌露出至少这么久,再淡出移除整个节点。

## 独立窗口不需要它

图片编辑器、PDF 编辑器是通过 `?view=image` 之类参数打开的独立 Tauri 窗口,不是主界面——它们不该有品牌启动页(体积小、开合频繁),`main.tsx` 检测到 `view` 参数就直接把 `#splash` 摘掉,不走淡出动画。

## 小结

启动页要解决的不是"好看",而是"第一帧别是空的"。做法就两条:纯 HTML/CSS 保证不等 JS 就能画出来;双 `requestAnimationFrame` + 最短展示时间保证移除的时机既不早于真实内容画完,也不会因为太快而一闪而过。
