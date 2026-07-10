//! 通用带宽限速:令牌桶(token bucket)。
//!
//! 与厂商、协议无关——只按"字节/秒"配额放行字节。桶按时间匀速补充令牌,
//! 桶容量 = 1 秒的额度(即允许 1 秒的突发)。速率可在运行时热更新(设置里改限速立即生效);
//! `rate == 0` 表示不限速,`acquire` 直接返回。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use tokio::time::Instant;

/// 令牌桶限速器。可被多个传输任务共享(`Arc<RateLimiter>`),形成**全局**带宽上限。
pub struct RateLimiter {
    /// 字节/秒;0 表示不限速。可热更新。
    rate: AtomicU64,
    bucket: Mutex<Bucket>,
}

struct Bucket {
    /// 当前可用令牌(字节),可为负表示"欠账",由后续等待偿还。
    tokens: f64,
    /// 上次补充令牌的时刻。
    last: Instant,
}

impl RateLimiter {
    /// 新建限速器。`bytes_per_sec == 0` 表示不限速。
    pub fn new(bytes_per_sec: u64) -> Self {
        Self {
            rate: AtomicU64::new(bytes_per_sec),
            bucket: Mutex::new(Bucket {
                tokens: bytes_per_sec as f64,
                last: Instant::now(),
            }),
        }
    }

    /// 热更新速率(字节/秒);0 表示不限速。设置里调整限速时调用,立即对进行中的传输生效。
    pub fn set_rate(&self, bytes_per_sec: u64) {
        self.rate.store(bytes_per_sec, Ordering::Relaxed);
    }

    /// 当前速率(字节/秒)。
    pub fn rate(&self) -> u64 {
        self.rate.load(Ordering::Relaxed)
    }

    /// 取用 `n` 字节的配额:先按流逝时间补桶并扣除,欠账则异步等待偿还后返回。
    ///
    /// 采用"欠账"模型:一次可扣到负值,再按速率把负债等待抹平,因此对任意块大小都能正确限速,
    /// 不会因单块超过桶容量而死循环。
    pub async fn acquire(&self, n: u64) {
        let rate = self.rate.load(Ordering::Relaxed);
        if rate == 0 || n == 0 {
            return;
        }
        let wait = {
            let mut b = self.bucket.lock().unwrap();
            let now = Instant::now();
            let elapsed = now.duration_since(b.last).as_secs_f64();
            let cap = rate as f64; // 桶容量 = 1 秒额度
            b.tokens = (b.tokens + elapsed * rate as f64).min(cap);
            b.last = now;
            b.tokens -= n as f64;
            if b.tokens < 0.0 {
                Some(Duration::from_secs_f64(-b.tokens / rate as f64))
            } else {
                None
            }
        };
        if let Some(d) = wait {
            tokio::time::sleep(d).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn unlimited_when_rate_is_zero() {
        let rl = RateLimiter::new(0);
        let start = Instant::now();
        rl.acquire(1_000_000).await;
        assert!(start.elapsed() < Duration::from_millis(1));
    }

    #[tokio::test(start_paused = true)]
    async fn paces_transfer_to_the_configured_rate() {
        let rl = RateLimiter::new(1000); // 1000 B/s,桶容量 1000
        let start = Instant::now();
        rl.acquire(1000).await; // 满桶 → 立即
        rl.acquire(1000).await; // 空桶 → 约等 1s
        rl.acquire(1000).await; // 再等约 1s
        let e = start.elapsed();
        assert!(e >= Duration::from_millis(1900), "elapsed {e:?}");
        assert!(e < Duration::from_millis(2200), "elapsed {e:?}");
    }

    #[tokio::test(start_paused = true)]
    async fn handles_chunks_larger_than_capacity() {
        let rl = RateLimiter::new(1000); // 桶容量 1000,却要一次取 5000
        let start = Instant::now();
        rl.acquire(5000).await; // 欠账 4000 → 等约 4s,不死循环
        let e = start.elapsed();
        assert!(e >= Duration::from_millis(3900), "elapsed {e:?}");
    }

    #[tokio::test(start_paused = true)]
    async fn rate_change_takes_effect() {
        let rl = RateLimiter::new(1000);
        rl.acquire(1000).await; // 清空满桶
        rl.set_rate(0); // 改为不限速
        let start = Instant::now();
        rl.acquire(1_000_000).await; // 立即返回
        assert!(start.elapsed() < Duration::from_millis(1));
    }
}
