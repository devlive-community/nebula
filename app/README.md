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
Endpoint(如 `oss-cn-hangzhou.aliyuncs.com`),即可开始管理。

## 功能

- **账号**:多账号;元信息存 SQLite,AccessKeySecret 存系统钥匙串(keyring)
- **浏览**:桶 / 目录逐层展开(OSS delimiter)、列排序、按名称过滤、对象详情抽屉
- **上传**:拖拽文件 / 文件夹(递归)、多选文件、选文件夹;大文件自动分片、进度条
- **下载**:流式下载带进度;多选批量下载到指定文件夹
- **文件操作**:重命名、复制 / 移动到(级联目录选择器)、新建文件夹、删除(单个 / 批量确认)
- **分享**:生成预签名临时链接,一键复制
- **传输面板**:多任务并发、每任务独立进度、失败可重试、可清除已完成
- **设置**:分享链接有效期、批量并发数(存 SQLite);明暗主题切换

## 打包

打包前用一张 1024×1024 的 PNG 生成完整图标集(含 macOS `.icns` / Windows `.ico`):

```bash
pnpm tauri icon ./path/to/app-icon.png
pnpm tauri build
```

> 仓库里 `src-tauri/icons/` 目前是纯色占位 PNG,仅够 `tauri dev` 使用。

## 说明

- 数据目录下的 `nebula.db`(SQLite)存账号元信息与设置;密钥单独存系统钥匙串。
- 目前内置阿里云 OSS 一家;新增厂商只需实现 `StorageProvider` 并在 `app-core` 注册,界面无需改动。
