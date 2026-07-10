//! 传输的全局带宽约束。
//!
//! 持有一个共享的 [`RateLimiter`](令牌桶),把限速套到**读取侧**的字节流上——
//! 下载、文件夹下载、跨账号迁移都经由它限速。速率由 [`Settings`](crate::Settings) 驱动,
//! 可热更新;`0` 表示不限速。
//!
//! 克隆 [`TransferLimits`] 共享同一个令牌桶,因此 `App` 的所有克隆(含 Tauri 各 command)
//! 汇入**同一个全局带宽上限**。

use std::sync::Arc;

use cloud_core::RateLimiter;
use futures::StreamExt;
use nebula_provider::ByteStream;

/// 每 KiB 的字节数。设置以 KiB/秒 表达,底层按字节计。
const KIB: u64 = 1024;

/// 全局传输限速。克隆共享同一令牌桶。
#[derive(Clone)]
pub struct TransferLimits {
    rate: Arc<RateLimiter>,
}

impl TransferLimits {
    /// 以「KiB/秒」建限速(`0` = 不限速)。
    pub fn new(kib_per_sec: u64) -> Self {
        Self {
            rate: Arc::new(RateLimiter::new(kib_per_sec.saturating_mul(KIB))),
        }
    }

    /// 热更新限速(KiB/秒;`0` = 不限速),对进行中的传输立即生效。
    pub fn set_kib_per_sec(&self, kib_per_sec: u64) {
        self.rate.set_rate(kib_per_sec.saturating_mul(KIB));
    }

    /// 当前限速(KiB/秒;`0` 表示不限速)。
    pub fn kib_per_sec(&self) -> u64 {
        self.rate.rate() / KIB
    }

    /// 取用 `n` 字节的配额(不限速时立即返回)。下载循环每写一块调用一次。
    pub async fn throttle(&self, n: u64) {
        self.rate.acquire(n).await;
    }

    /// 给字节流套上限速:每块产出前按其大小取配额。用于迁移的读取流。
    pub fn throttled(&self, stream: ByteStream) -> ByteStream {
        let rate = self.rate.clone();
        Box::pin(stream.then(move |item| {
            let rate = rate.clone();
            async move {
                if let Ok(bytes) = &item {
                    rate.acquire(bytes.len() as u64).await;
                }
                item
            }
        }))
    }
}

impl Default for TransferLimits {
    /// 默认不限速。
    fn default() -> Self {
        Self::new(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use tokio::time::{Duration, Instant};

    fn stream_of(chunks: Vec<&'static [u8]>) -> ByteStream {
        Box::pin(futures::stream::iter(
            chunks.into_iter().map(|c| Ok(Bytes::from_static(c))),
        ))
    }

    #[tokio::test(start_paused = true)]
    async fn unlimited_passes_through_immediately() {
        let limits = TransferLimits::new(0);
        let mut s = limits.throttled(stream_of(vec![b"aaaa", b"bbbb"]));
        let start = Instant::now();
        while s.next().await.is_some() {}
        assert!(start.elapsed() < Duration::from_millis(1));
    }

    #[tokio::test(start_paused = true)]
    async fn throttled_stream_paces_by_bytes() {
        // 1 KiB/s;两块各 1024 字节 → 第二块约需等 1s。
        let limits = TransferLimits::new(1);
        let kib = vec![0u8; 1024].leak();
        let mut s = limits.throttled(stream_of(vec![kib, kib]));
        let start = Instant::now();
        while s.next().await.is_some() {}
        assert!(start.elapsed() >= Duration::from_millis(900), "too fast");
    }

    #[test]
    fn kib_roundtrip() {
        let limits = TransferLimits::new(512);
        assert_eq!(limits.kib_per_sec(), 512);
        limits.set_kib_per_sec(0);
        assert_eq!(limits.kib_per_sec(), 0);
    }
}
