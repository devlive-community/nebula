//! 内容完整性校验:把本地内容的 MD5 与对象存储返回的 ETag 比对。
//!
//! 对象存储对**整对象**(非分片)上传返回的 ETag 就是内容 MD5 的十六进制值(通常带引号);
//! **分片上传**的 ETag 形如 `"<md5>-<分片数>"`,并非整体 MD5,无法用这种方式校验。
//!
//! 该判定只依赖 ETag 语义——每家 S3 兼容云与自有签名云(阿里云 OSS、华为云 OBS、
//! 腾讯云 COS、七牛云 Kodo、AWS S3、Cloudflare R2、MinIO,及以后新增的任意一家)都遵循同一约定,
//! 因此无需为任何厂商单独适配,新增厂商也自动享有完整性校验。

use cloud_core::crypto::md5_hex;
use serde::{Deserialize, Serialize};

/// 完整性校验结果。序列化为 `{ "status": "verified" | "mismatch" | "unverifiable", .. }`。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum Integrity {
    /// 本地内容 MD5 与远端 ETag 一致。
    Verified,
    /// 不一致——内容可能在传输中损坏。
    Mismatch { expected: String, actual: String },
    /// 无法用 MD5 校验(分片对象的 ETag 非整体 MD5,或对象无 ETag)。
    Unverifiable { reason: String },
}

/// 若 ETag 表示"整对象 MD5"(去引号后为 32 位十六进制、无 `-N` 分片后缀),返回其小写形式。
pub(crate) fn etag_as_md5(etag: &str) -> Option<String> {
    let e = etag.trim().trim_matches('"');
    if e.len() == 32 && e.bytes().all(|b| b.is_ascii_hexdigit()) {
        Some(e.to_ascii_lowercase())
    } else {
        None
    }
}

/// 用远端 ETag 校验一段内容的完整性。
pub fn verify_bytes(data: &[u8], etag: Option<&str>) -> Integrity {
    let Some(etag) = etag else {
        return Integrity::Unverifiable {
            reason: "对象未返回 ETag,无法校验".into(),
        };
    };
    let Some(expected) = etag_as_md5(etag) else {
        return Integrity::Unverifiable {
            reason: "分片上传对象的 ETag 非整体 MD5,无法用 MD5 校验".into(),
        };
    };
    let actual = md5_hex(data);
    if actual == expected {
        Integrity::Verified
    } else {
        Integrity::Mismatch { expected, actual }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // "abc" 的 MD5。
    const ABC_MD5: &str = "900150983cd24fb0d6963f7d28e17f72";

    #[test]
    fn verified_when_md5_matches_etag() {
        assert_eq!(verify_bytes(b"abc", Some(ABC_MD5)), Integrity::Verified);
    }

    #[test]
    fn etag_quotes_and_case_are_ignored() {
        let quoted = format!("\"{}\"", ABC_MD5.to_ascii_uppercase());
        assert_eq!(verify_bytes(b"abc", Some(&quoted)), Integrity::Verified);
    }

    #[test]
    fn mismatch_reports_both_digests() {
        match verify_bytes(b"tampered", Some(ABC_MD5)) {
            Integrity::Mismatch { expected, actual } => {
                assert_eq!(expected, ABC_MD5);
                assert_ne!(actual, ABC_MD5);
            }
            other => panic!("expected mismatch, got {other:?}"),
        }
    }

    #[test]
    fn multipart_etag_is_unverifiable() {
        // 分片 ETag 带 `-N` 后缀,长度也不是 32。
        let mp = format!("\"{ABC_MD5}-3\"");
        assert!(matches!(
            verify_bytes(b"abc", Some(&mp)),
            Integrity::Unverifiable { .. }
        ));
    }

    #[test]
    fn missing_etag_is_unverifiable() {
        assert!(matches!(
            verify_bytes(b"abc", None),
            Integrity::Unverifiable { .. }
        ));
    }
}
