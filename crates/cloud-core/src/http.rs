//! 各家 SDK 共用的 HTTP 客户端封装。
//!
//! 只是 [`reqwest::Client`] 之上很薄的一层:统一默认配置(超时、UA)、
//! 复用连接池、把传输错误映射到 [`CoreError`]。请求的构造(URL / 头 / 签名)
//! 仍由各 SDK 自己完成——本层不假设任何厂商语义。

use std::time::Duration;

use crate::error::{CoreError, Result};
use crate::retry::RetryPolicy;

/// 共享的 HTTP 客户端。内部持有 [`reqwest::Client`],可低成本 clone(引用计数)。
#[derive(Debug, Clone)]
pub struct HttpClient {
    inner: reqwest::Client,
    retry: RetryPolicy,
}

impl HttpClient {
    /// 用带合理默认值的客户端创建。
    ///
    /// 注意用 `read_timeout`(每次读取的超时)而非 `timeout`(整个请求的超时):
    /// 后者会把流式下载的整段 body 读取算进去,大文件超时后被中断,表现为
    /// "error decoding response body"。`read_timeout` 只要求数据持续到达即可。
    pub fn new() -> Self {
        let inner = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .read_timeout(Duration::from_secs(60))
            .user_agent(concat!("nebula-cloud-core/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("default reqwest client should build");
        Self {
            inner,
            retry: RetryPolicy::default(),
        }
    }

    /// 复用调用方已配置好的 [`reqwest::Client`](如需自定义代理 / TLS)。
    pub fn from_client(inner: reqwest::Client) -> Self {
        Self {
            inner,
            retry: RetryPolicy::default(),
        }
    }

    /// 覆盖重试策略(默认 [`RetryPolicy::default`]);传 [`RetryPolicy::none`] 可关闭重试。
    pub fn with_retry(mut self, retry: RetryPolicy) -> Self {
        self.retry = retry;
        self
    }

    /// 访问底层客户端,用于构造 [`reqwest::Request`]。
    pub fn inner(&self) -> &reqwest::Client {
        &self.inner
    }

    /// 发送一个已构造(通常已签名)的请求,传输失败映射为 [`CoreError::Transport`]。
    ///
    /// **重试**:对**幂等方法**(GET / HEAD / PUT / DELETE)且遇到瞬时故障(连接 / 超时,
    /// 或 429 / 500 / 502 / 503 / 504)时,按 [`RetryPolicy`] 指数退避重试;非幂等方法
    /// (如 POST)不重试,以免重复副作用。请求需可 `try_clone`(流式 body 不可克隆)才会重试。
    ///
    /// 注意:这里**不**根据 HTTP 状态码判定成功与否——是否把 4xx/5xx 当错误、
    /// 如何解析各家的错误响应体,由各 SDK 决定;本层只在**可重试**的状态码上重发。
    pub async fn execute(&self, mut request: reqwest::Request) -> Result<reqwest::Response> {
        let idempotent = is_idempotent(request.method());
        let mut attempt = 0u32;
        loop {
            // 只有幂等方法、且还有重试额度时才预留一个克隆用于重发。
            let retry_copy = if idempotent && attempt < self.retry.max_retries {
                request.try_clone()
            } else {
                None
            };
            let result = self.inner.execute(request).await;

            let retryable = match &result {
                Ok(resp) => is_retryable_status(resp.status()),
                Err(err) => is_retryable_transport(err),
            };
            match retry_copy {
                Some(next) if retryable => {
                    let delay = self.retry.backoff(attempt);
                    if !delay.is_zero() {
                        tokio::time::sleep(delay).await;
                    }
                    attempt += 1;
                    request = next;
                }
                _ => return result.map_err(CoreError::from),
            }
        }
    }
}

/// 幂等方法可安全重试(重发不产生额外副作用)。
fn is_idempotent(method: &reqwest::Method) -> bool {
    use reqwest::Method;
    matches!(
        *method,
        Method::GET | Method::HEAD | Method::PUT | Method::DELETE | Method::OPTIONS
    )
}

/// 可重试的状态码:限流(429)与常见瞬时服务端错误(500/502/503/504)。
fn is_retryable_status(status: reqwest::StatusCode) -> bool {
    status == reqwest::StatusCode::TOO_MANY_REQUESTS
        || matches!(status.as_u16(), 500 | 502 | 503 | 504)
}

/// 可重试的传输错误:连接建立失败或超时。
fn is_retryable_transport(err: &reqwest::Error) -> bool {
    err.is_timeout() || err.is_connect()
}

impl Default for HttpClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::{Method, StatusCode};

    #[test]
    fn only_idempotent_methods_retry() {
        assert!(is_idempotent(&Method::GET));
        assert!(is_idempotent(&Method::PUT));
        assert!(is_idempotent(&Method::DELETE));
        assert!(is_idempotent(&Method::HEAD));
        assert!(!is_idempotent(&Method::POST));
        assert!(!is_idempotent(&Method::PATCH));
    }

    #[test]
    fn retryable_statuses() {
        for s in [500u16, 502, 503, 504, 429] {
            assert!(is_retryable_status(StatusCode::from_u16(s).unwrap()));
        }
        for s in [200u16, 301, 400, 403, 404, 409, 501] {
            assert!(!is_retryable_status(StatusCode::from_u16(s).unwrap()));
        }
    }
}
