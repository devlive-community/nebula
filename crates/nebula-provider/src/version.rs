//! 对象版本历史模型。

use serde::{Deserialize, Serialize};

/// 一个对象的一条历史版本(或一个删除标记)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectVersion {
    pub version_id: String,
    pub is_latest: bool,
    /// `true` 表示这不是真实内容,而是一次删除操作留下的标记。
    pub is_delete_marker: bool,
    /// 删除标记没有内容,`size` 为 0。
    pub size: u64,
    /// 删除标记没有 ETag。
    pub etag: Option<String>,
    pub last_modified: String,
}
