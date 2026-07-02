//! provider 路径解析工具,供适配层复用,保证各家对 `bucket/key` 的解释一致。

/// 把 provider 路径拆成 `(bucket, key)`。
///
/// - `""` / `"/"` → `(None, "")`,表示根(列桶)
/// - `"bucket"` / `"bucket/"` → `(Some("bucket"), "")`
/// - `"bucket/a/b.txt"` → `(Some("bucket"), "a/b.txt")`
pub fn split(path: &str) -> (Option<&str>, &str) {
    let trimmed = path.trim_start_matches('/');
    if trimmed.is_empty() {
        return (None, "");
    }
    match trimmed.split_once('/') {
        Some((bucket, key)) => (Some(bucket), key),
        None => (Some(trimmed), ""),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_paths_have_no_bucket() {
        assert_eq!(split(""), (None, ""));
        assert_eq!(split("/"), (None, ""));
    }

    #[test]
    fn bucket_only() {
        assert_eq!(split("mybucket"), (Some("mybucket"), ""));
        assert_eq!(split("mybucket/"), (Some("mybucket"), ""));
        assert_eq!(split("/mybucket/"), (Some("mybucket"), ""));
    }

    #[test]
    fn bucket_and_key() {
        assert_eq!(split("mybucket/a/b.txt"), (Some("mybucket"), "a/b.txt"));
        assert_eq!(split("mybucket/photos/"), (Some("mybucket"), "photos/"));
    }
}
