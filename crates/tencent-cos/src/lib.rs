//! # tencent-cos
//!
//! 手写的腾讯云 COS(对象存储)异步 Rust SDK,不依赖任何聚合库。
//!
//! COS 请求/响应是 S3 风格,但**认证用腾讯专有签名**(`q-sign-algorithm=sha1`,基于 HMAC-SHA1),
//! 不是 AWS SigV4,因此不复用 s3-core,自带 [`sign`]。
//!
//! 当前增量提供:
//!
//! - [`sign`] — COS 专有签名(HMAC-SHA1,`q-sign` 头)
//!
//! 本 crate 只依赖 [`cloud_core`] 与 reqwest,不感知任何上层应用。

pub mod sign;
