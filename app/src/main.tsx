import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { ImageWindow } from "./components/ImageWindow";
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

// 图片在独立窗口打开:URL 带 ?view=image&… 时,该窗口只渲染图片浏览器(单张)。
const params = new URLSearchParams(window.location.search);
const isImageWindow = params.get("view") === "image";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <LocaleProvider>
      {isImageWindow ? (
        <ImageWindow
          account={params.get("account") ?? ""}
          path={params.get("path") ?? ""}
          name={params.get("name") ?? ""}
          etag={params.get("etag") || null}
          size={Number(params.get("size") ?? 0)}
        />
      ) : (
        <App />
      )}
    </LocaleProvider>
  </React.StrictMode>,
);
