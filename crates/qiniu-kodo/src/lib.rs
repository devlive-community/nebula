//! # qiniu-kodo
//!
//! 手写的七牛云 Kodo(对象存储)异步 Rust SDK,走七牛的 **S3 兼容端点**,
//! 用 **AWS Signature V4** 签名,不依赖任何聚合库。
//!
//! 与阿里云 OSS / 华为云 OBS 的差异:路径风格访问(bucket 在 path)、SigV4 签名体系
//! (SHA256 链,不是 V2 的 HMAC-SHA1)、endpoint 为 `s3.{region}.qiniucs.com`。
//!
//! 当前增量提供:
//!
//! - [`sign`] — AWS Signature V4 签名
//!
//! 本 crate 只依赖 [`cloud_core`] 与 reqwest,不感知任何上层应用。

pub mod sign;
