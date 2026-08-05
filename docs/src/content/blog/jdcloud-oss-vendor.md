---
title: 官方文档不肯说签名版本，第三方 SDK 的源码替它说了
date: 2026-08-05
author: Nebula Team
description: 金山云 KS3 因为找不到可验证的官方签名向量搁置之后，接下来查京东云对象存储时又撞上类似的墙——官方文档翻遍了也没有一句话明确写"这是 SigV4"。但这次没有卡住：一个第三方 PHP 客户端的源码显示它直接实例化官方 AWS SDK 的 S3Client 指向京东云，这比任何一句文档描述都更有说服力。
tags: ['开发', '对象存储', '架构']
---

UCloud US3 接完之后,同一档"专有签名"厂商里还剩金山云 KS3(搁置,缺可验证的官方签名向量)和
京东云。查京东云对象存储(OSS)的时候,一开始像是要重蹈 KS3 的覆辙——官方文档的"签名认证"
页面翻了好几次,始终没有一句话明确写"这是 AWS Signature Version 4"还是自定义的 V2 类算法。

## 文档说不清楚,但有人真的拿标准 AWS SDK 跑通过

转向找第三方客户端库作为佐证时,找到一个 PHP 包 `jsuphp/jdcloud-oss`。它的 `composer.json`
里没有声明依赖 `aws/aws-sdk-php`,一开始以为它是自己实现的签名器——结果看源码发现完全相反:

```php
new \Aws\S3\S3Client([
    'version' => 'latest',
    'region' => $config['region'],
    'endpoint' => $config['endpoint'],   // s3.cn-south-1.jdcloud-oss.com
    'signature_version' => 'v4',
    'credentials' => [
        'key'    => $config['access_key_id'],
        'secret' => $config['access_key_secret'],
    ],
]);
```

这个包本质上就是官方 AWS SDK 的一层配置封装,`composer.json` 里没写 `aws/aws-sdk-php` 依赖
只是因为它把这行依赖漏标了(常见的第三方小包疏漏),不代表它没用。这行代码是比任何一句文档
描述都更硬的证据:**有真实开发者拿着官方 AWS SDK、显式声明 `signature_version=v4`,指向京东云
的 endpoint,而且发布成了一个能跑的包**。如果京东云的签名和标准 SigV4 有任何偏差,这个包根本
不可能工作,也不会有人把它发出来。

## 京东云的 endpoint 恰好也是最简单的形状

`s3.{region}.jdcloud-oss.com`(比如 `s3.cn-south-1.jdcloud-oss.com`)——和 AWS 的
`s3.{region}.amazonaws.com` 同形状,现成的 `parse_region` 不用改一行就能解析对,是纯粹的
`pub use` 门面,不需要像 UCloud US3(`s3-` 连字符前缀)或 DigitalOcean Spaces(完全没有 `s3`
前缀)那样另写构造函数。五家 S3 兼容云接下来,这是第三家能直接复用 `S3Client::new` 的(另外
两家是 AWS 本身和 Backblaze B2/Wasabi/Scaleway)。

## 能力矩阵:官方产品功能页面挨个点过去,唯独版本控制没找到

京东云 OSS 的产品功能文档给生命周期管理、跨域访问设置(CORS)、静态网站托管、对象标签、存储
类型转换、分片拷贝都开了独立的文档页面,逐项对上 Nebula 现有能力位,全部标支持。对象 ACL
确认三态(`private`/`public-read`/`public-read-write`),是现有二态实现的超集——中间有个
小插曲:查到一条资料说"京东云的 Put Bucket 接口不支持 `x-amz-acl` 头,无法用 canned ACL 建桶
时指定权限",一度以为 ACL 要标不支持。查清楚后发现这条限制针对的是**建桶时**指定 ACL,而
Nebula 用的是 `PutObjectAcl`(`?acl` 子资源,对象级、独立接口)不受影响——两个不同的接口,不能
因为一个不支持就连带怀疑另一个。

唯独版本控制,翻遍产品功能列表也没找到独立的文档页面。功能列表里有"跨区域复制"这一项,而
跨区域复制在大多数 S3 系统里依赖版本控制作为前提条件,这算是个间接线索,但不是直接证据——不能
凭一个可能相关的功能存在,就代入一个没有直接确认过的能力位。这次继续按"没查到证据就不开"的
既有原则处理,标不支持。

## 小结

这轮的收获不是"京东云支持什么",是找证据的方法论又多了一条路:**官方文档语焉不详时,一个
真实存在、能正常工作的第三方客户端库的源码,是比文档描述更可信的证据**——毕竟文档可能写得
不完整,但一段真的被人在生产环境里跑通、发布出来的代码,撒谎的成本要高得多。金山云那次没有
这样的旁证可用(没找到任何第三方库或代码样例能反过来验证签名细节),这次刚好有,是运气,不是
方法本身能保证每次都找得到。金山云和更长尾的其它专有签名厂商,还是得等到能验证的向量或者
真实账号才动手。
