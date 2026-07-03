# Nebula 桌面应用

Tauri 2 + React + TypeScript + 自定义 CSS 的多云存储管理界面。

## 架构

```
React 前端 (src/)  ──invoke──►  Tauri command (src-tauri/src/lib.rs)
                                    │ 调用
                                    ▼
                               app-core::App  ──►  provider-aliyun ──► aliyun-oss
```

前端只通过 `src/api.ts` 里类型化的 `invoke` 调后端;后端 command 极薄,业务逻辑都在
`crates/app-core`(可 `cargo test`)。

## 运行(需在本机)

前置:Node ≥ 18、pnpm、Rust 工具链;macOS/Windows/Linux 桌面环境。

```bash
cd app
pnpm install
pnpm tauri dev      # 启动开发窗口(热重载)
```

首次进入点「+ 添加账号」,填入阿里云 OSS 的账号别名 / AccessKeyId / AccessKeySecret /
Endpoint(如 `oss-cn-hangzhou.aliyuncs.com`),即可浏览 bucket、进目录、上传 / 下载 / 删除。

## 打包

打包前用一张 1024×1024 的 PNG 生成完整图标集(含 macOS `.icns` / Windows `.ico`):

```bash
pnpm tauri icon ./path/to/app-icon.png
pnpm tauri build
```

> 仓库里 `src-tauri/icons/` 目前是纯色占位 PNG,仅够 `tauri dev` 使用。

## 说明

- 账号目前保存在内存中(重启需重新添加);后续接入系统钥匙串(keyring)持久化。
- 目录浏览基于 OSS delimiter 逐层展开;上传 / 下载通过系统文件对话框选择本地文件。
