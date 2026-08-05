//! # ucloud-us3
//!
//! UCloud US3(优刻得对象存储)的异步 Rust SDK,走 US3 官方文档确认的"仅支持 SigV4"S3 兼容
//! API,是共享 crate [`s3_core`] 的品牌门面。
//!
//! US3 的 endpoint 形如 `s3-{region}.ufileos.com`(比如 `s3-cn-bj.ufileos.com`)——**带
//! `s3-`(连字符)而不是 `s3.`(点号)前缀**,和 AWS/B2/Wasabi/Scaleway 那种
//! `s3.{region}.xxx.com` 形状不一样,`S3Client::new` 内置的 region 解析(只认 `s3.` 前缀)
//! 在这里会失败。因此**用 [`new_client`] 构造**,不要直接 `S3Client::new`——它会剥掉 `s3-`
//! 前缀后取第一个 `.` 之前的片段作为 region。这是"包一层构造函数推导 region"模式的第四种
//! 变体(R2/MinIO 硬编码常量、DigitalOcean Spaces 取 endpoint 第一段、这次是剥前缀后取段)。
//!
//! ```no_run
//! let client = ucloud_us3::new_client("ak", "sk", "s3-cn-bj.ufileos.com");
//! ```

pub use s3_core::{bucket, client, error, multipart, object};
pub use s3_core::{
    BucketSummary, CorsRule, LifecycleRule, ListEntry, ObjectMeta, ObjectSummary, ObjectVersion,
    Result, S3Client, S3Error, WebsiteConfig, MIN_PART_SIZE,
};

/// 用 US3 endpoint 创建客户端。region 从 `s3-` 前缀之后、第一个 `.` 之前的片段推导
/// (`s3-cn-bj.ufileos.com` → `cn-bj`),因为 US3 的 endpoint 不是 `S3Client::new` 内置
/// region 解析认识的 `s3.` 点号前缀形状。
pub fn new_client(
    access_key: impl Into<String>,
    secret_key: impl Into<String>,
    endpoint: impl Into<String>,
) -> S3Client {
    let client = S3Client::new(access_key, secret_key, endpoint);
    let region = client
        .endpoint()
        .strip_prefix("s3-")
        .and_then(|rest| rest.split('.').next())
        .unwrap_or("")
        .to_string();
    client.with_region(region)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn region_derived_by_stripping_dash_prefix() {
        let client = new_client("ak", "sk", "s3-cn-bj.ufileos.com");
        assert_eq!(client.region(), "cn-bj");
    }
}
