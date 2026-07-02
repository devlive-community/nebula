//! 各家 SDK 共用的 HTTP 客户端封装。
//!
//! 只是 [`reqwest::Client`] 之上很薄的一层:统一默认配置(超时、UA)、
//! 复用连接池、把传输错误映射到 [`CoreError`]。请求的构造(URL / 头 / 签名)
//! 仍由各 SDK 自己完成——本层不假设任何厂商语义。

use std::time::Duration;

use crate::error::{CoreError, Result};

/// 共享的 HTTP 客户端。内部持有 [`reqwest::Client`],可低成本 clone(引用计数)。
#[derive(Debug, Clone)]
pub struct HttpClient {
    inner: reqwest::Client,
}

impl HttpClient {
    /// 用带合理默认值(30s 超时、Nebula UA)的客户端创建。
    pub fn new() -> Self {
        let inner = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .user_agent(concat!("nebula-cloud-core/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("default reqwest client should build");
        Self { inner }
    }

    /// 复用调用方已配置好的 [`reqwest::Client`](如需自定义代理 / TLS)。
    pub fn from_client(inner: reqwest::Client) -> Self {
        Self { inner }
    }

    /// 访问底层客户端,用于构造 [`reqwest::Request`]。
    pub fn inner(&self) -> &reqwest::Client {
        &self.inner
    }

    /// 发送一个已构造(通常已签名)的请求,传输失败映射为 [`CoreError::Transport`]。
    ///
    /// 注意:这里**不**根据 HTTP 状态码判定成功与否——是否把 4xx/5xx 当错误、
    /// 如何解析各家的错误响应体,由各 SDK 决定。
    pub async fn execute(&self, request: reqwest::Request) -> Result<reqwest::Response> {
        self.inner.execute(request).await.map_err(CoreError::from)
    }
}

impl Default for HttpClient {
    fn default() -> Self {
        Self::new()
    }
}
