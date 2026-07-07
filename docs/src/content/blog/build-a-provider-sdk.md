---
title: 手把手:为 Nebula 新增一个云厂商 SDK
date: 2026-07-07
author: Nebula Team
description: Nebula 的每家云都是从零手写的独立 Rust SDK。这篇按标准增量顺序,带你把一个新厂商从 crate 骨架写到能在 App 里跑通,并说清每一步怎么验收。
tags: ['开发', 'SDK']
---

> 完整规范见仓库里的 [SDK 开发手册](https://github.com/devlive-community/nebula/blob/main/docs/sdk-playbook.md);本文是它的实操版,配合厂商规格卡一起看。

Nebula 的架构把"厂商差异"关在最底层:

```
cloud-core  →  厂商 SDK  →  provider 适配层  →  app
```

加一家新云,本质就是:**写一个厂商 SDK + 一个适配层 + 在 app-core 注册一下**,App 界面一行都不用改。下面按标准流程走一遍。

## 0. 三条不可违反的红线

1. 厂商 SDK 只依赖 `cloud-core` + reqwest/serde/tokio,**绝不**依赖 `nebula-provider` / `app` 或其它厂商 SDK。
2. 依赖方向严格单向:`cloud-core → 厂商 SDK → 适配层 → app`。
3. SDK 用**厂商原名**(如 `tencent-cos`,不是 `nebula-tencent`),便于被搜索复用。

## 1. crate 六件套

```
crates/<vendor>/src/
├─ lib.rs      导出 + crate 文档
├─ error.rs    XxxError(thiserror,#[non_exhaustive])
├─ sign.rs     该家签名组装(★最易翻车)
├─ client.rs   XxxClient:凭证 + endpoint + url 拼接
├─ object.rs   put/get/delete/head + 签名请求构造器
├─ bucket.rs   list_objects(分页)、list/create/delete bucket
└─ multipart.rs 分片上传
```

## 2. 增量顺序(每步一次独立提交,做完必须验收)

| # | 增量 | 验收方式 |
|---|------|---------|
| 1 | crate 骨架 | `cargo metadata` 通过 |
| 2 | **签名** | ★官方文档**示例向量**单测逐字节相等 |
| 3 | client | endpoint / url 单测 |
| 4 | error | 错误响应解析单测 |
| 5 | 对象操作 | 离线:请求 URL + 签名头逐字节比对 |
| 6 | 列举分页 | 离线:签名 / 查询 / XML 解析 + 翻页游标 |
| 7 | 桶管理 | 离线:service / bucket-root 签名 |
| 8 | 分片上传 | 离线:子资源排序签名 + complete body |
| 9 | smoke | ★真实账号跑通全链路 |

## 3. 签名:唯一能离线证明正确的手段

各家签名算法不同(OSS 是 HMAC-SHA1,腾讯 TC3 是多轮 HMAC-SHA256)。**加密积木**(HMAC / SHA / base64)放 `cloud-core::crypto`,**如何拼签名**放各自 SDK 的 `sign.rs`。

关键规矩:**必须用厂商官方文档给的示例(AK/SK + 预期签名)写单测**,逐字节相等。这是唯一能离线证明签名对不对的办法,不允许跳过。比如 `aliyun-oss` 就用官方 `44CF9590006BF252F707` 那组向量锁死了签名。

## 4. 离线 vs 联网验证的边界

- **能离线验证**:签名(官方向量)、请求 URL/头组装、查询参数、XML 解析、分页游标、分片 complete body → 单测覆盖。
- **只能联网验证**:真正的 put/get/list/multipart 往返 → `examples/smoke.rs`,由掌握真实账号的人跑,密钥只走**环境变量**。

## 5. 接进 App

写一个适配层 `providers/provider-<vendor>`,把 SDK 包成统一的 `StorageProvider`;再在 `app-core` 注册:

```rust
// 之前
registry.register(Arc::new(AliyunProvider::new(...)));
// 加一家
registry.register(Arc::new(TencentProvider::new(...)));
```

App 全程只认 `dyn StorageProvider`,界面**零改动**。

## 6. 验收硬标准

每个增量提交前必须同时满足(即本仓 CI 的三项):

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test -p <vendor>
```

---

照这套走,复刻 `tencent-cos` / `huawei-obs` / `aws-s3` 就是"复制骨架、换 `sign.rs`、换 endpoint"。欢迎来 [GitHub](https://github.com/devlive-community/nebula) 一起补齐更多云。
