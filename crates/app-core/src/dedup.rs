//! 秒传:上传前判断远端对象是否已与本地文件内容一致,若一致则跳过上传。
//!
//! 判据复用完整性校验的 ETag 语义——只有当远端 ETag 是**整对象 MD5**(非分片、非缺失)、
//! 且大小与本地一致时,才流式计算本地 MD5 做最终比对。因此只在真正可能相同时才付出读盘代价,
//! 且判断是**保守**的:任何无法确认相同的情形都照常上传,绝不会因误判而漏传。

use tokio::io::AsyncReadExt;

use crate::Result;

/// 流式计算本地文件的 MD5(小写十六进制),不一次性读入内存。
pub(crate) async fn file_md5(path: &str) -> Result<String> {
    let mut f = tokio::fs::File::open(path).await?;
    let mut hasher = cloud_core::crypto::Md5Hasher::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finish())
}

/// 依据远端 ETag / 大小与本地大小,判断是否**需要**读本地算 MD5 再比对。
/// 返回 `Some(expected_md5)` 表示"值得比对"(远端为整对象 MD5 且大小一致);
/// `None` 表示直接判定为"需上传"(远端缺失、分片 ETag、或大小不同)。纯逻辑,便于测试。
pub(crate) fn worth_comparing(
    remote_etag: Option<&str>,
    remote_size: u64,
    local_size: u64,
) -> Option<String> {
    if remote_size != local_size {
        return None;
    }
    remote_etag.and_then(crate::integrity::etag_as_md5)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ABC_MD5: &str = "900150983cd24fb0d6963f7d28e17f72";

    #[test]
    fn size_mismatch_short_circuits() {
        assert_eq!(worth_comparing(Some(ABC_MD5), 3, 4), None);
    }

    #[test]
    fn multipart_or_missing_etag_is_not_worth_comparing() {
        let mp = format!("\"{ABC_MD5}-2\"");
        assert_eq!(worth_comparing(Some(&mp), 3, 3), None);
        assert_eq!(worth_comparing(None, 3, 3), None);
    }

    #[test]
    fn plain_md5_with_equal_size_yields_expected() {
        assert_eq!(
            worth_comparing(Some(ABC_MD5), 3, 3),
            Some(ABC_MD5.to_string())
        );
    }
}
