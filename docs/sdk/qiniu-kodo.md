# 厂商规格卡:qiniu-kodo(七牛云 Kodo,S3 兼容)

> 配合《[SDK 开发手册](../sdk-playbook.md)》阅读。本卡只记该家**特有**参数与进度。
> 七牛云 Kodo 走 **S3 兼容端点 + AWS SigV4**,签名体系与阿里云 OSS / 华为云 OBS 的 V2 不同。

## 基本信息

| 项 | 值 |
|----|----|
| crate 名 | `qiniu-kodo` |
| 数据面 endpoint | `s3.{region}.qiniucs.com`(如 `s3.cn-east-1.qiniucs.com`) |
| 访问风格 | **路径风格**:`https://s3.{region}.qiniucs.com/{bucket}/{key}` |
| service endpoint(列桶) | 同上,`GET /` |
| 凭证 | AccessKey(AK) + SecretKey(SK) |
| region 示例 | `cn-east-1` 华东-浙江 · `cn-north-1` · `cn-south-1` · `us-north-1` · `ap-southeast-1` |
| 官方签名文档 | 七牛《AWS S3 兼容》+ AWS《Signature Version 4》 |

## 签名(sign.rs)—— AWS Signature V4

- 方式:Header 方式,`Authorization: AWS4-HMAC-SHA256 Credential=.../..., SignedHeaders=..., Signature=...`
- CanonicalRequest:
  ```
  METHOD\nCanonicalURI\nCanonicalQueryString\nCanonicalHeaders\nSignedHeaders\nHashedPayload
  ```
- StringToSign:`AWS4-HMAC-SHA256\n{amzDate}\n{scope}\nhex(sha256(CanonicalRequest))`
- scope:`{YYYYMMDD}/{region}/s3/aws4_request`
- 派生签名密钥(链式 HMAC-SHA256):`AWS4+SK → date → region → s3 → aws4_request`
- 每个请求必带 `x-amz-content-sha256`(payload 的 sha256;空 body 用空串哈希 `e3b0c4…b855`)、`x-amz-date`、`host`
- **官方测试向量**(已用于单测):AWS SigV4 测试套件 **get-vanilla**
  - AK=`AKIDEXAMPLE` SK=`wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY` region=`us-east-1` service=`service`
  - date=`20150830T123600Z` → 预期签名=`5fa00fa31553b73ebf1942676e86291e8372ff2a2260956d9b8aae1d763fbf31`
  - (SigV4 是标准算法;七牛不公开自有向量,用 AWS 官方向量逐字节验证签名链正确)

## 与 OSS/OBS 的差异

- **路径风格**(bucket 在 path,不在 host);签名体系是 SigV4(SHA256 链),不是 V2(HMAC-SHA1)。
- 预签名走 SigV4 **query 方式**(`X-Amz-Algorithm/Credential/Date/Expires/SignedHeaders/Signature`)。
- region 必须显式知道(算进 scope);从 endpoint `s3.{region}.qiniucs.com` 解析。
- `sign.rs` 的 SigV4 与将来 `aws-s3`、Cloudflare R2、MinIO 通用,后续可抽共享。

## 错误响应

S3 风格 XML `<Error>`:`Code` / `Message` / `RequestId` → `KodoError::Api { status, code, message, request_id }`。

## 列举分页

ListObjects **V2**(`list-type=2`):`continuation-token` 翻页;或 V1 marker。先用可离线验证的版本。

## 已知取舍

- 走 S3 兼容端点(统一路径/桶模型,复用现有 provider 抽象),不用七牛原生 Kodo API(QBox token / 上传 token / 绑定域名)。

## 开发进度

- [x] 1 crate 骨架
- [x] 2 签名(SigV4 官方向量单测通过)
- [x] 3 client(SigV4 请求签名器 + 无依赖时间戳)
- [x] 4 error
- [x] 5 对象操作 put/get/delete/head(含 copy / SigV4 query 预签名)
- [x] 6 list_objects 分页(ListObjectsV2 + 目录折叠)
- [x] 7 桶管理 list/create/delete
- [ ] 8 分片上传
- [ ] 9 smoke 真账号跑通

## 待办 / 后续

- [ ] provider-qiniu 适配层 + app-core 注册 + GUI 选项
- [ ] 把 SigV4 抽成可被 aws-s3 / R2 / MinIO 复用的共享实现
