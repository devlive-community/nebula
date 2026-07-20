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
