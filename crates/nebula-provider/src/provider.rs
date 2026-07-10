//! 统一存储抽象:App 只依赖 [`StorageProvider`],不感知具体是哪家云。

use std::pin::Pin;

use async_trait::async_trait;
use bytes::Bytes;
use futures::Stream;

use crate::capabilities::Capabilities;
use crate::entry::Entry;
use crate::error::{ProviderError, Result};

/// 进度回调:`(已处理字节, 总字节)`。适配层在传输过程中多次调用。
pub type ProgressFn<'a> = &'a (dyn Fn(u64, u64) + Send + Sync);

/// 分块字节流,用于流式下载(边下边写、可报进度)。
pub type ByteStream = Pin<Box<dyn Stream<Item = Result<Bytes>> + Send>>;

/// 把一个 [`ByteStream`] 收集成完整 [`Bytes`],任一分块出错即向上传播。
///
/// 供适配层在"小文件退化为简单 PUT"或兜底路径复用;流式上传不应走这里(会缓冲整个对象)。
pub async fn collect_stream(mut stream: ByteStream) -> Result<Bytes> {
    use futures::StreamExt;
    let mut buf = bytes::BytesMut::new();
    while let Some(chunk) = stream.next().await {
        buf.extend_from_slice(&chunk?);
    }
    Ok(buf.freeze())
}

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

    /// 流式下载:返回 `(内容长度, 分块流)`,用于边下边写并报告进度。
    ///
    /// 默认实现回退到 [`read`](Self::read)(整块作为单个分块);支持流式的适配层可
    /// 覆盖以获得真实的分块与进度。
    async fn read_stream(&self, path: &str) -> Result<(Option<u64>, ByteStream)> {
        let data = self.read(path).await?;
        let len = data.len() as u64;
        let stream = futures::stream::once(async move { Ok(data) });
        Ok((Some(len), Box::pin(stream)))
    }

    /// 从 `offset` 字节开始流式下载(HTTP Range),返回 `(对象总大小, 剩余字节流)`,用于
    /// **断点续传**。默认实现忽略 `offset` 回退到 [`read_stream`](Self::read_stream)(不支持续传);
    /// 支持 Range 的适配层应覆盖此方法以真正从 `offset` 续传。
    async fn read_range(&self, path: &str, offset: u64) -> Result<(Option<u64>, ByteStream)> {
        let _ = offset;
        self.read_stream(path).await
    }

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

    /// 流式上传:从 `stream` 边收边写,不把整个对象缓冲进内存。`len` 为已知的对象总大小
    /// (用于进度分母与"小文件走简单 PUT"的判断),未知时传 `None`。
    ///
    /// 默认实现把整个流收集成 [`Bytes`] 再调 [`write_with_progress`](Self::write_with_progress)
    /// (**不省内存**,仅作兜底);支持分片的适配层应覆盖此方法,用流式分片上传把内存占用
    /// 压到常数级。
    async fn write_stream(
        &self,
        path: &str,
        len: Option<u64>,
        stream: ByteStream,
        content_type: Option<&str>,
        progress: ProgressFn<'_>,
    ) -> Result<()> {
        let _ = len;
        let data = collect_stream(stream).await?;
        self.write_with_progress(path, data, content_type, progress)
            .await
    }

    /// 删除单个对象。
    async fn delete(&self, path: &str) -> Result<()>;

    /// 复制对象。默认实现下载再上传;支持服务端复制的适配层应覆盖以提升效率。
    async fn copy(&self, from: &str, to: &str) -> Result<()> {
        let data = self.read(from).await?;
        self.write(to, data, None).await
    }

    /// 重命名(移动)对象:复制到新路径后删除原对象。
    async fn rename(&self, from: &str, to: &str) -> Result<()> {
        self.copy(from, to).await?;
        self.delete(from).await
    }

    /// 生成一个 `expires_secs` 秒后失效的预签名下载链接。默认不支持。
    async fn presign(&self, _path: &str, _expires_secs: u64) -> Result<String> {
        Err(ProviderError::Unsupported("presign".into()))
    }
}
