//! 统一的条目模型。App 的文件浏览器直接消费 [`Entry`]。

use serde::{Deserialize, Serialize};

/// 条目类型:文件或目录(桶 / 前缀在统一模型里都表现为目录)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EntryKind {
    File,
    Directory,
}

/// 列举 / stat 返回的统一条目。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// 展示名(路径最后一段),如 `photo.jpg` 或 `photos`。
    pub name: String,
    /// 完整 provider 路径,如 `mybucket/photos/photo.jpg`。
    pub path: String,
    /// 文件还是目录。
    pub kind: EntryKind,
    /// 文件字节数;目录为 0。
    pub size: u64,
    /// 最后修改时间(原始字符串,格式随后端),目录通常为 `None`。
    pub last_modified: Option<String>,
    /// ETag,目录为 `None`。
    pub etag: Option<String>,
}

impl Entry {
    /// 构造一个目录条目(桶 / 前缀)。`path` 为其完整路径。
    pub fn directory(path: impl Into<String>) -> Self {
        let path = path.into();
        Self {
            name: last_segment(&path).to_string(),
            path,
            kind: EntryKind::Directory,
            size: 0,
            last_modified: None,
            etag: None,
        }
    }

    /// 构造一个文件条目。
    pub fn file(path: impl Into<String>, size: u64) -> Self {
        let path = path.into();
        Self {
            name: last_segment(&path).to_string(),
            path,
            kind: EntryKind::File,
            size,
            last_modified: None,
            etag: None,
        }
    }

    /// 链式设置修改时间。
    pub fn with_last_modified(mut self, value: impl Into<String>) -> Self {
        self.last_modified = Some(value.into());
        self
    }

    /// 链式设置 ETag。
    pub fn with_etag(mut self, value: impl Into<String>) -> Self {
        self.etag = Some(value.into());
        self
    }

    /// 是否为目录。
    pub fn is_dir(&self) -> bool {
        self.kind == EntryKind::Directory
    }
}

/// 取路径最后一段作为展示名(忽略结尾斜杠)。
fn last_segment(path: &str) -> &str {
    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directory_name_is_last_segment() {
        let e = Entry::directory("mybucket/photos/");
        assert_eq!(e.name, "photos");
        assert_eq!(e.path, "mybucket/photos/");
        assert!(e.is_dir());
    }

    #[test]
    fn bucket_root_directory_name() {
        let e = Entry::directory("mybucket");
        assert_eq!(e.name, "mybucket");
    }

    #[test]
    fn file_builder_sets_fields() {
        let e = Entry::file("mybucket/a/b.txt", 42)
            .with_etag("\"abc\"")
            .with_last_modified("2024-01-01T00:00:00Z");
        assert_eq!(e.name, "b.txt");
        assert_eq!(e.size, 42);
        assert_eq!(e.kind, EntryKind::File);
        assert_eq!(e.etag.as_deref(), Some("\"abc\""));
        assert_eq!(e.last_modified.as_deref(), Some("2024-01-01T00:00:00Z"));
    }
}
