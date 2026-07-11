# Nebula

跨平台(桌面)多云存储管理器 —— 用统一界面管理阿里云 OSS、腾讯云 COS、华为云 OBS、AWS S3 等多家云存储。

底层每家云都是从零手写的**独立可发布 Rust SDK**,不依赖任何聚合库。

## 技术栈

- **语言 / GUI**:Rust + Tauri 2.0(桌面三端 macOS / Windows / Linux)
- **架构**:monorepo,三层解耦,依赖严格单向
  ```
  cloud-core → 各厂商 SDK → providers 适配层 → app
  ```
- **凭证**:系统钥匙串(keyring)
- **异步**:tokio

## 目录结构

```
nebula/
├─ crates/
│  ├─ cloud-core/               公共底座(HTTP / 签名积木 / 错误 / 重试 / 分页)
│  ├─ s3-sigv4/                 可复用的 AWS SigV4 签名器(S3 系通用)
│  ├─ s3-core/                  通用 S3 兼容客户端(供 qiniu/aws/R2/MinIO 复用)
│  ├─ aliyun-oss/               阿里云对象存储 SDK(对象/桶/列举/分片/预签名)
│  ├─ huawei-obs/               华为云对象存储 SDK(对象/桶/列举/分片/预签名)
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

> 已实现阿里云 OSS、华为云 OBS(均真账号验证过)、腾讯云 COS(专有 q-sign 签名)、七牛云 Kodo、
> AWS S3、Cloudflare R2、MinIO(后四家 S3 兼容共用 `s3-core`;COS 单独签名。除阿里/华为外真账号
> 冒烟待验证)。新增厂商 = 按
> [SDK 开发手册](./docs/sdk-playbook.md) 写一个 `<vendor>` SDK(S3 兼容的可直接复用 `s3-core`)
> + 一个 provider 适配层,在 `app-core` 注册即可,App 界面无需改动。完整规划见 [PLAN.md](./PLAN.md)。

## 应用功能

账号(SQLite + 系统钥匙串)· 浏览(目录逐层 / 列表·网格视图 / 排序 / 过滤 / 详情 / 存储类型 / Content-Type 查看·编辑 / 图片预览)
· 上传(拖拽文件·文件夹 / 多选 / 并发分片 / 断点续传 / 进度)· 下载(流式 / 批量 / 断点续传)· 重命名 / 复制·移动(级联选择器)
· 跨账号·跨云迁移 · 文件夹级递归(下载 / 迁移 / 删除整个目录)· 完整性校验(MD5 对比 ETag)· 存储类型转换 / 归档取回(单个 / 多选 / 整个文件夹)· 桶内递归搜索(名字 / 大小 / 类型过滤)· 文件夹 / Bucket 统计(对象数 / 总大小)· 分享(预签名链接)· 新建 / 删除 Bucket · 新建文件夹 · 删除(单个 / 批量)· 传输面板(并发 / 全局限速 / 取消·续传 / 重启恢复 / 重试)· 设置 · 中英双语 · 明暗主题 · 快捷键 · 右键菜单。

App 的运行与打包见 [app/README.md](./app/README.md)。

## 开发约定

- 每家 SDK 保持厂商原名(`aliyun-oss` 而非 `nebula-aliyun`),便于被搜索复用。
- 厂商 SDK **不得**依赖 App 层任何 crate,以保持独立可发布。
- 按功能逐个开发:每个功能验证通过后独立提交,确认后再进入下一个。

## 开发文档

- [SDK 开发手册](./docs/sdk-playbook.md) — 从零手写一个厂商 SDK 的标准流程、增量顺序、验收标准
- 厂商规格卡 `docs/sdk/<crate>.md` — 每家特有的签名/endpoint/进度(见 [aliyun-oss](./docs/sdk/aliyun-oss.md)、[huawei-obs](./docs/sdk/huawei-obs.md)、[tencent-cos](./docs/sdk/tencent-cos.md)、[qiniu-kodo](./docs/sdk/qiniu-kodo.md)、[aws-s3](./docs/sdk/aws-s3.md),新建用 [_template](./docs/sdk/_template.md))

## License

[MIT](./LICENSE)
