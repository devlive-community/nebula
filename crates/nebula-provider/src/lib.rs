//! # nebula-provider
//!
//! Nebula App 与各家云之间的**统一抽象层**。App 只依赖本 crate 的
//! [`StorageProvider`] trait 与数据模型,不直接接触任何厂商 SDK;各厂商由
//! `providers/provider-*` 适配层实现该 trait。
//!
//! 提供:
//!
//! - [`StorageProvider`] — 对象安全的统一存储 trait(路径约定见其文档)
//! - [`Entry`] / [`EntryKind`] — 统一条目模型(可 serde,供前端消费)
//! - [`Capabilities`] — provider 能力位
//! - [`ProviderError`] — 统一错误
//! - [`ProviderRegistry`] — 按 id 管理多账号 / 多云 provider
//! - [`path`] — `bucket/key` 路径解析工具(供适配层复用)

pub mod capabilities;
pub mod cors;
pub mod entry;
pub mod error;
pub mod grant;
pub mod lifecycle;
pub mod path;
pub mod provider;
pub mod registry;
pub mod version;
pub mod website;

pub use capabilities::Capabilities;
pub use cors::CorsRule;
pub use entry::{Entry, EntryKind};
pub use error::{ProviderError, Result};
pub use grant::{Grant, Permission};
pub use lifecycle::LifecycleRule;
pub use provider::{
    collect_stream, ByteStream, IncompleteUpload, Page, ProgressFn, StorageProvider,
};
pub use registry::ProviderRegistry;
pub use version::ObjectVersion;
pub use website::WebsiteConfig;
