# 厂商规格卡:tencent-cos(腾讯云 COS)

> 配合《[SDK 开发手册](../sdk-playbook.md)》阅读。COS 请求/响应是 S3 风格,但**认证用腾讯专有
> 签名**(`q-sign-algorithm=sha1`,基于 HMAC-SHA1),不是 SigV4,所以**不能复用 s3-core**,
> 需从头写 `sign.rs`(类似华为 OBS 那样)。

## 基本信息

| 项 | 值 |
|----|----|
| crate 名 | `tencent-cos` |
| 数据面 endpoint | `{bucket}-{appid}.cos.{region}.myqcloud.com`(如 `bkt-123.cos.ap-beijing.myqcloud.com`) |
| 访问风格 | 虚拟托管:`https://{bucket}-{appid}.cos.{region}.myqcloud.com/{key}` |
| service endpoint(列桶) | `https://service.cos.myqcloud.com/` |
| 凭证 | SecretId + SecretKey |
| 官方签名文档 | COS《[请求签名](https://cloud.tencent.com/document/product/436/7778)》 |

## 签名(sign.rs)—— COS 专有(HMAC-SHA1)

- `KeyTime = "{startTs};{endTs}"`(签名有效期,秒级 unix 时间)
- `SignKey = hex(HMAC-SHA1(SecretKey, KeyTime))`(40 位十六进制)
- `HttpString = method(小写)\nUriPath\nHttpParameters\nHttpHeaders\n`
  - HttpParameters / HttpHeaders:`key(小写,urlencode)=urlencode(value)`,按键字典序,`&` 连接
- `StringToSign = "sha1\n{KeyTime}\n" + hex(SHA1(HttpString)) + "\n"`
- `Signature = hex(HMAC-SHA1(SignKey 作为字符串, StringToSign))`
- `Authorization = q-sign-algorithm=sha1&q-ak={SecretId}&q-sign-time={KeyTime}&q-key-time={KeyTime}&q-header-list={hl}&q-url-param-list={ul}&q-signature={sig}`
  - `hl`/`ul` = 参与签名的头 / 参数键(小写、排序)用 `;` 连接
- **测试向量**(官方示例的 SecretKey 打码,故拆两路验证):
  1. **StringToSign 格式**:用官方示例的 `KeyTime=1557989151;1557996351` + `SHA1(HttpString)=8b2751e77f43a0995d6e9eb9477f4b685cca4172`,断言 StringToSign 逐字节等于官方串。
  2. **整链**:用参考实现(Python hmac/sha1)自造一条完整向量(SK=`MySecretKey123`…),Rust 单测对齐其 SignKey / HttpString / StringToSign / Signature,逐字节相等——证明签名组装正确(HMAC-SHA1 原语已由 cloud-core RFC 向量证过)。

## 与 S3 系的差异

- 认证是 COS 专有(HMAC-SHA1 + q-sign 头),不是 SigV4;不复用 s3-core。
- endpoint 带 appid(`{bucket}-{appid}`),虚拟托管风格。

## 开发进度

- [x] 1 crate 骨架
- [x] 2 签名(参考向量 + 官方格式锚点单测通过)
- [x] 3 client(COS 签名请求器)
- [x] 4 error
- [x] 5 对象操作 put/get/delete/head(含 copy / 预签名)
- [x] 6 list_objects 分页(V1 marker + 目录折叠)
- [x] 7 桶管理 list/create/delete
- [x] 8 分片上传
- [ ] 9 smoke 真账号跑通

## 待办 / 后续

- [ ] provider-tencent 适配层 + app-core 注册 + GUI 选项
