//! 阿里云 OSS 专有签名(Header 方式,HMAC-SHA1)。
//!
//! 签名串(StringToSign)构造:
//! ```text
//! VERB + "\n"
//! + Content-MD5 + "\n"
//! + Content-Type + "\n"
//! + Date + "\n"
//! + CanonicalizedOSSHeaders
//! + CanonicalizedResource
//! ```
//! `Authorization: OSS {AccessKeyId}:{base64(HMAC-SHA1(AccessKeySecret, StringToSign))}`
//!
//! 参考:阿里云 OSS《在 Header 中包含签名》。加密原语复用 [`cloud_core::crypto`]。

use std::collections::BTreeMap;

/// 把请求头中的 `x-oss-*` 头整理成 CanonicalizedOSSHeaders。
///
/// 规则:头名转小写、只保留 `x-oss-` 前缀、按字典序排序、每项 `key:value\n`。
/// 若没有任何 `x-oss-` 头,返回空串。
pub fn canonicalized_oss_headers<'a, I>(headers: I) -> String
where
    I: IntoIterator<Item = (&'a str, &'a str)>,
{
    let mut sorted: BTreeMap<String, String> = BTreeMap::new();
    for (key, value) in headers {
        let lower = key.to_ascii_lowercase();
        if lower.starts_with("x-oss-") {
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

/// 拼装完整的 StringToSign。
///
/// `content_md5` / `content_type` 缺省时传空串。`canonicalized_oss_headers`
/// 应是 [`canonicalized_oss_headers`] 的返回值(每行自带结尾 `\n`,可能为空)。
/// `canonicalized_resource` 形如 `/bucket/object`(可含排序后的子资源)。
pub fn string_to_sign(
    method: &str,
    content_md5: &str,
    content_type: &str,
    date: &str,
    canonicalized_oss_headers: &str,
    canonicalized_resource: &str,
) -> String {
    format!(
        "{method}\n{content_md5}\n{content_type}\n{date}\n{canonicalized_oss_headers}{canonicalized_resource}"
    )
}

/// 对 StringToSign 计算签名:`base64(HMAC-SHA1(secret, string_to_sign))`。
pub fn signature(access_key_secret: &str, string_to_sign: &str) -> String {
    let mac =
        cloud_core::crypto::hmac_sha1(access_key_secret.as_bytes(), string_to_sign.as_bytes());
    cloud_core::crypto::base64_encode(&mac)
}

/// 生成完整的 `Authorization` 头值:`OSS {AccessKeyId}:{Signature}`。
pub fn authorization(access_key_id: &str, access_key_secret: &str, string_to_sign: &str) -> String {
    format!(
        "OSS {}:{}",
        access_key_id,
        signature(access_key_secret, string_to_sign)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // 阿里云 OSS 官方文档《在 Header 中包含签名》的经典示例。
    const AK_ID: &str = "44CF9590006BF252F707";
    const AK_SECRET: &str = "OtxrzxIsfpFjA7SwPzILwy8Bw21TLhquhboDYROV";

    #[test]
    fn canonicalized_headers_lowercased_sorted() {
        let headers = [
            ("X-OSS-Meta-Author", "foo@bar.com"),
            ("X-OSS-Magic", "abracadabra"),
            ("Content-Type", "text/html"), // 非 x-oss-,应被过滤
        ];
        let got = canonicalized_oss_headers(headers);
        assert_eq!(
            got,
            "x-oss-magic:abracadabra\nx-oss-meta-author:foo@bar.com\n"
        );
    }

    #[test]
    fn official_put_example_signature() {
        let oss_headers = canonicalized_oss_headers([
            ("X-OSS-Meta-Author", "foo@bar.com"),
            ("X-OSS-Magic", "abracadabra"),
        ]);
        let sts = string_to_sign(
            "PUT",
            "ODBGOERFMDMzQTczRUY3NUE3NzA5QzdFNUYzMDQxNEM=",
            "text/html",
            "Thu, 17 Nov 2005 18:49:58 GMT",
            &oss_headers,
            "/oss-example/nelson",
        );

        // 官方文档给出的期望签名与 Authorization 头。
        assert_eq!(signature(AK_SECRET, &sts), "26NBxoKdsyly4EDv6inkoDft/yA=");
        assert_eq!(
            authorization(AK_ID, AK_SECRET, &sts),
            "OSS 44CF9590006BF252F707:26NBxoKdsyly4EDv6inkoDft/yA="
        );
    }

    #[test]
    fn no_oss_headers_yields_empty() {
        let sts = string_to_sign(
            "GET",
            "",
            "",
            "Thu, 17 Nov 2005 18:49:58 GMT",
            "",
            "/oss-example/nelson",
        );
        // 没有 x-oss 头时,Date 行后直接接 CanonicalizedResource。
        assert_eq!(
            sts,
            "GET\n\n\nThu, 17 Nov 2005 18:49:58 GMT\n/oss-example/nelson"
        );
    }
}
