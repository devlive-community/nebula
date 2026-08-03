//! 华为云 OBS 专有签名(Header 方式,HMAC-SHA1,V2 风格)。
//!
//! 签名串(StringToSign)构造:
//! ```text
//! VERB + "\n"
//! + Content-MD5 + "\n"
//! + Content-Type + "\n"
//! + Date + "\n"
//! + CanonicalizedHeaders
//! + CanonicalizedResource
//! ```
//! `Authorization: OBS {AccessKey}:{base64(HMAC-SHA1(SecretKey, StringToSign))}`
//!
//! 与阿里云 OSS 同构,差异仅在:canonical 头前缀 `x-obs-`、授权词 `OBS`。
//!
//! > ⚠ 华为官方文档把 StringToSign 写成 `...CanonicalizedHeaders + "\n" + CanonicalizedResource`
//! > (头块与资源间多一个换行),但**官方 OBS Python/Java SDK** 里每个 `x-obs-` 头行自带行尾
//! > `\n`、头块与资源间无额外换行(与 OSS/S3 V2 一致)。这里以 SDK 行为为准。
//!
//! 参考:OBS《在头域中携带签名》。加密原语复用 [`cloud_core::crypto`]。

use std::collections::BTreeMap;

/// 把请求头中的 `x-obs-*` 头整理成 CanonicalizedHeaders。
///
/// 规则:头名转小写、只保留 `x-obs-` 前缀、按字典序排序、每项 `key:value\n`。
/// 若没有任何 `x-obs-` 头,返回空串。
pub fn canonicalized_obs_headers<'a, I>(headers: I) -> String
where
    I: IntoIterator<Item = (&'a str, &'a str)>,
{
    let mut sorted: BTreeMap<String, String> = BTreeMap::new();
    for (key, value) in headers {
        let lower = key.to_ascii_lowercase();
        if lower.starts_with("x-obs-") {
            sorted.insert(lower, value.trim().to_string());
        }
    }

    let mut out = String::new();
    for (key, value) in sorted {
        out.push_str(&key);
        out.push(':');
        out.push_str(&value);
        out.push('\n');
    }
    out
}

/// OBS 签名中被视为"子资源"、需计入 CanonicalizedResource 的参数键。
///
/// 必须涵盖所有我们会发出的子资源:分片上传(`uploads`/`uploadId`/`partNumber`)、
/// 对象标签(`tagging`)、归档取回(`restore`)、bucket 生命周期规则(`lifecycle`)、
/// 版本控制(`versioning`/`versions`/`versionId`)、CORS 规则(`cors`)、静态网站托管
/// (`website`)。漏掉任何一个都会导致我们签名时把它从 CanonicalizedResource 里过滤掉,
/// 而请求 URL 仍带着它 → 服务端算出不同签名 → 签名不匹配。OBS 完整子资源集很大
/// (`acl`……),用到再补。
const SUBRESOURCE_KEYS: &[&str] = &[
    "uploads",
    "uploadId",
    "partNumber",
    "tagging",
    "restore",
    "lifecycle",
    "versioning",
    "versions",
    "versionId",
    "cors",
    "website",
];

/// 构造带子资源的 CanonicalizedResource,如 `/bucket/key?partNumber=1&uploadId=xxx`。
///
/// `params` 里非子资源的普通查询参数会被忽略(它们不参与签名);子资源按键的字典序
/// 排序,无值的(如 `uploads`)只保留键名。
pub fn canonicalized_resource(bucket: &str, key: &str, params: &[(&str, Option<&str>)]) -> String {
    let mut subs: Vec<(&str, Option<&str>)> = params
        .iter()
        .filter(|(k, _)| SUBRESOURCE_KEYS.contains(k))
        .copied()
        .collect();
    subs.sort_by(|a, b| a.0.cmp(b.0));

    let base = format!("/{bucket}/{key}");
    if subs.is_empty() {
        return base;
    }
    let joined = subs
        .iter()
        .map(|(k, v)| match v {
            Some(v) => format!("{k}={v}"),
            None => (*k).to_string(),
        })
        .collect::<Vec<_>>()
        .join("&");
    format!("{base}?{joined}")
}

/// 拼装完整的 StringToSign。
///
/// `content_md5` / `content_type` 缺省时传空串。`canonicalized_obs_headers`
/// 应是 [`canonicalized_obs_headers`] 的返回值(每行自带结尾 `\n`,可能为空)。
/// `canonicalized_resource` 形如 `/bucket/object`(可含排序后的子资源)。
pub fn string_to_sign(
    method: &str,
    content_md5: &str,
    content_type: &str,
    date: &str,
    canonicalized_obs_headers: &str,
    canonicalized_resource: &str,
) -> String {
    format!(
        "{method}\n{content_md5}\n{content_type}\n{date}\n{canonicalized_obs_headers}{canonicalized_resource}"
    )
}

/// 对 StringToSign 计算签名:`base64(HMAC-SHA1(secret, string_to_sign))`。
pub fn signature(secret_key: &str, string_to_sign: &str) -> String {
    let mac = cloud_core::crypto::hmac_sha1(secret_key.as_bytes(), string_to_sign.as_bytes());
    cloud_core::crypto::base64_encode(&mac)
}

/// 生成完整的 `Authorization` 头值:`OBS {AccessKey}:{Signature}`。
pub fn authorization(access_key: &str, secret_key: &str, string_to_sign: &str) -> String {
    format!(
        "OBS {}:{}",
        access_key,
        signature(secret_key, string_to_sign)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonicalized_headers_lowercased_sorted() {
        let headers = [
            ("X-Obs-Meta-Author", "foo@bar.com"),
            ("X-Obs-Acl", "public-read"),
            ("Content-Type", "text/html"), // 非 x-obs-,应被过滤
        ];
        let got = canonicalized_obs_headers(headers);
        assert_eq!(
            got,
            "x-obs-acl:public-read\nx-obs-meta-author:foo@bar.com\n"
        );
    }

    // 官方向量 ①:华为《在头域中携带签名》建桶示例的 StringToSign 逐字节相等。
    //
    //   PUT / HTTP/1.1
    //   Host: newbucketname2.obs.<region>.myhuaweicloud.com
    //   Date: Fri, 06 Jul 2018 03:45:51 GMT
    //   x-obs-acl:private
    //   x-obs-storage-class:STANDARD
    //   Authorization: OBS UDSIAMSTUBTEST000254:ydH8ffpcbS6YpeOMcEZfn0wE90c=
    //
    // 该文档示例把 SK 打码,无法复现最终签名;这里只断言 StringToSign 的构造格式
    // (OBS 特有的 x-obs 头 canonical + `/{bucket}/` 资源),证明格式正确。
    #[test]
    fn official_huawei_string_to_sign_format() {
        let obs_headers = canonicalized_obs_headers([
            ("x-obs-acl", "private"),
            ("x-obs-storage-class", "STANDARD"),
        ]);
        let sts = string_to_sign(
            "PUT",
            "",
            "",
            "Fri, 06 Jul 2018 03:45:51 GMT",
            &obs_headers,
            "/newbucketname2/",
        );
        assert_eq!(
            sts,
            "PUT\n\n\nFri, 06 Jul 2018 03:45:51 GMT\n\
             x-obs-acl:private\nx-obs-storage-class:STANDARD\n/newbucketname2/"
        );
        // 授权头前缀与 AK 拼接格式(签名部分因 SK 打码不校验)。
        let auth = authorization("UDSIAMSTUBTEST000254", "dummy-sk", &sts);
        assert!(auth.starts_with("OBS UDSIAMSTUBTEST000254:"));
    }

    // 官方向量 ②:AWS S3《Signing and Authenticating REST Requests》公开示例。
    // OBS V2 与 S3 V2 是同一签名算法(HMAC-SHA1 + base64,同构 StringToSign),
    // 且该向量 SK 公开,用它逐字节验证加密原语拼装正确。
    #[test]
    fn s3_v2_public_vector_signature_bytes() {
        const SK: &str = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY";
        let sts = string_to_sign(
            "GET",
            "",
            "",
            "Tue, 27 Mar 2007 19:36:42 +0000",
            "",
            "/johnsmith/photos/puppy.jpg",
        );
        assert_eq!(
            sts,
            "GET\n\n\nTue, 27 Mar 2007 19:36:42 +0000\n/johnsmith/photos/puppy.jpg"
        );
        assert_eq!(signature(SK, &sts), "bWq2s1WEIj+Ydj0vQ697zp+IXMU=");
        assert_eq!(
            authorization("AKIAIOSFODNN7EXAMPLE", SK, &sts),
            "OBS AKIAIOSFODNN7EXAMPLE:bWq2s1WEIj+Ydj0vQ697zp+IXMU="
        );
    }

    #[test]
    fn canonicalized_resource_sorts_and_formats_subresources() {
        let got = canonicalized_resource(
            "b",
            "k",
            &[("uploadId", Some("abc")), ("partNumber", Some("1"))],
        );
        assert_eq!(got, "/b/k?partNumber=1&uploadId=abc");
    }

    #[test]
    fn canonicalized_resource_keeps_valueless_and_drops_normal_params() {
        let got =
            canonicalized_resource("b", "k", &[("uploads", None), ("prefix", Some("photos/"))]);
        assert_eq!(got, "/b/k?uploads");
    }

    #[test]
    fn canonicalized_resource_without_subresources() {
        let got = canonicalized_resource("b", "k", &[("prefix", Some("x"))]);
        assert_eq!(got, "/b/k");
    }

    #[test]
    fn canonicalized_resource_signs_tagging_and_restore() {
        // 回归:标签 / 归档取回的子资源必须计入签名,否则签名不匹配。
        assert_eq!(
            canonicalized_resource("b", "k", &[("tagging", None)]),
            "/b/k?tagging"
        );
        assert_eq!(
            canonicalized_resource("b", "k", &[("restore", None)]),
            "/b/k?restore"
        );
    }

    #[test]
    fn no_obs_headers_yields_empty() {
        let sts = string_to_sign(
            "GET",
            "",
            "",
            "Tue, 27 Mar 2007 19:36:42 +0000",
            "",
            "/johnsmith/photos/puppy.jpg",
        );
        // 没有 x-obs 头时,Date 行后直接接 CanonicalizedResource。
        assert_eq!(
            sts,
            "GET\n\n\nTue, 27 Mar 2007 19:36:42 +0000\n/johnsmith/photos/puppy.jpg"
        );
    }
}
