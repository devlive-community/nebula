//! 腾讯云 COS 专有签名(`q-sign-algorithm=sha1`,基于 HMAC-SHA1)。
//!
//! 签名链:
//! ```text
//! SignKey      = hex(HMAC-SHA1(SecretKey, KeyTime))
//! HttpString   = method(小写)\nUriPath\nHttpParameters\nHttpHeaders\n
//! StringToSign = "sha1\n{KeyTime}\n" + hex(SHA1(HttpString)) + "\n"
//! Signature    = hex(HMAC-SHA1(SignKey<作为字符串>, StringToSign))
//! ```
//! `Authorization: q-sign-algorithm=sha1&q-ak={SecretId}&q-sign-time={KeyTime}&q-key-time={KeyTime}
//!  &q-header-list={hl}&q-url-param-list={ul}&q-signature={sig}`
//!
//! 参考:COS《请求签名》。加密原语复用 [`cloud_core::crypto`]。

use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};

/// COS 的 urlencode:除 `A-Za-z0-9 - _ . ~` 外全部编码(大写十六进制)。
const COS_ENC: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');

fn enc(s: &str) -> String {
    utf8_percent_encode(s, COS_ENC).to_string()
}

/// 把头 / 参数整理成 COS canonical:返回 `(list, string)`。
///
/// `list` = 参与签名的键(小写)按字典序用 `;` 连接;
/// `string` = `key(小写,urlencode)=urlencode(value)` 按键字典序用 `&` 连接。
pub fn canonical(pairs: &[(&str, &str)]) -> (String, String) {
    let mut items: Vec<(String, String)> = pairs
        .iter()
        .map(|(k, v)| (enc(&k.to_ascii_lowercase()), enc(v)))
        .collect();
    items.sort_by(|a, b| a.0.cmp(&b.0));
    let list = items
        .iter()
        .map(|(k, _)| k.clone())
        .collect::<Vec<_>>()
        .join(";");
    let string = items
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&");
    (list, string)
}

/// `SignKey = hex(HMAC-SHA1(SecretKey, KeyTime))`。
pub fn sign_key(secret_key: &str, key_time: &str) -> String {
    cloud_core::crypto::hex_encode(&cloud_core::crypto::hmac_sha1(
        secret_key.as_bytes(),
        key_time.as_bytes(),
    ))
}

/// 组装 HttpString:`method(小写)\nUriPath\nHttpParameters\nHttpHeaders\n`。
/// `param_string` / `header_string` 为 [`canonical`] 返回的第二项(可为空串)。
pub fn http_string(
    method: &str,
    uri_path: &str,
    param_string: &str,
    header_string: &str,
) -> String {
    format!(
        "{}\n{uri_path}\n{param_string}\n{header_string}\n",
        method.to_ascii_lowercase()
    )
}

/// 由 HttpString 的 SHA1 摘要拼 StringToSign:`sha1\n{KeyTime}\n{http_sha1_hex}\n`。
pub fn string_to_sign_from_digest(key_time: &str, http_sha1_hex: &str) -> String {
    format!("sha1\n{key_time}\n{http_sha1_hex}\n")
}

/// 组装 StringToSign(内部对 `http_string` 做 SHA1)。
pub fn string_to_sign(key_time: &str, http_string: &str) -> String {
    string_to_sign_from_digest(
        key_time,
        &cloud_core::crypto::sha1_hex(http_string.as_bytes()),
    )
}

/// `Signature = hex(HMAC-SHA1(SignKey 作为字符串, StringToSign))`。
pub fn signature(sign_key: &str, string_to_sign: &str) -> String {
    cloud_core::crypto::hex_encode(&cloud_core::crypto::hmac_sha1(
        sign_key.as_bytes(),
        string_to_sign.as_bytes(),
    ))
}

/// 组装完整 `Authorization` 头值。
pub fn authorization(
    secret_id: &str,
    key_time: &str,
    header_list: &str,
    url_param_list: &str,
    signature: &str,
) -> String {
    format!(
        "q-sign-algorithm=sha1&q-ak={secret_id}&q-sign-time={key_time}&q-key-time={key_time}\
         &q-header-list={header_list}&q-url-param-list={url_param_list}&q-signature={signature}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // 官方向量 ①:腾讯 COS《请求签名》示例的 StringToSign 格式逐字节相等(SK 打码,故只锚定格式)。
    #[test]
    fn official_string_to_sign_format() {
        assert_eq!(
            string_to_sign_from_digest(
                "1557989151;1557996351",
                "8b2751e77f43a0995d6e9eb9477f4b685cca4172"
            ),
            "sha1\n1557989151;1557996351\n8b2751e77f43a0995d6e9eb9477f4b685cca4172\n"
        );
    }

    // 官方向量 ②:参考实现(Python hmac/sha1)自造的完整链路,逐字节对齐。
    // SK=MySecretKey123 KeyTime=1600000000;1600003600
    // PUT /exampleobject,头 content-type=text/plain & host=bkt-123.cos.ap-beijing.myqcloud.com
    #[test]
    fn reference_full_chain() {
        let (hlist, hstr) = canonical(&[
            ("host", "bkt-123.cos.ap-beijing.myqcloud.com"),
            ("Content-Type", "text/plain"),
        ]);
        assert_eq!(hlist, "content-type;host");
        assert_eq!(
            hstr,
            "content-type=text%2Fplain&host=bkt-123.cos.ap-beijing.myqcloud.com"
        );

        let http = http_string("PUT", "/exampleobject", "", &hstr);
        assert_eq!(
            http,
            "put\n/exampleobject\n\ncontent-type=text%2Fplain&host=bkt-123.cos.ap-beijing.myqcloud.com\n"
        );
        assert_eq!(
            cloud_core::crypto::sha1_hex(http.as_bytes()),
            "b75fd77206db51c7aed035ecdc2f0b08baafc8e9"
        );

        let sk = sign_key("MySecretKey123", "1600000000;1600003600");
        assert_eq!(sk, "44fed2277a832d557a4f6b9a335305ddec577b77");

        let sts = string_to_sign("1600000000;1600003600", &http);
        assert_eq!(
            sts,
            "sha1\n1600000000;1600003600\nb75fd77206db51c7aed035ecdc2f0b08baafc8e9\n"
        );

        assert_eq!(
            signature(&sk, &sts),
            "b57dda064fada1748de7a9540ccae37391c3cd02"
        );
    }

    #[test]
    fn authorization_format() {
        let auth = authorization("SID", "1;2", "host", "", "SIG");
        assert_eq!(
            auth,
            "q-sign-algorithm=sha1&q-ak=SID&q-sign-time=1;2&q-key-time=1;2\
             &q-header-list=host&q-url-param-list=&q-signature=SIG"
        );
    }

    #[test]
    fn canonical_sorts_and_urlencodes() {
        let (list, s) = canonical(&[("x-cos-acl", "public-read"), ("Host", "b.com")]);
        assert_eq!(list, "host;x-cos-acl");
        assert_eq!(s, "host=b.com&x-cos-acl=public-read");
    }
}
