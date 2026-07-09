//! AWS Signature V4 的纯签名函数(与具体服务无关,service 作为参数传入)。
//!
//! 签名链:
//! ```text
//! CanonicalRequest = METHOD\nURI\nQuery\nCanonicalHeaders\nSignedHeaders\nHashedPayload
//! StringToSign     = "AWS4-HMAC-SHA256\n{amzDate}\n{scope}\n" + hex(sha256(CanonicalRequest))
//! SigningKey       = HMAC(HMAC(HMAC(HMAC("AWS4"+SK, date), region), service), "aws4_request")
//! Signature        = hex(HMAC(SigningKey, StringToSign))
//! ```
//! 参考:AWS《Signature Version 4》。加密原语复用 [`cloud_core::crypto`]。

use cloud_core::crypto;

/// 签名算法标识。
pub const ALGORITHM: &str = "AWS4-HMAC-SHA256";
/// S3 服务名(计入 scope)。其它 AWS 服务传各自 service。
pub const S3: &str = "s3";
/// scope 终止串。
pub const REQUEST_TYPE: &str = "aws4_request";
/// 空 body 的 SHA-256(无请求体时的 payload hash)。
pub const EMPTY_PAYLOAD_HASH: &str =
    "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

/// 组装 CanonicalRequest。
pub fn canonical_request(
    method: &str,
    canonical_uri: &str,
    canonical_query: &str,
    canonical_headers: &str,
    signed_headers: &str,
    payload_hash: &str,
) -> String {
    format!(
        "{method}\n{canonical_uri}\n{canonical_query}\n{canonical_headers}\n{signed_headers}\n{payload_hash}"
    )
}

/// 信用范围 scope:`{date}/{region}/{service}/aws4_request`。`date` 为 `YYYYMMDD`。
pub fn credential_scope(date: &str, region: &str, service: &str) -> String {
    format!("{date}/{region}/{service}/{REQUEST_TYPE}")
}

/// 组装 StringToSign。内部对 `canonical_request` 做 sha256。
pub fn string_to_sign(amz_date: &str, scope: &str, canonical_request: &str) -> String {
    format!(
        "{ALGORITHM}\n{amz_date}\n{scope}\n{}",
        crypto::sha256_hex(canonical_request.as_bytes())
    )
}

/// 链式派生 SigV4 签名密钥。`date` 为 `YYYYMMDD`。
pub fn signing_key(secret_key: &str, date: &str, region: &str, service: &str) -> Vec<u8> {
    let k_date = crypto::hmac_sha256(format!("AWS4{secret_key}").as_bytes(), date.as_bytes());
    let k_region = crypto::hmac_sha256(&k_date, region.as_bytes());
    let k_service = crypto::hmac_sha256(&k_region, service.as_bytes());
    crypto::hmac_sha256(&k_service, REQUEST_TYPE.as_bytes())
}

/// 计算签名(小写十六进制)。
pub fn signature(signing_key: &[u8], string_to_sign: &str) -> String {
    crypto::hmac_sha256_hex(signing_key, string_to_sign.as_bytes())
}

/// 生成完整的 `Authorization` 头值。
pub fn authorization(
    access_key: &str,
    scope: &str,
    signed_headers: &str,
    signature: &str,
) -> String {
    format!(
        "{ALGORITHM} Credential={access_key}/{scope}, SignedHeaders={signed_headers}, Signature={signature}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // AWS SigV4 官方测试套件 `get-vanilla` 向量(service 名就叫 "service")。
    const AK: &str = "AKIDEXAMPLE";
    const SK: &str = "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY";

    #[test]
    fn sigv4_get_vanilla_official_vector() {
        let cr = canonical_request(
            "GET",
            "/",
            "",
            "host:example.amazonaws.com\nx-amz-date:20150830T123600Z\n",
            "host;x-amz-date",
            EMPTY_PAYLOAD_HASH,
        );
        let scope = credential_scope("20150830", "us-east-1", "service");
        let sts = string_to_sign("20150830T123600Z", &scope, &cr);
        let key = signing_key(SK, "20150830", "us-east-1", "service");
        let sig = signature(&key, &sts);

        assert_eq!(
            sig,
            "5fa00fa31553b73ebf1942676e86291e8372ff2a2260956d9b8aae1d763fbf31"
        );
        assert_eq!(
            authorization(AK, &scope, "host;x-amz-date", &sig),
            "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20150830/us-east-1/service/aws4_request, \
             SignedHeaders=host;x-amz-date, \
             Signature=5fa00fa31553b73ebf1942676e86291e8372ff2a2260956d9b8aae1d763fbf31"
        );
    }

    #[test]
    fn empty_payload_hash_matches_sha256_of_empty() {
        assert_eq!(EMPTY_PAYLOAD_HASH, crypto::sha256_hex(b""));
    }

    #[test]
    fn signing_key_varies_by_region() {
        let a = signing_key(SK, "20150830", "cn-east-1", S3);
        assert_eq!(a, signing_key(SK, "20150830", "cn-east-1", S3));
        assert_ne!(a, signing_key(SK, "20150830", "cn-north-1", S3));
    }
}
