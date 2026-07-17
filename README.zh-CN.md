[English](README.md) · **简体中文** · [Русский](README.ru.md) · [日本語](README.ja.md) · [한국어](README.ko.md)

# Nebula

**跨平台桌面端的多云对象存储管理器** —— 在一个原生应用里,像用本地文件管理器一样管理阿里云 OSS、腾讯云 COS、华为云 OBS、AWS S3、Cloudflare R2、七牛云 Kodo、MinIO:浏览、上传下载、分享、跨云迁移,一站搞定。

用 Rust + Tauri 打造,轻量、快速、原生三端(macOS / Windows / Linux)。底层每家云都是从零手写的**独立可发布 Rust SDK**,不依赖任何聚合库。

> 官网:<https://nebula.devlive.org/> · 下载:[GitHub Releases](https://github.com/devlive-community/nebula/releases)

## 支持的云

| 云厂商 | 状态 | 签名 |
|--------|------|------|
| 阿里云 OSS | ✅ 真账号验证 | V2 (HMAC-SHA1) |
| 华为云 OBS | ✅ 真账号验证 | V2 (HMAC-SHA1) |
| 腾讯云 COS | ✅ | 专有 q-sign |
| 七牛云 Kodo | ✅ | S3 兼容(共用 `s3-core`) |
| AWS S3 | ✅ | SigV4 |
| Cloudflare R2 | ✅ | S3 兼容(SigV4) |
| MinIO | ✅ | S3 兼容(支持 http / 自定义端口) |

> 新增一家厂商 = 写一个 `<vendor>` SDK(S3 兼容的可直接复用 `s3-core`)+ 一个 provider 适配层,在 `app-core` 注册即可,**App 界面无需改动**。

## 功能

- **浏览与预览** —— 目录逐层展开、列表 / 网格视图、排序过滤;**Rust 加速的图片浏览器 / 编辑器**(在独立窗口打开单张图;后端解码 / 缩放 / 按 ETag 磁盘缓存,前端只拿小图,支持缩放·平移·旋转·EXIF;编辑支持裁剪 / 旋转 / 拉直 / 翻转 / 调整尺寸 / 灰度 / 反相 / 亮度 / 对比度 / 饱和度 / 色温 / 锐化 / 模糊 / 色相,撤销重做,可选 JPEG / PNG / WebP 格式与画质,实时预览后一键存回云端(覆盖或另存为新对象)或下载到本地);视频 / 音频 / PDF / 文本代码预览;对象详情含存储类型、真实 Content-Type(可改)与可读写的对象标签;文件夹 / Bucket 大小统计与按存储类型的分布。
- **上传与下载** —— 拖拽文件 / 文件夹、多选上传;大文件并发分片、断点续传;内容未变的文件自动秒传跳过;流式下载不占内存、整目录递归下载。
- **迁移与整理** —— 对象或整个文件夹跨账号、跨云迁移(同账号服务端复制,跨账号自动中转);同账号内复制 / 移动 / 重命名到任意目录;多选对象**批量重命名**(加前缀 / 后缀 / 查找替换,先预览后执行)。
- **分享与公开** —— 生成预签名下载 / 上传临时链接;把对象设为公开读并拿到**永久公共直链**,还能给账号配自定义域名 / CDN,直链走你自己的域名。
- **搜索与批量** —— 桶内递归搜索(名字 / 大小 / 类型过滤),结果可多选批量转档 / 取回 / 删除 / 下载;跨区域的桶自动路由到各自的区域。
- **导航与效率** —— **Cmd/Ctrl+K 命令面板**(输入即跳转账号 / 收藏或执行命令)、**收藏夹**、**最近访问**;中英双语、明暗主题、快捷键、右键菜单。
- **传输管理** —— 并发控制、全局限速、速度 / ETA、取消 / 续传、重启恢复、一键重试全部失败。
- **存储治理** —— 一键清理桶内残留的未完成分片上传,回收白白计费的存储;单个对象或整个文件夹都能转换存储层、取回归档。
- **账号与安全** —— 密钥存入系统钥匙串;元信息、设置、收藏、最近访问、界面偏好持久化到本地 SQLite(分表存储,重启后未完成的传输可续)。
- **自动更新** —— 应用内检测新版本,一键下载安装并校验更新签名。

App 的运行与打包见 [app/README.md](./app/README.md)。

## 技术栈与架构

- **语言 / GUI**:Rust + Tauri 2.0(桌面三端 macOS / Windows / Linux)
- **凭证**:系统钥匙串(keyring) · **异步**:tokio
- **架构**:monorepo,三层解耦,依赖严格单向

```
cloud-core → 各厂商 SDK → providers 适配层 → app
```

```
nebula/
├─ crates/
│  ├─ cloud-core/               公共底座(HTTP / 签名积木 / 错误 / 重试 / 分页)
│  ├─ s3-sigv4/                 可复用的 AWS SigV4 签名器(S3 系通用)
│  ├─ s3-core/                  通用 S3 兼容客户端(供 qiniu/aws/R2/MinIO 复用)
│  ├─ aliyun-oss/               阿里云对象存储 SDK(对象/桶/列举/分片/预签名)
│  ├─ huawei-obs/               华为云对象存储 SDK
│  ├─ tencent-cos/              腾讯云对象存储 SDK(COS 专有签名 q-sign)
│  ├─ qiniu-kodo/               七牛云对象存储 SDK(S3 兼容,s3-core 门面)
│  ├─ aws-s3/                   AWS S3 SDK(S3 REST + SigV4,s3-core 门面)
│  ├─ cloudflare-r2/            Cloudflare R2 SDK(S3 兼容,s3-core 门面)
│  ├─ minio-s3/                 MinIO SDK(S3 兼容,支持 http/自定义端口)
│  ├─ nebula-provider/          App 统一抽象 trait(StorageProvider)
│  ├─ providers/provider-*      适配层(各 SDK → StorageProvider)
│  └─ app-core/                 App 业务逻辑(账号/传输/设置,框架无关,可 cargo test)
└─ app/                         Tauri 桌面应用(src-tauri 后端 + src 前端)
```

## 开发文档

- [SDK 开发手册](./docs/sdk-playbook.md) —— 从零手写一个厂商 SDK 的标准流程、增量顺序、验收标准
- 厂商规格卡 `docs/sdk/<crate>.md` —— 每家特有的签名 / endpoint / 进度(见 [aliyun-oss](./docs/sdk/aliyun-oss.md)、[huawei-obs](./docs/sdk/huawei-obs.md)、[tencent-cos](./docs/sdk/tencent-cos.md)、[qiniu-kodo](./docs/sdk/qiniu-kodo.md)、[aws-s3](./docs/sdk/aws-s3.md),新建用 [_template](./docs/sdk/_template.md))

## 开发约定

- 每家 SDK 保持厂商原名(`aliyun-oss` 而非 `nebula-aliyun`),便于被搜索复用。
- 厂商 SDK **不得**依赖 App 层任何 crate,以保持独立可发布。
- 按功能逐个开发:每个功能验证通过后独立提交,确认后再进入下一个。

## License

[MIT](./LICENSE)
