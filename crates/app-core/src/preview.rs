//! 文本预览:流式读取对象前若干字节,按 UTF-8 (lossy) 解码后交前端展示。
//!
//! 走后端读取(而非前端 fetch 预签名链接)有两个好处:绕开云端 GET 常常缺失的
//! CORS 头,并能在服务端就把体积**截断**在上限内,不为预览拉整个大文件。

use serde::Serialize;

/// 文本预览结果。
#[derive(Debug, Clone, Serialize)]
pub struct TextPreview {
    /// 已解码的文本(至多 `max_bytes` 字节的内容)。
    pub text: String,
    /// 内容超过上限、已被截断。
    pub truncated: bool,
}

/// 把一个分块累积进缓冲区,最多到 `max` 字节。返回是否发生了截断
/// (缓冲区已满且仍有未纳入的字节)。纯逻辑,便于测试。
pub(crate) fn accumulate(acc: &mut Vec<u8>, chunk: &[u8], max: usize) -> bool {
    if acc.len() >= max {
        return true;
    }
    let room = max - acc.len();
    if chunk.len() <= room {
        acc.extend_from_slice(chunk);
        false
    } else {
        acc.extend_from_slice(&chunk[..room]);
        true
    }
}

/// 按 UTF-8 (lossy) 解码为文本;非法字节替换为 U+FFFD,不会失败。
pub(crate) fn decode(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accumulate_within_cap_keeps_all() {
        let mut buf = Vec::new();
        assert!(!accumulate(&mut buf, b"hello", 10));
        assert!(!accumulate(&mut buf, b" you", 10));
        assert_eq!(buf, b"hello you");
    }

    #[test]
    fn accumulate_truncates_overflowing_chunk() {
        let mut buf = Vec::new();
        assert!(!accumulate(&mut buf, b"1234", 6));
        // 第二块只放得下 2 字节 → 截断。
        assert!(accumulate(&mut buf, b"5678", 6));
        assert_eq!(buf, b"123456");
    }

    #[test]
    fn accumulate_reports_truncation_when_already_full() {
        let mut buf = b"123456".to_vec();
        assert!(accumulate(&mut buf, b"7", 6));
        assert_eq!(buf, b"123456");
    }

    #[test]
    fn decode_replaces_invalid_utf8() {
        // 0xFF 不是合法 UTF-8 起始字节 → 替换字符,不 panic。
        let s = decode(&[b'a', 0xFF, b'b']);
        assert!(s.starts_with('a') && s.ends_with('b'));
    }
}
