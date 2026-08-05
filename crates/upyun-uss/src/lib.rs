//! # upyun-uss
//!
//! 又拍云(UPYUN)云存储的异步 Rust SDK,走又拍云官方文档确认的"S3 v4 协议标准兼容"API,是
//! 共享 crate [`s3_core`] 的品牌门面。
//!
//! ⚠️ **region 值未经官方文档或第三方代码证实,是推断值,需要真实账号验证**。又拍云的 S3 兼容
//! endpoint 固定是 `s3.api.upyun.com`,主机名里**不含地域**,官方文档原话是"又拍云仅支持
//! 文件相关 API,不支持配置区域"——这句话可能是指"没有多地域部署选择",也可能是指"SigV4 的
//! region 参数本身可以任意/固定",两种理解都合理,没有找到真实工作的第三方代码或官方样例能
//! 二选一。这里按照 rclone(未配置 region 时的兜底值)与 MinIO(经典默认值)的通用惯例,
//! 硬编码成 `us-east-1`——如果这个猜测错了,表现是登录鉴权失败(签名不匹配),不会是数据损坏,
//! 但在有真实账号验证之前,不应该把这当作确定结论。
//!
//! endpoint 固定为 `s3.api.upyun.com`,没有 `{region}` 占位符可解析,因此**用 [`new_client`]
//! 构造**,不要直接 `S3Client::new`——和 MinIO 硬编码 `us-east-1`、R2 硬编码 `auto` 是同一类
//! "包一层构造函数固定 region"模式。
//!
//! 又拍云的 S3 兼容层官方文档明确列出的支持操作只有文件相关的一部分(list/put/get/head/
//! delete/copy/multipart),没有出现 bucket 级管理接口(CORS/生命周期/网站托管/建桶/删桶),
//! 也没有出现标签/ACL/版本控制——这不是"没查到证据"的保守判断,是文档明确给出的**完整**
//! 支持列表里就没有这些项,能力矩阵按此收窄(见 `provider-upyun` 的实现)。
//!
//! ```no_run
//! let client = upyun_uss::new_client("ak", "sk", "s3.api.upyun.com");
//! ```

pub use s3_core::{bucket, client, error, multipart, object};
pub use s3_core::{
    BucketSummary, CorsRule, LifecycleRule, ListEntry, ObjectMeta, ObjectSummary, ObjectVersion,
    Result, S3Client, S3Error, WebsiteConfig, MIN_PART_SIZE,
};

/// 用又拍云 S3 兼容 endpoint 创建客户端。region 硬编码 `us-east-1`(未经官方确认的推断值,
/// 见 crate 文档)。
pub fn new_client(
    access_key: impl Into<String>,
    secret_key: impl Into<String>,
    endpoint: impl Into<String>,
) -> S3Client {
    S3Client::new(access_key, secret_key, endpoint).with_region("us-east-1")
}
