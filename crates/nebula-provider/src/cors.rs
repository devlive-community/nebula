//! Bucket CORS 规则模型。

use serde::{Deserialize, Serialize};

/// 一条 CORS 规则。
///
/// 对应各家云 `PUT /?cors` 的一条 `CORSRule`;读写都是**整套替换**语义。
/// 只建模 S3/OSS/OBS/COS 的公共子集,不包含 OSS/COS 额外的桶级 `ResponseVary` 开关。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CorsRule {
    /// 规则 ID;可留空,由云端生成或直接忽略。
    pub id: Option<String>,
    pub allowed_origins: Vec<String>,
    pub allowed_methods: Vec<String>,
    pub allowed_headers: Vec<String>,
    pub expose_headers: Vec<String>,
    /// 预检请求结果的浏览器缓存时间(秒)。
    pub max_age_seconds: Option<u32>,
}
