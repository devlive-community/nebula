//! 各家云签名共用的加密 / 编码原语。
//!
//! 这里只提供无状态的纯函数积木(HMAC / 摘要 / 编码);
//! 每家云"如何用这些积木拼出签名"放在各自 SDK 的 `sign.rs`,不在这里抽象。

use base64::Engine as _;
use hmac::{Hmac, Mac};
use md5::Md5;
use sha1::Sha1;
use sha2::{Digest, Sha256};

type HmacSha1 = Hmac<Sha1>;
type HmacSha256 = Hmac<Sha256>;

/// HMAC-SHA1,返回原始字节。用于阿里云 OSS 等专有签名。
pub fn hmac_sha1(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha1::new_from_slice(key).expect("HMAC accepts key of any size");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

/// HMAC-SHA256,返回原始字节。用于腾讯 TC3 / 阿里 ACS3 / AWS SigV4 等。
pub fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts key of any size");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

/// HMAC-SHA256 的小写十六进制字符串形式。
pub fn hmac_sha256_hex(key: &[u8], data: &[u8]) -> String {
    hex::encode(hmac_sha256(key, data))
}

/// SHA-256 摘要的小写十六进制字符串。用于 canonical request 的 payload hash。
pub fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

/// 标准 base64(带 padding)。
pub fn base64_encode(data: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(data)
}

/// Content-MD5:对内容取 MD5 原始字节后 base64。OSS/COS 上传校验用。
pub fn content_md5(data: &[u8]) -> String {
    let digest = Md5::digest(data);
    base64_encode(&digest)
}

/// 小写十六进制编码。
pub fn hex_encode(data: &[u8]) -> String {
    hex::encode(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    // RFC 2202 HMAC-SHA1 测试向量 #2:
    // key = "Jefe", data = "what do ya want for nothing?"
    #[test]
    fn hmac_sha1_rfc2202_case2() {
        let mac = hmac_sha1(b"Jefe", b"what do ya want for nothing?");
        assert_eq!(hex::encode(mac), "effcdf6ae5eb2fa2d27416d5f184df9c259a7c79");
    }

    // RFC 4231 HMAC-SHA256 测试向量 #2(同样的 key/data)。
    #[test]
    fn hmac_sha256_rfc4231_case2() {
        let mac = hmac_sha256(b"Jefe", b"what do ya want for nothing?");
        assert_eq!(
            hex::encode(mac),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    // NIST 空串 SHA-256。
    #[test]
    fn sha256_empty() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn sha256_abc() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn base64_basic() {
        assert_eq!(base64_encode(b"hello"), "aGVsbG8=");
    }

    // Content-MD5 of empty payload — 广泛用作已知向量。
    #[test]
    fn content_md5_empty() {
        assert_eq!(content_md5(b""), "1B2M2Y8AsgTpgAmY7PhCfg==");
    }
}
