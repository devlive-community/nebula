//! 统一存储抽象:App 只依赖 [`StorageProvider`],不感知具体是哪家云。

use async_trait::async_trait;
use bytes::Bytes;

use crate::capabilities::Capabilities;
use crate::entry::Entry;
use crate::error::Result;

/// 进度回调:`(已处理字节, 总字节)`。适配层在传输过程中多次调用。
pub type ProgressFn<'a> = &'a (dyn Fn(u64, u64) + Send + Sync);

/// 一个存储 provider 实例(通常 = 一个云账号)。
///
/// # 路径约定
///
/// 路径采用 `bucket/key` 形式,适配层据此解释:
///
/// - `list("")` / `list("/")` → 列出所有 bucket(每个是一个目录)
/// - `list("bucket/")` → 列出该桶根下条目
/// - `list("bucket/prefix/")` → 列出该前缀下条目
/// - `read` / `write` / `delete` / `stat("bucket/key")` → 对象操作
///
/// trait 是对象安全的:App 可持有 `Arc<dyn StorageProvider>` 放进注册表统一调度。
#[async_trait]
pub trait StorageProvider: Send + Sync {
    /// provider 实例的稳定标识(通常是账号 / 别名),注册表以此为键。
    fn id(&self) -> &str;

    /// 声明支持的高级能力。
    fn capabilities(&self) -> Capabilities;

    /// 列出某路径下的条目(桶或前缀)。
    async fn list(&self, path: &str) -> Result<Vec<Entry>>;

    /// 读取单个对象的元信息。
    async fn stat(&self, path: &str) -> Result<Entry>;

    /// 下载单个对象的完整内容。
    async fn read(&self, path: &str) -> Result<Bytes>;

    /// 上传 / 覆盖单个对象。
    async fn write(&self, path: &str, data: Bytes, content_type: Option<&str>) -> Result<()>;

    /// 带进度的上传。默认实现直接调用 [`write`](Self::write),成功后回报 100%;
    /// 支持分块上传的适配层可覆盖以回报中间进度。
    async fn write_with_progress(
        &self,
        path: &str,
        data: Bytes,
        content_type: Option<&str>,
        progress: ProgressFn<'_>,
    ) -> Result<()> {
        let total = data.len() as u64;
        let result = self.write(path, data, content_type).await;
        if result.is_ok() {
            progress(total, total);
        }
        result
    }

    /// 删除单个对象。
    async fn delete(&self, path: &str) -> Result<()>;
}
