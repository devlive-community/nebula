# Nebula 项目规划

## 一、定位

桌面端多云对象存储管理器。统一界面管理阿里云 OSS、腾讯云 COS、华为云 OBS、AWS S3 及其他云,
底层每家云从零手写独立可发布的 Rust SDK。

**边界**:不做移动端;一期不做本地-云实时同步(放后期)。

## 二、已锁定的核心决策

1. **不用任何聚合库**(不用 OpenDAL / rusoto 等),每家云从零写原生 SDK。
2. 每家 SDK 是独立 crate,可单独发布到 crates.io 复用。
3. **三层解耦 + 严格单向依赖**:
   ```
   cloud-core → 厂商 SDK → providers 适配层 → app
   ```
   厂商 SDK 不得依赖 App 层任何东西(红线)。
4. **按签名 / endpoint 体系拆 crate**:同签名同 endpoint = 一个 crate;换签名或换 endpoint = 拆新 crate。
5. monorepo 单仓组织;需要时再把稳定 SDK 拆出独立仓。
6. GUI 用 Tauri 2.0 + React/TS。

## 三、crate 划分

| crate | 覆盖范围 | 签名体系 | 发布 |
|-------|---------|---------|------|
| `cloud-core` | HTTP / 签名积木 / 错误 / 重试 / 分页 / 流式 body | — | ✅ |
| `aliyun-oss` | OSS 对象读写 + 桶管理 + 生命周期 + ACL | OSS 专有签名 | ✅ |
| `aliyun-openapi` | CDN / 账单 / STS | ACS3-HMAC-SHA256 | ✅ |
| `aliyun-pan` | 阿里云盘私有 API | OAuth2 | ✅ |
| `tencent-cos` | COS 对象读写 + 桶管理 | COS 专有签名 | ✅ |
| `tencent-cloud-api` | CDN / 账单 | TC3-HMAC-SHA256 | ✅ |
| `huawei-obs` | OBS 对象读写 + 桶管理 | OBS 类 AWS 签名 | ✅ |
| `huawei-openapi` | CDN / 账单 | SDK-HMAC-SHA256 | ✅ |
| `aws-s3` | S3 对象读写 + 桶管理 | SigV4 | ✅ |
| `nebula-provider` | 统一抽象 trait + 能力位 + 条目模型 | — | 可选 |
| `providers/provider-*` | SDK → StorageProvider 适配 | — | ❌ App 私有 |

## 四、每个 SDK 的六件套模板

以 `aliyun-oss` 为范本,其余家复制骨架、主要换 `sign.rs` 和 endpoint:

```
src/
├─ lib.rs      导出公共 API
├─ client.rs   Client::new(ak, sk, endpoint, region)
├─ sign.rs     该家签名组装
├─ object.rs   put / get / delete / head / copy / 分片上传
├─ bucket.rs   建桶 / 删桶 / 列桶 / 生命周期 / ACL
├─ types.rs    请求/响应结构体(serde)
└─ error.rs    XxxError(thiserror)
```

## 五、签名(最易翻车,各家不同)

| SDK | 数据面签名 | 管理面签名 |
|------|-----------|-----------|
| 阿里云 OSS | OSS 专有(HMAC-SHA1)/ V4 | ACS3-HMAC-SHA256 |
| 腾讯云 COS | COS 专有(sha1) | TC3-HMAC-SHA256 |
| 华为云 OBS | 类 AWS V2/V4 | SDK-HMAC-SHA256 |
| AWS S3 | SigV4 | SigV4 |
| 阿里云盘 | OAuth2 token | 同左 |

`cloud-core` 只放签名积木(HMAC / SHA / canonical 构造 / 时间戳);
每家签名的具体组装放各自 SDK,规则差异太大不强行抽象。

## 六、构建顺序(逐功能,每步验证后提交)

1. **项目脚手架**(本提交):workspace + 配置 + 文档。
2. `cloud-core`:HTTP + 签名积木 + 错误 + 分页(先够 OSS 用)。
3. `aliyun-oss`:第一个完整 SDK,数据面 + 桶管理 + 签名跑通,作为模板。
4. 试发布 `aliyun-oss` 到 crates.io,验证发布流程。
5. 复制模式做 `tencent-cos`、`huawei-obs`。
6. `nebula-provider` trait + 适配层 + Tauri App 骨架。
7. 传输引擎(分片 / 并发 / 断点续传 / 限速)。
8. 私有网盘、CDN/账单等 `*-openapi`。
9. (后期)同步引擎。

## 七、开发 workflow

- 一次只做一个功能。
- 功能完成 → 用户验证 → 独立提交 → 用户确认 → 进入下一个功能。

## 八、主要风险

| 风险 | 对策 |
|------|------|
| 手写签名易出 bug | 每家先用真实账号跑通最小用例再扩展;签名单测覆盖官方文档示例向量 |
| crates.io 命名冲突 | 早查名/占位(如 `aliyun-oss` 被占则用 `-rs` 后缀) |
| SemVer 约束 | 公开类型用 `#[non_exhaustive]` 留扩展余地 |
| 凭证安全 | 全程 keyring,不明文落盘 |
| 传输/同步引擎工作量大 | 一期串行传输,引擎硬化与同步后置 |
