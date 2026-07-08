# 厂商规格卡:huawei-obs(华为云 OBS)

> 配合《[SDK 开发手册](../sdk-playbook.md)》阅读。本卡只记该家**特有**参数与进度。
> 参照第一个打样 SDK [`aliyun-oss`](./aliyun-oss.md);OBS 的签名与 OSS 高度同构。

## 基本信息

| 项 | 值 |
|----|----|
| crate 名 | `huawei-obs` |
| 数据面 endpoint | `obs.{region}.myhuaweicloud.com`(如 `obs.cn-north-4.myhuaweicloud.com`) |
| 访问风格 | 虚拟托管:`https://{bucket}.{endpoint}/{key}` |
| service endpoint(列桶) | `https://{endpoint}/` |
| 凭证 | AccessKey(AK) + SecretKey(SK) |
| 官方签名文档 | OBS《[在头域中携带签名](https://support.huaweicloud.com/intl/zh-cn/api-obs/obs_04_0010.html)》(V2 风格) |

## 签名(sign.rs)

- 方式:Header 方式,`Authorization: OBS {AK}:{base64(HMAC-SHA1(SK, StringToSign))}`
- StringToSign(与 OSS 同构,仅前缀/授权词不同):
  ```
  VERB\nContent-MD5\nContent-Type\nDate\nCanonicalizedHeaders + CanonicalizedResource
  ```
- CanonicalizedHeaders:`x-obs-*` 头小写、字典序、每行 `key:value\n`
- CanonicalizedResource:`/{bucket}/{key}`,列桶为 `/`,加子资源见下
- **⚠ 官方文档笔误**:文档把 StringToSign 写成 `...CanonicalizedHeaders + "\n" + CanonicalizedResource`
  (头块与资源间多一个 `\n`),但**华为官方 OBS Python/Java SDK 源码**里每个 `x-obs-` 头行**自带**
  行尾 `\n`、头块与资源间**无额外换行**(与 OSS/S3 V2 一致)。以 SDK 行为为准。
- **官方测试向量**(华为文档的 SK 打码,不可复现最终签名,故拆成两个官方向量组合验证):
  1. **StringToSign 字符串**:用华为官方《在头域中携带签名》建桶示例断言逐字节相等
     (`PUT` + 两个 `x-obs-*` 头 + `/newbucketname2/`),证明 OBS 特有格式。
  2. **签名字节**:用 AWS S3《Signing REST Requests》公开向量(与 OBS V2 同算法、SK 公开)
     断言 `base64(HMAC-SHA1(SK, StringToSign))` 逐字节相等,证明加密原语拼装。
     - AK=`AKIAIOSFODNN7EXAMPLE` SK=`wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY`
     - StringToSign=`GET\n\n\nTue, 27 Mar 2007 19:36:42 +0000\n/johnsmith/photos/puppy.jpg`
     - 预期签名=`bWq2s1WEIj+Ydj0vQ697zp+IXMU=`

## 子资源(分片上传等需计入签名的参数)

当前 `SUBRESOURCE_KEYS` = `uploads`、`uploadId`、`partNumber`(按字典序;无值键只留键名)。
OBS 完整子资源集很大(`acl`/`location`/`delete`/`versioning`/`uploads`/`uploadId`/`partNumber`…),
后续接更多桶特性时在此扩充。

## 错误响应

XML `<Error>`:`Code` / `Message` / `RequestId` → `ObsError::Api { status, code, message, request_id }`
(结构与 OSS 一致)。

## 列举分页

- ListObjects(GET Bucket):`prefix`/`marker`/`max-keys`/`delimiter` 为**普通查询参数,不参与签名**
  (CanonicalizedResource 仅 `/{bucket}/`)。下一页游标 = `NextMarker`,截断且无 NextMarker 时退回本页最后一个 Key。
- 列桶 GET Service 返回 `ListAllMyBucketsResult`。

## 已知取舍 / 与 OSS 的差异

- endpoint 域名不同(`obs.{region}.myhuaweicloud.com`);canonical 头前缀 `x-obs-`;授权词 `OBS`。
- **建桶跨区域**:非默认区域需在请求体带 `<CreateBucketConfiguration><Location>{region}</Location>`;
  region 从 endpoint 解析(`obs.{region}.myhuaweicloud.com`)。
- 列举暂用普通 GET Bucket(签名简单、可离线验证)。

## 开发进度

- [x] 1 crate 骨架
- [x] 2 签名(官方向量单测通过)
- [x] 3 client
- [x] 4 error
- [x] 5 对象操作 put/get/delete/head(含 copy / 预签名)
- [x] 6 list_objects 分页(含 list_dir 目录折叠)
- [x] 7 桶管理 list/create/delete
- [x] 8 分片上传(子资源签名)
- [x] 9 smoke 真账号跑通(list_buckets/对象/分片全链路验证)

## 待办 / 后续

- [ ] 首次试发布 crates.io(查名 + `cargo publish --dry-run`)
- [x] provider-huawei 适配层 + app-core 注册(`App::add_huawei_account`)
- [ ] 分片上传并发化
