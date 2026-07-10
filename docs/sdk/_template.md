# 厂商规格卡:<crate>(<厂商 + 产品>)

> 配合《[SDK 开发手册](../sdk-playbook.md)》阅读。新建 SDK 时复制本文件为 `docs/sdk/<crate>.md` 并填空。
> 参照已完成的 [`aliyun-oss`](./aliyun-oss.md)。

## 基本信息

| 项 | 值 |
|----|----|
| crate 名 | `<crate>` |
| 数据面 endpoint | `<...>` |
| 访问风格 | `<虚拟托管 / 路径风格,URL 模板>` |
| service endpoint(列桶) | `<...>` |
| 凭证 | `<AK/SK / token / ...>` |
| 官方签名文档 | `<链接或名称>` |

## 签名(sign.rs)

- 方式:`<Header / query;算法,如 HMAC-SHA1 / TC3-HMAC-SHA256>`
- StringToSign / CanonicalRequest 构造:
  ```
  <逐行写清楚该家的签名串结构>
  ```
- Authorization 头格式:`<...>`
- **官方测试向量**(必须填并写进单测):
  - AK=`<...>` SK=`<...>` → 预期签名=`<...>`

## 子资源 / 特殊签名参数

`<哪些查询参数需计入签名;排序规则>`

## 错误响应

`<XML/JSON 结构 → XxxError::Api 字段映射>`

## 列举分页

`<list 接口版本;哪些参数不签名;下一页游标怎么取>`

## 已知取舍

`<选了哪个 API 版本、为什么;有哪些暂不实现>`

## 开发进度

- [ ] 1 crate 骨架
- [ ] 2 签名(官方向量单测通过)
- [ ] 3 client
- [ ] 4 error
- [ ] 5 对象操作 put/get/delete/head
- [ ] 6 list_objects 分页
- [ ] 7 桶管理 list/create/delete
- [ ] 8 分片上传(含**流式** `upload_multipart_stream` + 适配层 `write_stream` 覆盖)
- [ ] 9 smoke 真账号跑通

## 待办 / 后续

- [ ] `<...>`
