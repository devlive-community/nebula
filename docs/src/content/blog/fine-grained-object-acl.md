---
title: 只给两家云做的功能，顺手在第三处代码里挖出一个签名 bug
date: 2026-08-05
author: Nebula Team
description: 细粒度对象 ACL——按具体账号 ID 授权,而不是公开/私有二选一——七家云里认真查过文档后只有 AWS S3 和华为云 OBS 值得做。写华为云那份实现时,新方法一上线就报签名不匹配,原因是半年前就留了一句"用到再补"的注释,这次终于用到了却忘了回来补。
tags: ['开发', '对象存储', '权限']
---

`set_object_acl(path, public: bool)` 只有公开读/私有两个开关,一直是个已知缺口——真实场景里
"只把这份文件分享给某个具体的协作者账号"这个需求,布尔开关满足不了。这个功能在路线图上放了
一阵子,主要是范围没定下来:七家云挨个查过官方文档,只有 AWS S3 和华为云 OBS 真支持按账号 ID
授权的对象级 ACL,腾讯云 COS 看着像支持、细看发现分享给子账号这条最实际的路走不通,剩下四家
基本都是不支持或者不生效的摆设接口。定下"只做这两家"之后,这次把它写完了。

## 两家的 XML 形状几乎一样,除了一个属性

AWS S3 的 `PutObjectAcl`/`GetObjectAcl` 和华为云 OBS 的对应接口,请求体都是
`AccessControlPolicy` → `Owner` + `AccessControlList` → 一串 `Grant`(`Grantee` + `Permission`)
这个结构,连 `Permission` 的五个枚举值(`READ`/`WRITE`/`READ_ACP`/`WRITE_ACP`/`FULL_CONTROL`)
都完全一样,不用在两家之间做值映射。唯一的差别是 `Grantee` 元素:AWS 要求带
`xsi:type="CanonicalUser"` 属性区分账号授权和预置分组授权,华为云的 `Grantee` 就只有一个
`<ID>`,没有这层区分(它的预置分组用完全不同的 `<Canned>Everyone</Canned>` 表示)。这个共性
让上层的 `Grant`/`Permission` 模型可以完全共享,只有两边各自的 XML 编解码代码不一样——和这个
仓库里"通用积木放 `nebula-provider`,每家怎么拼 XML 各写各的"这条老规矩完全一致。

## 一行签名白名单漏掉的注释,终于兑现了

写华为云 OBS 那份实现时,复用了已有的 `build_part_request`/`PartRequest` 这套"带子资源的
签名请求"抽象(标签、生命周期规则都走这条路)。传 `subresources: &[("acl", None)]` 一测,
报 `SignatureDoesNotMatch`。翻开 `sign.rs` 才发现问题:

```rust
/// OBS 完整子资源集很大(`acl`……),用到再补。
const SUBRESOURCE_KEYS: &[&str] = &[
    "uploads", "uploadId", "partNumber", "tagging", "restore",
    "lifecycle", "versioning", "versions", "versionId", "cors", "website",
];
```

`acl` 这个子资源半年前(实现公开/私有 ACL 那轮)就没进这份白名单——因为那时候
`set_object_acl` 走的是另一条更老的手工签名路径(直接拼 `format!("/{bucket}/{key}?acl")`
塞进 canonical string,不经过白名单过滤),侥幸绕开了这个坑。这次细粒度授权走的是标准的
`PartRequest` 抽象,而 `canonicalized_resource()` 会把不在白名单里的查询参数从签名串里
过滤掉——请求 URL 上还带着 `?acl`,但参与签名计算的字符串里没有,服务端按它收到的完整 URL
重新算一遍签名,两边对不上。

这正是仓库里存档过的教训("OSS/OBS 子资源白名单:发出的每个子资源都要登记")在真实开发中
兑现的一次——不是复述教训,是照着教训做检查时真的抓到了一个本来会在真实账号上炸掉的 bug。
补上 `"acl"` 之后,专门加了一个断言"用 `PartRequest` 发 `?acl` 请求时签名串确实包含
`?acl`"的回归测试,不只是让当前这个功能测试通过,防止以后又有新代码悄悄绕过白名单检查。

## 前端复用了标签编辑器的骨架

细粒度授权的编辑弹窗(`GrantsDialog.tsx`)结构上和已有的对象标签编辑器
(`TagsDialog.tsx`)几乎一样:加载现有列表 → 增删行 → 整套覆盖保存。区别只是标签编辑器
每行是"键值对"两个文本框,这里是"账号 ID 文本框 + 权限下拉框"(下拉框照例用项目里统一的
`Select` 组件,不用原生 `<select>`)。这个仓库目前没有给前端暴露 per-account 的能力位
(`Capabilities` 只在后端用来决定 trait 方法有没有实现),所以这个按钮和标签、CORS 等其它
按钮一样,对所有账号一视同仁地显示——9 家不支持的厂商点了会在弹窗里看到后端返回的
"不支持"报错,这是这个应用一直以来的既有约定,不是这次新引入的取巧。

## 小结

这次功能本身不复杂——真正花时间的是范围调研(七家云挨个查文档,逐一排除五家)和签名细节
(照着已经写过好几次的 XML 结构抄一遍)。真正的收获是"子资源签名白名单"这条
老规矩在这次真派上了用场:如果没有专门检查这份白名单,这个 bug 会一直等到有人拿真实账号
测试才会暴露——而这个仓库大部分新功能都没有真实账号可以测,能在离线阶段就抓到的 bug,
抓到就是净赚。
