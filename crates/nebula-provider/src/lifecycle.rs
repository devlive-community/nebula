//! Bucket 生命周期规则模型。

use serde::{Deserialize, Serialize};

/// 一条生命周期规则:按前缀匹配对象,到期后转存储类型或删除。
///
/// 对应各家云 `PUT /?lifecycle` 的一条 `Rule`;读写都是**整套替换**语义
/// (各云都是整体覆盖生命周期配置,没有增量 patch 的接口)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LifecycleRule {
    /// 规则 ID;新建时前端可传空串,由适配层生成或直接使用云端返回的 ID。
    pub id: String,
    /// 匹配前缀;空串表示整个桶。
    pub prefix: String,
    pub enabled: bool,
    /// 到期天数后删除对象;`None` 表示不设过期。
    pub expiration_days: Option<u32>,
    /// `(天数, 目标存储类型字符串)` 有序对,天数到达后转换到该存储类型。
    /// 存储类型字符串与 [`set_storage_class`](crate::StorageProvider::set_storage_class)
    /// 用的是同一套厂商原始值,不再单独定义枚举。
    pub transitions: Vec<(u32, String)>,
}
