//! 文件**内容**搜索:在文本类文件与 PDF 里查关键字(不同于按文件名搜)。
//!
//! 需要下载并解析对象,代价高,故有硬上限:只扫文本类 / PDF 扩展名、跳过过大的文件、
//! 限制命中数与扫描数。PDF 文本复用 [`nebula_pdf::extract_text`],文本按 UTF-8 有损解码。

use nebula_provider::Entry;
use serde::{Deserialize, Serialize};

use crate::{App, AppError, Result};

/// 单个文件的内容命中:条目 + 匹配处的上下文片段。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentHit {
    pub entry: Entry,
    pub snippet: String,
}

/// 内容搜索结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentSearchResult {
    pub hits: Vec<ContentHit>,
    /// 实际读取解析过的文件数。
    pub scanned: usize,
    /// 是否因命中数 / 扫描上限而截断。
    pub truncated: bool,
}

/// 单个文件最大读取字节数(超过则跳过,避免下载巨物)。
const MAX_CONTENT_BYTES: u64 = 16 * 1024 * 1024;
/// 最多读取解析多少个候选文件。
const MAX_SCAN_FILES: usize = 60;

/// 可做内容搜索的文本类扩展名(小写,不含点)。PDF 单独判断。
const TEXT_EXTS: &[&str] = &[
    "txt",
    "md",
    "markdown",
    "csv",
    "tsv",
    "json",
    "log",
    "xml",
    "html",
    "htm",
    "css",
    "scss",
    "js",
    "jsx",
    "ts",
    "tsx",
    "rs",
    "py",
    "go",
    "java",
    "kt",
    "c",
    "h",
    "cpp",
    "hpp",
    "cc",
    "cs",
    "rb",
    "php",
    "swift",
    "sh",
    "bash",
    "zsh",
    "sql",
    "yaml",
    "yml",
    "toml",
    "ini",
    "conf",
    "env",
    "gitignore",
    "dockerfile",
    "makefile",
    "properties",
    "gradle",
    "vue",
    "svelte",
];

/// 取小写扩展名(无扩展名返回空)。
fn ext_of(name: &str) -> String {
    name.rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default()
}

/// 该文件是否值得做内容搜索(文本类或 PDF)。
fn is_searchable(name: &str) -> bool {
    let e = ext_of(name);
    e == "pdf" || TEXT_EXTS.contains(&e.as_str())
}

/// 在 `text` 里找 `needle`(均已小写传入 needle),命中则返回一段上下文片段(原文大小写)。
/// 片段取匹配点前后各约 60 字符,折叠连续空白,首尾按需加省略号。纯函数,便于测试。
pub fn make_snippet(text: &str, needle_lower: &str) -> Option<String> {
    if needle_lower.is_empty() {
        return None;
    }
    let lower = text.to_lowercase();
    let pos = lower.find(needle_lower)?;
    // 用字符边界安全地取窗口。
    let chars: Vec<char> = text.chars().collect();
    // pos 是字节位置;换算成字符索引。
    let char_idx = text[..pos].chars().count();
    let start = char_idx.saturating_sub(60);
    let end = (char_idx + needle_lower.chars().count() + 60).min(chars.len());
    let body: String = chars[start..end].iter().collect();
    // 折叠空白,首尾按需加省略号。
    let collapsed = body.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut snip = String::new();
    if start > 0 {
        snip.push('…');
    }
    snip.push_str(&collapsed);
    if end < chars.len() {
        snip.push('…');
    }
    Some(snip)
}

impl App {
    /// 在 `root` 下递归对文本类 / PDF 文件做**内容**搜索,返回命中文件与片段。
    pub async fn search_content(
        &self,
        account: &str,
        root: &str,
        query: &str,
        max_hits: usize,
    ) -> Result<ContentSearchResult> {
        let needle = query.trim().to_lowercase();
        if needle.is_empty() {
            return Ok(ContentSearchResult {
                hits: Vec::new(),
                scanned: 0,
                truncated: false,
            });
        }
        let provider = self.provider(account)?;

        // 先收集候选文件(文本类 / PDF,大小合规),再逐个下载解析。
        let mut candidates: Vec<Entry> = Vec::new();
        let mut queue = std::collections::VecDeque::new();
        queue.push_back(root.to_string());
        'walk: while let Some(dir) = queue.pop_front() {
            let mut cursor = None;
            loop {
                let page = provider.list_page(&dir, cursor).await?;
                for entry in page.entries {
                    if entry.is_dir() {
                        queue.push_back(entry.path);
                    } else if is_searchable(&entry.name) && entry.size <= MAX_CONTENT_BYTES {
                        candidates.push(entry);
                        if candidates.len() >= MAX_SCAN_FILES {
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

        let mut hits = Vec::new();
        let mut scanned = 0usize;
        let mut truncated = candidates.len() >= MAX_SCAN_FILES;
        for entry in candidates {
            let bytes = match provider.read(&entry.path).await {
                Ok(b) => b.to_vec(),
                Err(_) => continue, // 读不到就跳过这一个
            };
            scanned += 1;
            let text = if ext_of(&entry.name) == "pdf" {
                tokio::task::spawn_blocking(move || nebula_pdf::extract_text(&bytes))
                    .await
                    .map_err(|e| AppError::Image(e.to_string()))?
                    .map(|pages| pages.join("\n"))
                    .unwrap_or_default()
            } else {
                String::from_utf8_lossy(&bytes).into_owned()
            };
            if let Some(snippet) = make_snippet(&text, &needle) {
                hits.push(ContentHit { entry, snippet });
                if hits.len() >= max_hits {
                    truncated = true;
                    break;
                }
            }
        }
        Ok(ContentSearchResult {
            hits,
            scanned,
            truncated,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snippet_wraps_match_with_context() {
        let text = "the quick brown fox jumps over the lazy dog";
        let s = make_snippet(text, "fox").unwrap();
        assert!(s.contains("fox"));
    }

    #[test]
    fn snippet_case_insensitive() {
        assert!(make_snippet("Hello WORLD", "world").is_some());
    }

    #[test]
    fn snippet_none_when_absent() {
        assert!(make_snippet("hello", "zzz").is_none());
    }

    #[test]
    fn snippet_adds_ellipsis_for_long_text() {
        let long = "a ".repeat(200) + "needle " + &"b ".repeat(200);
        let s = make_snippet(&long, "needle").unwrap();
        assert!(s.starts_with('…') && s.ends_with('…'));
        assert!(s.contains("needle"));
    }

    #[test]
    fn searchable_extensions() {
        assert!(is_searchable("a.txt"));
        assert!(is_searchable("doc.PDF"));
        assert!(is_searchable("main.rs"));
        assert!(!is_searchable("photo.jpg"));
        assert!(!is_searchable("video.mp4"));
    }
}
