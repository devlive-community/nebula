//! 细粒度对象 ACL:按具体账号 ID 授权,而不是 [`crate::provider::StorageProvider::set_object_acl`]
//! 那种"公开/私有"二态。

use serde::{Deserialize, Serialize};

/// 授权的操作权限。
///
/// 字符串值直接对应 AWS S3 / 华为云 OBS 官方 `PutObjectAcl` XML 里的 `Permission` 枚举——
/// 两家恰好用同一套五个值,不需要做厂商间映射(这是 7 家云里唯二真支持按账号 ID 授权对象
/// 级 ACL 的两家,详见 `docs/sdk-playbook.md` 之外的路线图记录)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Permission {
    Read,
    Write,
    ReadAcp,
    WriteAcp,
    FullControl,
}

impl Permission {
    /// 对应官方 XML `<Permission>` 元素的文本值。
    pub fn as_str(self) -> &'static str {
        match self {
            Permission::Read => "READ",
            Permission::Write => "WRITE",
            Permission::ReadAcp => "READ_ACP",
            Permission::WriteAcp => "WRITE_ACP",
            Permission::FullControl => "FULL_CONTROL",
        }
    }

    /// 从官方 XML `<Permission>` 元素的文本值解析;未知值返回 `None`。
    ///
    /// 命名 `parse_official` 而不是 `from_str`,避免和 `std::str::FromStr::from_str`
    /// 撞名(clippy `should_implement_trait` 会拦)。
    pub fn parse_official(s: &str) -> Option<Self> {
        match s {
            "READ" => Some(Permission::Read),
            "WRITE" => Some(Permission::Write),
            "READ_ACP" => Some(Permission::ReadAcp),
            "WRITE_ACP" => Some(Permission::WriteAcp),
            "FULL_CONTROL" => Some(Permission::FullControl),
            _ => None,
        }
    }
}

/// 一条细粒度授权:被授权账号(AWS canonical user ID / 华为云账号 ID)+ 权限。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grant {
    /// 被授权方的账号 ID(AWS 的 canonical user ID,或华为云的账号 ID / DomainId)。
    /// 不建模预置分组(`AllUsers` 等)——那类需求已经被现有的公开/私有二态覆盖。
    pub grantee_id: String,
    pub permission: Permission,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_round_trips_through_official_strings() {
        for p in [
            Permission::Read,
            Permission::Write,
            Permission::ReadAcp,
            Permission::WriteAcp,
            Permission::FullControl,
        ] {
            assert_eq!(Permission::parse_official(p.as_str()), Some(p));
        }
    }

    #[test]
    fn unknown_permission_string_is_none() {
        assert_eq!(Permission::parse_official("BOGUS"), None);
    }
}
