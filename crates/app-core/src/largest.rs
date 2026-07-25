//! 大文件排行:列出一个文件夹下最占空间的 Top N 对象,帮你一眼看到该清理谁(成本治理)。

use nebula_provider::Entry;
use serde::{Deserialize, Serialize};

use crate::{App, Result};

/// 大文件排行结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LargestFiles {
    /// 按大小降序的前 N 个文件。
    pub files: Vec<Entry>,
    /// 该文件夹下所有文件的总字节数(用于算每个文件的占比)。
    pub total_bytes: u64,
    /// 所有文件数。
    pub total_count: usize,
}

/// 取最大的 `top` 个文件(按大小降序;并列时按路径稳定排序)。纯函数,便于测试。
pub fn top_by_size(mut files: Vec<Entry>, top: usize) -> Vec<Entry> {
    files.sort_by(|a, b| b.size.cmp(&a.size).then_with(|| a.path.cmp(&b.path)));
    files.truncate(top);
    files
}

impl App {
    /// 列出 `root` 下最大的 `top` 个文件,并给出总大小与总数(算占比用)。
    pub async fn largest_files(
        &self,
        account: &str,
        root: &str,
        top: usize,
    ) -> Result<LargestFiles> {
        let files = self.list_all_files(account, root).await?;
        let total_bytes = files.iter().map(|f| f.size).sum();
        let total_count = files.len();
        Ok(LargestFiles {
            files: top_by_size(files, top),
            total_bytes,
            total_count,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(path: &str, size: u64) -> Entry {
        Entry::file(path, size)
    }

    #[test]
    fn takes_top_n_by_size_desc() {
        let files = vec![f("a", 10), f("b", 50), f("c", 30), f("d", 5)];
        let top = top_by_size(files, 2);
        assert_eq!(top.len(), 2);
        assert_eq!(top[0].size, 50);
        assert_eq!(top[1].size, 30);
    }

    #[test]
    fn ties_broken_by_path() {
        let top = top_by_size(vec![f("z", 10), f("a", 10)], 2);
        assert_eq!(top[0].path, "a");
        assert_eq!(top[1].path, "z");
    }

    #[test]
    fn top_larger_than_len_is_ok() {
        let top = top_by_size(vec![f("a", 1)], 10);
        assert_eq!(top.len(), 1);
    }
}
