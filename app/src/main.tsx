import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { ImageWindow } from "./components/ImageWindow";
import { PdfWindow } from "./components/PdfWindow";
import { LocaleProvider } from "./i18n";
import "./styles.css";

// 禁用系统(webview 默认)右键菜单;App 自身的自定义右键菜单由 React 状态驱动,不受影响。
window.addEventListener("contextmenu", (e) => {
  e.preventDefault();
  // WebKit 会在右键时选中指针下的词;非输入区清掉这段残留选区。
  const target = e.target as HTMLElement;
  if (!target.closest("input, textarea, [contenteditable='true']")) {
    window.getSelection()?.removeAllRanges();
  }
});

// 独立窗口:URL 带 ?view=image / ?view=pdf 时,该窗口只渲染对应的浏览器 / 编辑器。
const params = new URLSearchParams(window.location.search);
const view = params.get("view");

function Root() {
  if (view === "image") {
    return (
      <ImageWindow
        account={params.get("account") ?? ""}
        path={params.get("path") ?? ""}
        name={params.get("name") ?? ""}
        etag={params.get("etag") || null}
        size={Number(params.get("size") ?? 0)}
      />
    );
  }
  if (view === "pdf") {
    return (
      <PdfWindow
        account={params.get("account") ?? ""}
        path={params.get("path") ?? ""}
        name={params.get("name") ?? ""}
      />
    );
  }
  return <App />;
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <LocaleProvider>
      <Root />
    </LocaleProvider>
  </React.StrictMode>,
);

// 启动页只属于主窗口;图片 / PDF 独立编辑窗口(带 ?view=)立即移除,不显示品牌启动页。
const splashEl = document.getElementById("splash");
if (view) {
  splashEl?.remove();
} else if (splashEl) {
  // 主窗口:等 React 首帧(双 rAF)+ 最短展示时间后淡出,避免一闪而过。
  const SPLASH_MIN_MS = 500;
  const started = performance.now();
  const hideSplash = () => {
    const wait = Math.max(0, SPLASH_MIN_MS - (performance.now() - started));
    setTimeout(() => {
      splashEl.classList.add("hide");
      setTimeout(() => splashEl.remove(), 400);
    }, wait);
  };
  requestAnimationFrame(() => requestAnimationFrame(hideSplash));
}
