//! 前缀下按存储类型的分布统计:遍历对象、按存储层(标准 / 低频 / 归档 …)累计数量与字节,
//! 供统计弹窗展示,帮助判断哪一层值得转档省钱。

use std::collections::HashMap;

use serde::Serialize;

/// 单个存储类型的小计。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ClassStat {
    /// 规范化(大写)的存储类型名;未标注的对象归为 `STANDARD`。
    pub class: String,
    pub files: u64,
    pub bytes: u64,
}

/// 一个前缀下的存储类型分布。
#[derive(Debug, Clone, Default, Serialize)]
pub struct StorageBreakdown {
    pub files: u64,
    pub bytes: u64,
    /// 各存储类型小计,按字节降序。
    pub classes: Vec<ClassStat>,
    /// 因扫描量触顶而偏小。
    pub truncated: bool,
}

/// 存储类型名规范化:去空白转大写;缺失 / 空视为标准存储。
pub(crate) fn normalize_class(storage_class: Option<&str>) -> String {
    match storage_class {
        Some(s) if !s.trim().is_empty() => s.trim().to_ascii_uppercase(),
        _ => "STANDARD".to_string(),
    }
}

/// 按存储类型累计对象数量与字节的累加器。
#[derive(Default)]
pub(crate) struct ClassTally {
    map: HashMap<String, (u64, u64)>, // class -> (files, bytes)
}

impl ClassTally {
    pub fn add(&mut self, storage_class: Option<&str>, size: u64) {
        let entry = self.map.entry(normalize_class(storage_class)).or_default();
        entry.0 += 1;
        entry.1 += size;
    }

    /// 汇总为按字节降序(同字节按类型名升序)的小计列表。
    pub fn into_sorted(self) -> Vec<ClassStat> {
        let mut classes: Vec<ClassStat> = self
            .map
            .into_iter()
            .map(|(class, (files, bytes))| ClassStat {
                class,
                files,
                bytes,
            })
            .collect();
        classes.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.class.cmp(&b.class)));
        classes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_treats_missing_and_blank_as_standard() {
        assert_eq!(normalize_class(None), "STANDARD");
        assert_eq!(normalize_class(Some("  ")), "STANDARD");
        assert_eq!(normalize_class(Some("ia")), "IA");
        assert_eq!(normalize_class(Some(" Archive ")), "ARCHIVE");
    }

    #[test]
    fn tally_groups_and_sorts_by_bytes_desc() {
        let mut t = ClassTally::default();
        t.add(Some("STANDARD"), 100);
        t.add(None, 50); // 也算 STANDARD
        t.add(Some("ARCHIVE"), 1000);
        t.add(Some("IA"), 1000); // 与 ARCHIVE 同字节 → 按类型名升序 ARCHIVE 在前
        let classes = t.into_sorted();
        assert_eq!(
            classes,
            vec![
                ClassStat {
                    class: "ARCHIVE".into(),
                    files: 1,
                    bytes: 1000
                },
                ClassStat {
                    class: "IA".into(),
                    files: 1,
                    bytes: 1000
                },
                ClassStat {
                    class: "STANDARD".into(),
                    files: 2,
                    bytes: 150
                },
            ]
        );
    }
}
