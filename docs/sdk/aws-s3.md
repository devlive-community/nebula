# 厂商规格卡:aws-s3(AWS S3)

> 配合《[SDK 开发手册](../sdk-playbook.md)》阅读。AWS S3 与七牛云 Kodo 同为 **S3 REST + SigV4**,
> 因此**共用 `s3-core`**;本 crate 只是带 AWS 品牌的门面。签名细节见 [qiniu-kodo](./qiniu-kodo.md)。

## 基本信息

| 项 | 值 |
|----|----|
| crate 名 | `aws-s3`(`s3-core` 门面) |
| 数据面 endpoint | `s3.{region}.amazonaws.com`(如 `s3.us-west-2.amazonaws.com`) |
| 访问风格 | 路径风格:`https://s3.{region}.amazonaws.com/{bucket}/{key}` |
| 凭证 | Access Key ID + Secret Access Key |
| 签名 | AWS Signature V4(service `s3`),复用 `s3-sigv4` |

## 与 qiniu-kodo 的差异

- **仅 endpoint 域名不同**(`amazonaws.com` vs `qiniucs.com`)。对象 / 桶 / 列举 / 分片 / 预签名
  逻辑完全复用 `s3-core`,零重复代码。
- region 从 `s3.{region}.amazonaws.com` 解析;`us-east-1` 的旧全局域名 `s3.amazonaws.com`
  解析不出 region,需 `S3Client::with_region("us-east-1")`。GUI 默认给区域型 endpoint。

## 已知取舍

- 用**路径风格**(与 s3-core 一致);未用虚拟托管 `{bucket}.s3.{region}.amazonaws.com`
  (需按桶变 host,后续可在 s3-core 加开关)。

## 开发进度

- [x] 复用 s3-core:对象 / 桶 / 列举 / 分片 / 预签名(离线单测由 s3-core / s3-sigv4 覆盖)
- [x] provider-aws 适配层 + app-core 注册(`App::add_aws_account`)+ GUI 选项
- [ ] smoke 真账号跑通(`cargo run -p aws-s3 --example aws_smoke`,待用户验证)
- [ ] 可选:虚拟托管风格
