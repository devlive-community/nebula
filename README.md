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
│  ├─ cloud-core/         公共底座(HTTP / 签名积木 / 错误 / 重试 / 分页)
│  ├─ aliyun-oss/         阿里云对象存储 SDK(可发布)
│  ├─ tencent-cos/        腾讯云对象存储 SDK(可发布)
│  ├─ huawei-obs/         华为云对象存储 SDK(可发布)
│  ├─ ...                 其余厂商 SDK
│  ├─ nebula-provider/    App 统一抽象 trait
│  └─ providers/          适配层(SDK → StorageProvider,App 私有)
└─ app/                   Tauri 桌面应用(src-tauri 后端 + src 前端)
```

完整规划见 [PLAN.md](./PLAN.md)。

## 开发约定

- 每家 SDK 保持厂商原名(`aliyun-oss` 而非 `nebula-aliyun`),便于被搜索复用。
- 厂商 SDK **不得**依赖 App 层任何 crate,以保持独立可发布。
- 按功能逐个开发:每个功能验证通过后独立提交,确认后再进入下一个。

## License

MIT OR Apache-2.0
