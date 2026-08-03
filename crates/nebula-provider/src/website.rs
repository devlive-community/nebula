//! 静态网站托管配置模型。

use serde::{Deserialize, Serialize};

/// 静态网站托管配置(仅建模 S3/OSS/OBS/COS 的公共最小子集:首页 + 错误页)。
///
/// 不建模条件重定向规则、阿里云镜像回源、腾讯云 AutoAddressing 等厂商扩展字段。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebsiteConfig {
    pub index_document: String,
    pub error_document: Option<String>,
}
