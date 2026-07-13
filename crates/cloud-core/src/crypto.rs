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

/// SHA-1 摘要的小写十六进制字符串。用于腾讯云 COS 的 HttpString 摘要。
pub fn sha1_hex(data: &[u8]) -> String {
    let mut hasher = Sha1::new();
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

/// 内容 MD5 的小写十六进制字符串。用于与对象存储返回的 ETag(整对象上传时即为 MD5)
/// 比对做下载 / 迁移完整性校验。
pub fn md5_hex(data: &[u8]) -> String {
    hex::encode(Md5::digest(data))
}

/// 增量 MD5:反复 [`update`](Self::update) 喂入分块,最后 [`finish`](Self::finish) 得
/// 小写十六进制。用于流式对本地大文件算 MD5 而不一次性读入内存。结果等同 [`md5_hex`]。
#[derive(Default)]
pub struct Md5Hasher(Md5);

impl Md5Hasher {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn update(&mut self, data: &[u8]) {
        self.0.update(data);
    }

    pub fn finish(self) -> String {
        hex::encode(self.0.finalize())
    }
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

    // NIST SHA-1 向量:"abc" 与空串。
    #[test]
    fn sha1_vectors() {
        assert_eq!(sha1_hex(b"abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(sha1_hex(b""), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
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

    #[test]
    fn md5_hex_known_vectors() {
        assert_eq!(md5_hex(b""), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(md5_hex(b"abc"), "900150983cd24fb0d6963f7d28e17f72");
    }

    #[test]
    fn incremental_md5_matches_one_shot() {
        // 分多块喂入,结果应与一次性 md5_hex 相同。
        let mut h = Md5Hasher::new();
        h.update(b"a");
        h.update(b"b");
        h.update(b"c");
        assert_eq!(h.finish(), md5_hex(b"abc"));
    }
}
