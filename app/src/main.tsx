import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./styles.css";

// 禁用系统(webview 默认)右键菜单;App 自身的自定义右键菜单由 React 状态驱动,不受影响。
window.addEventListener("contextmenu", (e) => e.preventDefault());

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
