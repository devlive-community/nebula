//! 通用异步重试,带指数退避。
//!
//! 与厂商、与 HTTP 无关:调用方给出一个"每次尝试"的异步操作,以及一个
//! "该错误是否可重试"的判定。只对幂等操作使用(GET / HEAD / PUT 等)。

use std::future::Future;
use std::time::Duration;

/// 重试策略。
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    /// 首次失败后最多再重试几次(0 表示不重试)。
    pub max_retries: u32,
    /// 第一次退避的基准时长,之后指数增长。
    pub base_delay: Duration,
    /// 退避时长上限。
    pub max_delay: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 3,
            base_delay: Duration::from_millis(200),
            max_delay: Duration::from_secs(10),
        }
    }
}

impl RetryPolicy {
    /// 不重试。便于测试或明确关闭重试。
    pub fn none() -> Self {
        Self {
            max_retries: 0,
            base_delay: Duration::ZERO,
            max_delay: Duration::ZERO,
        }
    }

    /// 第 `attempt` 次重试(从 0 计)前应等待的退避时长。
    pub fn backoff(&self, attempt: u32) -> Duration {
        let factor = 1u32.checked_shl(attempt).unwrap_or(u32::MAX);
        let delay = self.base_delay.saturating_mul(factor);
        delay.min(self.max_delay)
    }
}

/// 按 `policy` 反复执行 `op`,遇到 `is_retryable` 判定为可重试的错误就退避后重试。
///
/// 成功、或不可重试的错误、或用尽重试次数时立即返回。
pub async fn retry<T, E, F, Fut>(
    policy: &RetryPolicy,
    mut is_retryable: impl FnMut(&E) -> bool,
    mut op: F,
) -> Result<T, E>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, E>>,
{
    let mut attempt = 0;
    loop {
        match op().await {
            Ok(value) => return Ok(value),
            Err(err) => {
                if attempt >= policy.max_retries || !is_retryable(&err) {
                    return Err(err);
                }
                let delay = policy.backoff(attempt);
                if !delay.is_zero() {
                    tokio::time::sleep(delay).await;
                }
                attempt += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[tokio::test]
    async fn succeeds_first_try_without_retry() {
        let calls_cell = Cell::new(0);
        let calls = &calls_cell; // &Cell 是 Copy,可在闭包/future 间安全共享
        let out: Result<i32, ()> = retry(
            &RetryPolicy::none(),
            |_| true,
            || async move {
                calls.set(calls.get() + 1);
                Ok(42)
            },
        )
        .await;
        assert_eq!(out, Ok(42));
        assert_eq!(calls_cell.get(), 1);
    }

    #[tokio::test]
    async fn retries_until_success() {
        let policy = RetryPolicy {
            max_retries: 5,
            base_delay: Duration::ZERO, // 测试里不真正等待
            max_delay: Duration::ZERO,
        };
        let calls_cell = Cell::new(0);
        let calls = &calls_cell;
        let out: Result<&str, &str> = retry(
            &policy,
            |_| true,
            || async move {
                calls.set(calls.get() + 1);
                if calls.get() < 3 {
                    Err("transient")
                } else {
                    Ok("ok")
                }
            },
        )
        .await;
        assert_eq!(out, Ok("ok"));
        assert_eq!(calls_cell.get(), 3);
    }

    #[tokio::test]
    async fn stops_on_non_retryable_error() {
        let calls_cell = Cell::new(0);
        let calls = &calls_cell;
        let out: Result<(), &str> = retry(
            &RetryPolicy::default(),
            |_| false,
            || async move {
                calls.set(calls.get() + 1);
                Err("fatal")
            },
        )
        .await;
        assert_eq!(out, Err("fatal"));
        assert_eq!(calls_cell.get(), 1); // 不可重试,只调用一次
    }

    #[tokio::test]
    async fn exhausts_max_retries() {
        let policy = RetryPolicy {
            max_retries: 2,
            base_delay: Duration::ZERO,
            max_delay: Duration::ZERO,
        };
        let calls_cell = Cell::new(0);
        let calls = &calls_cell;
        let out: Result<(), &str> = retry(
            &policy,
            |_| true,
            || async move {
                calls.set(calls.get() + 1);
                Err("always")
            },
        )
        .await;
        assert_eq!(out, Err("always"));
        assert_eq!(calls_cell.get(), 3); // 1 次初始 + 2 次重试
    }

    #[test]
    fn backoff_grows_and_caps() {
        let policy = RetryPolicy {
            max_retries: 10,
            base_delay: Duration::from_millis(100),
            max_delay: Duration::from_millis(500),
        };
        assert_eq!(policy.backoff(0), Duration::from_millis(100));
        assert_eq!(policy.backoff(1), Duration::from_millis(200));
        assert_eq!(policy.backoff(2), Duration::from_millis(400));
        assert_eq!(policy.backoff(3), Duration::from_millis(500)); // 命中上限
        assert_eq!(policy.backoff(30), Duration::from_millis(500)); // 溢出也不 panic
    }
}
