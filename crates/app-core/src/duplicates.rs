//! 重复文件查找:在一个文件夹下按内容签名分组,找出内容相同的对象,便于清理省存储。
//!
//! 「相同」的判据:**大小一致且 ETag 一致**(ETag 为空的对象无法判定,排除)。同一份文件
//! 用相同方式上传会得到相同 ETag(整对象 MD5,或相同分片规格下确定的分片 ETag),所以按
//! `(size, etag)` 精确分组是保守可靠的——不会把不同内容误判为重复。

use std::collections::HashMap;

use nebula_provider::Entry;
use serde::{Deserialize, Serialize};

use crate::{App, Result};

/// 一组内容相同的对象。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DupGroup {
    pub size: u64,
    pub etag: String,
    pub entries: Vec<Entry>,
    /// 可回收字节数 = (数量 - 1) × 大小(保留一份、其余为冗余)。
    pub wasted: u64,
}

/// 重复查找结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DupResult {
    pub groups: Vec<DupGroup>,
    /// 全部冗余字节合计。
    pub total_wasted: u64,
    /// 扫描过的文件数。
    pub scanned: usize,
    /// 是否因扫描上限而截断。
    pub truncated: bool,
}

/// 扫描文件数上限(避免超大桶失控)。
const MAX_SCAN: usize = 50_000;

/// 按 `(size, etag)` 把文件分组,只保留 ≥2 个成员且 ETag 非空的组,按可回收字节数降序。纯函数。
pub fn group_dupes(files: Vec<Entry>) -> Vec<DupGroup> {
    let mut map: HashMap<(u64, String), Vec<Entry>> = HashMap::new();
    for f in files {
        let etag = match &f.etag {
            Some(e) if !e.is_empty() => e.clone(),
            _ => continue, // 无 ETag,无法判定,跳过
        };
        map.entry((f.size, etag)).or_default().push(f);
    }
    let mut groups: Vec<DupGroup> = map
        .into_iter()
        .filter(|(_, v)| v.len() >= 2)
        .map(|((size, etag), mut entries)| {
            entries.sort_by(|a, b| a.path.cmp(&b.path));
            let wasted = size * (entries.len() as u64 - 1);
            DupGroup {
                size,
                etag,
                entries,
                wasted,
            }
        })
        .collect();
    groups.sort_by_key(|g| std::cmp::Reverse(g.wasted));
    groups
}

impl App {
    /// 在 `root` 下递归查找内容重复的对象。
    pub async fn find_duplicates(&self, account: &str, root: &str) -> Result<DupResult> {
        let provider = self.provider(account)?;
        let mut files: Vec<Entry> = Vec::new();
        let mut truncated = false;
        let mut queue = std::collections::VecDeque::new();
        queue.push_back(root.to_string());
        'walk: while let Some(dir) = queue.pop_front() {
            let mut cursor = None;
            loop {
                let page = provider.list_page(&dir, cursor).await?;
                for entry in page.entries {
                    if entry.is_dir() {
                        queue.push_back(entry.path);
                    } else {
                        files.push(entry);
                        if files.len() >= MAX_SCAN {
                            truncated = true;
                            break 'walk;
                        }
                    }
                }
                match page.cursor {
                    Some(next) => cursor = Some(next),
                    None => break,
                }
            }
        }
        let scanned = files.len();
        let groups = group_dupes(files);
        let total_wasted = groups.iter().map(|g| g.wasted).sum();
        Ok(DupResult {
            groups,
            total_wasted,
            scanned,
            truncated,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, size: u64, etag: Option<&str>) -> Entry {
        let mut e = Entry::file(path, size);
        e.etag = etag.map(|s| s.to_string());
        e
    }

    #[test]
    fn groups_same_size_and_etag() {
        let files = vec![
            file("a.txt", 100, Some("abc")),
            file("dir/b.txt", 100, Some("abc")),
            file("c.txt", 100, Some("different")),
            file("d.txt", 200, Some("abc")), // 同 etag 但不同大小 → 不同组
        ];
        let groups = group_dupes(files);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].entries.len(), 2);
        assert_eq!(groups[0].wasted, 100); // (2-1)*100
    }

    #[test]
    fn ignores_empty_or_missing_etag() {
        let files = vec![
            file("a", 10, None),
            file("b", 10, None),
            file("c", 10, Some("")),
        ];
        assert!(group_dupes(files).is_empty());
    }

    #[test]
    fn singletons_are_not_groups() {
        let files = vec![file("a", 10, Some("x")), file("b", 20, Some("y"))];
        assert!(group_dupes(files).is_empty());
    }

    #[test]
    fn sorted_by_wasted_desc() {
        let files = vec![
            file("small1", 10, Some("s")),
            file("small2", 10, Some("s")),
            file("big1", 1000, Some("b")),
            file("big2", 1000, Some("b")),
            file("big3", 1000, Some("b")),
        ];
        let groups = group_dupes(files);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].size, 1000); // 大的浪费更多,排前
        assert_eq!(groups[0].wasted, 2000);
        assert_eq!(groups[1].wasted, 10);
    }
}
