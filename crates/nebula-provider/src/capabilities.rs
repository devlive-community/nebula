//! provider 能力位。App 据此决定是否暴露某些功能(如大文件分片、临时链接)。

use serde::{Deserialize, Serialize};

/// 一个 provider 支持哪些高级能力。缺省全部为 `false`,适配层按实际情况开启。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    /// 支持分片上传大文件。
    pub multipart_upload: bool,
    /// 支持可断点续传的分片上传(begin/upload_part/complete/abort 已实现)。
    pub resumable_upload: bool,
    /// 支持存储类型转换与归档取回(set_storage_class / restore 已实现)。
    pub storage_class_ops: bool,
    /// 支持新建 / 删除 bucket(create_bucket / delete_bucket 已实现)。
    pub bucket_ops: bool,
    /// 支持读写 bucket 生命周期规则(bucket_lifecycle / set_bucket_lifecycle 已实现)。
    pub bucket_lifecycle: bool,
    /// 支持修改对象元数据(set_content_type 已实现)。
    pub metadata_ops: bool,
    /// 支持读写对象标签(object_tags / set_object_tags 已实现)。
    pub object_tagging: bool,
    /// 支持列举 / 清理未完成的分片上传(list_incomplete_uploads 已实现)。
    pub multipart_cleanup: bool,
    /// 支持设置对象 ACL(公开读 / 私有)与公共直链(set_object_acl / public_url 已实现)。
    pub object_acl: bool,
    /// 支持生成预签名临时链接。
    pub presign: bool,
    /// 支持服务端复制(跨对象 / 跨桶免中转)。
    pub server_side_copy: bool,
    /// 后端有真正的层级目录(否则目录是按前缀模拟的)。
    pub hierarchical: bool,
}
