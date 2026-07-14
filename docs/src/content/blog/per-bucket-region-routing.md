---
title: 列表是全局的,桶却是分区域的:自动把每个桶路由到它的区域
date: 2026-07-14
author: Nebula Team
description: 「列举桶」返回你所有区域的桶,但每个桶只在一个区域;用错区域的 endpoint 访问就打不开。这篇讲怎么让每个桶自动路由到它自己的区域——以及为什么 AWS 要重新签名、阿里/华为/腾讯不用。
tags: ['开发', '对象存储', '架构', '签名']
---

有个反直觉的组合会让人踩坑:**「列举桶」是全局的,但「访问桶」是分区域的**。

你调 ListBuckets,拿到名下**所有区域**的桶——列表是全的。可当你点开一个和账号默认区域**不在同一区域**的桶时,请求发到了默认区域的 endpoint,服务端就拒绝:

- AWS:`301 PermanentRedirect` / `AuthorizationHeaderMalformed`
- 阿里 OSS:`The bucket you are attempting to access must be addressed using the specified endpoint`

之前 Nebula 每个账号只钉一个 endpoint + 区域,所以跨区域的桶「列得出来、打不开」。

## 解决:每个桶自动路由到它自己的区域

思路和 aws-cli / 各家控制台一样:**探测每个桶所在的区域,缓存起来,之后该桶的请求路由到它自己的 endpoint**。全透明,不用手动切区域。

区域信息从哪来?各家不一样,但都拿得到:

| 厂商 | 区域来源 | 时机 |
|---|---|---|
| AWS | 响应头 `x-amz-bucket-region`(发错区域时 301 里带着) | 首次访问被重定向时 |
| 阿里 OSS / 华为 OBS / 腾讯 COS | 桶列表里每个桶的 `Location` / `ExtranetEndpoint` | 列举桶时就拿到了 |

所以对阿里/华为/腾讯是**主动式**:列桶时顺手把「桶 → 区域 endpoint」缓存下来,之后点开任何桶都直接连对。对 AWS 是**反应式**:首次访问跨区域桶被重定向时,从响应头学到正确区域、缓存、重试一次;之后该桶所有请求都走对区域。

## 一个关键差异:签名要不要跟着改?

把请求路由到另一个区域,**签名要不要重算**?这里 AWS 和其它三家分道扬镳,根因在签名算法:

- **AWS 用 SigV4**,签名串里**嵌了区域**(credential scope 是 `{date}/{region}/s3/aws4_request`)。区域变了,签名必须**重算**。所以 AWS 的路由既要换 endpoint、又要用新区域重新签名。
- **阿里 OSS / 华为 OBS 用 V2 签名**,CanonicalizedResource 是 `/{bucket}/{key}`——**跟 host / 区域无关**。所以换区域**只需改请求的 host,签名一个字节都不用动**。
- **腾讯 COS 用 q-sign**:它会对请求实际用的 host 签名,所以只要用正确区域的 host 去构造请求,签出来自然就是对的。

于是三家非 AWS 的实现干净得多:一个「桶 → endpoint」缓存 + 让拼 host 的那个函数查缓存,就完事了。AWS 因为 SigV4 嵌区域,多了一层「按桶解析区域 → 用该区域签名」。

```rust
// 阿里 OSS:只改 host(V2 签名与 host 无关)
pub fn bucket_base_url(&self, bucket: &str) -> String {
    let endpoint = self.bucket_endpoints.lock().unwrap()
        .get(bucket).cloned().unwrap_or_else(|| self.endpoint.clone());
    format!("https://{bucket}.{endpoint}")
}

// AWS:按桶区域路由,并用该区域重新签名(SigV4 嵌区域)
fn regional_endpoint(base: &str, region: &str) -> String {
    if base.ends_with(".amazonaws.com") { format!("s3.{region}.amazonaws.com") }
    else { base.to_string() }   // MinIO / R2 / 七牛:单一 endpoint,原样
}
```

## 只在该动的地方动

跨区域只影响 AWS(全局列桶 + 区域化 endpoint 的组合)和三家自有云。**MinIO / R2 / 七牛是「一个账号一个 endpoint」,没这个问题**——所以 s3-core 里的 endpoint 改写**只对 `*.amazonaws.com` 生效**,其它 S3 兼容服务行为完全不变,零风险。

缓存是每个账号客户端一份、进程内的:同区域的桶零额外开销,首次跨区域访问付一次「学习」代价(AWS 一次重试;阿里/华为/腾讯连重试都不用,列桶时就学到了)。

## 小结

「列表全局、访问分区域」是对象存储一个容易忽略的坑。补法是让每个桶自动带上它的区域:阿里/华为/腾讯从桶列表主动拿,AWS 从重定向响应头反应式学。真正的分水岭是签名——**SigV4 把区域写进了签名(要重算),V2 / q-sign 的签名与 host 无关(只改地址)**。理解了这一点,四家的实现该繁该简就一目了然了。
