# Nebula 云厂商 SDK 开发手册

本手册描述在 Nebula 里**从零手写一个云厂商 SDK crate** 的标准流程。所有厂商 SDK
(`aliyun-oss`、`tencent-cos`、`huawei-obs`……)都遵循同一套流程与目录模板;每家**特有**
的参数(签名算法、endpoint、子资源、错误结构)写在对应的《厂商规格卡》`docs/sdk/<crate>.md`。

> 面向对象:接手开发的人或 AI。读完本手册 + 对应规格卡,即可按增量顺序把一个 SDK 写完。
> 已完成的参照实现:[`aliyun-oss`](./sdk/aliyun-oss.md)(第一个打样 SDK,可对照其代码)。

---

## 0. 不可违反的架构红线

1. **厂商 SDK 只能依赖 `cloud-core` + reqwest/serde/tokio 等基础库**,
   **绝不能**依赖 `nebula-provider`、`app` 或其他厂商 SDK。违反即丧失独立发布价值。
2. 依赖方向严格单向:`cloud-core → 厂商 SDK → providers 适配层 → app`。
3. 厂商 SDK 用**厂商原名**(`aliyun-oss`,不是 `nebula-aliyun`),便于被搜索复用。
4. 签名的**通用积木**(HMAC/SHA/base64/hex/md5)放 `cloud-core::crypto`;
   **每家如何拼签名**放各自 SDK 的 `sign.rs`,不在 `cloud-core` 抽象。

---

## 1. crate 目录模板(六件套)

```
crates/<vendor-crate>/
├─ Cargo.toml            版本继承 workspace;description/keywords/categories 面向 crates.io
├─ examples/
│  └─ smoke.rs           真账号端到端冒烟(读环境变量凭证)
└─ src/
   ├─ lib.rs             模块声明 + 公开 re-export + crate 级文档
   ├─ error.rs           XxxError(thiserror,#[non_exhaustive]);Core 透传 + Api 业务错误
   ├─ sign.rs            该家签名组装(★最易翻车,必须有官方向量单测)
   ├─ client.rs          XxxClient:凭证 + endpoint 规整 + url 拼接
   ├─ object.rs          对象操作:put/get/delete/head + 私有签名请求构造器
   ├─ bucket.rs          桶操作:list_objects(分页)、list/create/delete bucket
   └─ multipart.rs       分片上传:initiate/upload_part/complete/abort + 高层封装
```

---

## 2. 增量开发顺序(每个都是一次独立提交)

严格按此顺序,一次只做一个,**每步做完必须本地全绿并让用户验证后再提交**:

| # | 增量 | 关键产出 | 验收方式 |
|---|------|---------|---------|
| 1 | crate 骨架 | Cargo.toml + 空 lib.rs,加入 workspace members | `cargo metadata` 通过 |
| 2 | **签名** `sign.rs` | 签名串构造 + HMAC + Authorization | ★**官方文档示例向量**单测逐字节相等 |
| 3 | `client.rs` | 构造、endpoint 规整、bucket url | endpoint/url 单测 |
| 4 | `error.rs` | XxxError + 错误响应解析 | 错误 XML/JSON 解析单测 |
| 5 | 对象操作 `object.rs` | put/get/delete/head | 离线:请求 URL + 签名头逐字节比对 |
| 6 | 列举对象 `bucket.rs` | list_objects + `cloud_core::paginate` | 离线:签名/查询/XML 解析 + 翻页游标 |
| 7 | 桶管理 | list/create/delete bucket | 离线:service/bucket-root 签名 |
| 8 | 分片上传 `multipart.rs` | 四件套 + 高层 + **子资源签名** | 离线:子资源排序签名 + complete body |
| 9 | `examples/smoke.rs` | 真账号跑通全链路 | ★**用户用真实账号运行通过** |

> 分片上传通常需要扩展 `sign.rs` 支持**子资源**(计入 CanonicalizedResource 的特殊参数)。

---

## 3. 每一步的验收硬标准

一个增量在提交前必须**同时**满足:

```bash
cargo fmt --all -- --check                                   # 格式
cargo clippy --workspace --all-targets --all-features -- -D warnings   # 零告警
cargo test -p <vendor-crate>                                 # 全绿
```

即本仓 CI 的三项检查(见 `.github/workflows/ci.yml`)。**签名步骤额外要求**:必须用该厂商
官方文档给出的示例(AK/SK + 预期签名)写一个单测,验证签名逐字节相等——这是唯一能离线证明
签名正确的手段,不允许跳过。

## 4. 离线 vs 联网验证的边界(重要)

- **能离线验证的**:签名(官方向量)、请求 URL/头组装、查询参数、错误/列表 XML 解析、
  分页游标推导、分片 complete body 生成。→ 用单测覆盖。
- **只能联网验证的**:真正的 put/get/list/multipart 往返。→ 用 `examples/smoke.rs`,
  由掌握真实账号的人运行。密钥只走**环境变量**,严禁写进代码或提交。

`smoke.rs` 标准流程:`list_buckets → put → head → list_objects → get(逐字节校验)→
delete → multipart 上传(强制多片)+ 校验 + 清理`,每一条**新增的签名路径**都要在 smoke 里被走到。

## 5. 提交规范

- 走 `git-commit-convention` skill;**Conventional Commits,英文,祈使句**。
- 一个增量一个提交;`feat(<crate>): ...` 为主,示例/冒烟用 `test(<crate>): ...`。
- 作者:`qianmoQ <shicheng@devlive.org>`(仓库本地已配置)。
- 不含 AI 署名、不含中文。

## 6. 发布到 crates.io(SDK 成熟后)

1. 查名:目标名(如 `tencent-cos`)可能被占,被占则用 `-rs` 后缀。
2. `Cargo.toml` 补全 `description` / `keywords` / `categories` / `readme`。
3. 公开类型用 `#[non_exhaustive]` 预留扩展,避免破坏性版本。
4. `cargo publish --dry-run -p <crate>` 预检,再正式 `cargo publish`。
5. `cloud-core` 是公共依赖,其大版本升级会牵动所有 SDK,谨慎。

## 7. 新建一个 SDK 的启动清单

- [ ] 复制《厂商规格卡》模板到 `docs/sdk/<crate>.md`,填厂商特有参数
- [ ] 建 crate 骨架并加入 workspace `members`
- [ ] 按第 2 节顺序逐个增量开发,每步满足第 3 节验收
- [ ] `smoke.rs` 真账号跑通
- [ ] 更新对应规格卡的进度勾选
