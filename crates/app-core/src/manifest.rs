//! 文件夹清单导出:把一个文件夹下所有对象列成 CSV(路径 / 大小 / 时间 / 存储类 / ETag),
//! 用于盘点、审计、离线归档。CSV 生成是纯函数,便于测试转义等边界。

use nebula_provider::Entry;

use crate::{App, Result};

/// RFC 4180 字段转义:含逗号 / 引号 / 换行时用引号包裹并把内部引号翻倍。
fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// 把文件条目列表转成 CSV 文本(含表头)。纯函数。
pub fn entries_to_csv(entries: &[Entry]) -> String {
    let mut out = String::from("path,size,last_modified,storage_class,etag\n");
    for e in entries {
        out.push_str(&csv_field(&e.path));
        out.push(',');
        out.push_str(&e.size.to_string());
        out.push(',');
        out.push_str(&csv_field(e.last_modified.as_deref().unwrap_or("")));
        out.push(',');
        out.push_str(&csv_field(e.storage_class.as_deref().unwrap_or("")));
        out.push(',');
        out.push_str(&csv_field(e.etag.as_deref().unwrap_or("")));
        out.push('\n');
    }
    out
}

impl App {
    /// 递归列举 `root` 下所有对象,生成 CSV 清单文本。
    pub async fn folder_manifest_csv(&self, account: &str, root: &str) -> Result<String> {
        let files = self.list_all_files(account, root).await?;
        Ok(entries_to_csv(&files))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_and_rows() {
        let mut e = Entry::file("a/b.txt", 123);
        e.storage_class = Some("STANDARD".into());
        e.etag = Some("abc".into());
        let csv = entries_to_csv(&[e]);
        assert!(csv.starts_with("path,size,last_modified,storage_class,etag\n"));
        assert!(csv.contains("a/b.txt,123,,STANDARD,abc"));
    }

    #[test]
    fn escapes_special_chars() {
        let e = Entry::file("has,comma and \"quote\".txt", 1);
        let csv = entries_to_csv(&[e]);
        // 含逗号与引号 → 整体加引号,内部引号翻倍。
        assert!(csv.contains("\"has,comma and \"\"quote\"\".txt\""));
    }

    #[test]
    fn empty_list_is_header_only() {
        assert_eq!(
            entries_to_csv(&[]),
            "path,size,last_modified,storage_class,etag\n"
        );
    }
}
