# 厂商规格卡:aliyun-oss(阿里云 OSS)

> 配合《[SDK 开发手册](../sdk-playbook.md)》阅读。本卡只记该家**特有**参数与进度。
> 这是第一个打样 SDK,可作为其他家的对照实现。

## 基本信息

| 项 | 值 |
|----|----|
| crate 名 | `aliyun-oss` |
| 数据面 endpoint | `oss-{region}.aliyuncs.com`(如 `oss-cn-hangzhou.aliyuncs.com`) |
| 访问风格 | 虚拟托管:`https://{bucket}.{endpoint}/{key}` |
| service endpoint(列桶) | `https://{endpoint}/` |
| 凭证 | AccessKeyId + AccessKeySecret |
| 官方签名文档 | OSS《在 Header 中包含签名》 |

## 签名(sign.rs)

- 方式:Header 方式,`Authorization: OSS {AK}:{base64(HMAC-SHA1(SK, StringToSign))}`
- StringToSign:
  ```
  VERB\nContent-MD5\nContent-Type\nDate\nCanonicalizedOSSHeaders + CanonicalizedResource
  ```
- CanonicalizedOSSHeaders:`x-oss-*` 头小写、字典序、`key:value\n`
- CanonicalizedResource:`/{bucket}/{key}`,列桶为 `/`,加子资源见下
- **官方测试向量**(已用于单测):
  - AK=`44CF9590006BF252F707`,SK=`OtxrzxIsfpFjA7SwPzILwy8Bw21TLhquhboDYROV`
  - 预期签名=`26NBxoKdsyly4EDv6inkoDft/yA=`

## 子资源(分片上传等需计入签名的参数)

当前 `SUBRESOURCE_KEYS` = `uploads`、`uploadId`、`partNumber`(按字典序排序;无值键如
`uploads` 只留键名)。后续接 `acl`/`lifecycle`/`cors` 等时在此扩充。

## 错误响应

XML `<Error>`:`Code` / `Message` / `RequestId` → `OssError::Api { status, code, message, request_id }`。

## 列举分页

- ListObjects **V1**:`prefix`/`marker`/`max-keys` 为**普通查询参数,不参与签名**。
- 下一页游标 = `NextMarker`,截断且无 NextMarker 时退回本页最后一个 Key。
- 列桶 `ListAllMyBucketsResult` 同样用 marker 翻页。

## 已知取舍

- 用 V1 ListObjects(签名简单、可离线验证);未用 V2(`list-type=2` 会引入子资源签名)。
- `put_object` 为整体上传;大文件用 `upload_multipart`。

## 开发进度

- [x] 1 crate 骨架
- [x] 2 签名(官方向量单测通过)
- [x] 3 client
- [x] 4 error
- [x] 5 对象操作 put/get/delete/head(真账号验证)
- [x] 6 list_objects 分页(真账号 68k 对象验证)
- [x] 7 桶管理 list/create/delete
- [x] 8 分片上传(子资源签名)
- [x] 9 smoke 真账号跑通(list_buckets/对象/分片)

## 待办 / 后续

- [ ] 首次试发布 crates.io(查名 + `cargo publish --dry-run`)
- [ ] 分片上传并发化(当前顺序上传)
- [ ] server-side copy、断点续传下载(Range)
