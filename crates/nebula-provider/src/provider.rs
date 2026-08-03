//! 统一存储抽象:App 只依赖 [`StorageProvider`],不感知具体是哪家云。

use std::pin::Pin;

use async_trait::async_trait;
use bytes::Bytes;
use futures::Stream;

use crate::capabilities::Capabilities;
use crate::entry::Entry;
use crate::error::{ProviderError, Result};
use crate::lifecycle::LifecycleRule;

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

/// 分页列举的一页:本页条目 + 下一页游标(`None` 表示已到末页)。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Page {
    pub entries: Vec<Entry>,
    pub cursor: Option<String>,
}

/// 一个未完成(残留)的分片上传:已初始化但未完成 / 中止,分片仍在计费。可 serde 供前端消费。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct IncompleteUpload {
    /// 对象 key(桶内相对路径)。
    pub key: String,
    pub upload_id: String,
    /// 发起时间(ISO 8601);未知为空串。
    pub initiated: String,
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

    /// 分页列出某路径下的**一页**条目 + 下一页游标;`cursor` 为 `None` 取第一页。
    ///
    /// 默认实现回退到 [`list`](Self::list):一次性列全作为单页(无下一页游标),保证任何
    /// provider 都可用。支持游标分页的适配层应覆盖此方法,避免大目录一次性拉全导致的延迟。
    async fn list_page(&self, path: &str, cursor: Option<String>) -> Result<Page> {
        let _ = cursor;
        Ok(Page {
            entries: self.list(path).await?,
            cursor: None,
        })
    }

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

    /// 开始一次分片上传,返回 `upload_id`。用于**可断点续传**的大文件上传。
    ///
    /// 默认返回 [`ProviderError::Unsupported`],调用方据此回退到整体上传;
    /// 支持分片的适配层应覆盖这四个方法(begin / upload_part / complete / abort)
    /// 并在 [`capabilities`](Self::capabilities) 里置 `resumable_upload = true`。
    async fn begin_multipart(&self, _path: &str, _content_type: Option<&str>) -> Result<String> {
        Err(ProviderError::Unsupported(
            "resumable multipart upload".into(),
        ))
    }

    /// 上传第 `part_number` 个分片(从 1 计),返回该分片的 ETag。
    async fn upload_part(
        &self,
        _path: &str,
        _upload_id: &str,
        _part_number: u32,
        _data: Bytes,
    ) -> Result<String> {
        Err(ProviderError::Unsupported(
            "resumable multipart upload".into(),
        ))
    }

    /// 完成分片上传。`parts` 为 `(part_number, etag)` 列表(可乱序,由适配层排序)。
    async fn complete_multipart(
        &self,
        _path: &str,
        _upload_id: &str,
        _parts: &[(u32, String)],
    ) -> Result<()> {
        Err(ProviderError::Unsupported(
            "resumable multipart upload".into(),
        ))
    }

    /// 放弃分片上传,清理服务端已上传的分片。默认无操作。
    async fn abort_multipart(&self, _path: &str, _upload_id: &str) -> Result<()> {
        Ok(())
    }

    /// 列举某个 bucket 下所有未完成(残留)的分片上传——已初始化但未完成 / 中止,
    /// 其分片仍在计费。默认 [`ProviderError::Unsupported`];支持的适配层覆盖并置
    /// [`capabilities`](Self::capabilities) 的 `multipart_cleanup = true`。清理时对每项
    /// 调 [`abort_multipart`](Self::abort_multipart)。
    async fn list_incomplete_uploads(&self, _bucket: &str) -> Result<Vec<IncompleteUpload>> {
        Err(ProviderError::Unsupported("list incomplete uploads".into()))
    }

    /// 转换对象的存储类型 / 归档层(如标准 → 低频 / 归档)。`class` 为厂商的存储类型字符串。
    ///
    /// 通常通过"带新存储类型头的服务端自我复制"实现。默认 [`ProviderError::Unsupported`],
    /// 支持的适配层覆盖并在 [`capabilities`](Self::capabilities) 置 `storage_class_ops = true`。
    async fn set_storage_class(&self, _path: &str, _class: &str) -> Result<()> {
        Err(ProviderError::Unsupported("set storage class".into()))
    }

    /// 取回(解冻)归档 / 冷归档对象,`days` 为取回后可读的保持天数。归档对象在取回完成前
    /// 无法直接下载。默认 [`ProviderError::Unsupported`]。
    async fn restore(&self, _path: &str, _days: u32) -> Result<()> {
        Err(ProviderError::Unsupported("restore archived object".into()))
    }

    /// 设置对象为公开读(`public = true`)或私有(通过预置 ACL)。设为公开读后可用
    /// [`public_url`](Self::public_url) 拿永久直链。默认 [`ProviderError::Unsupported`];
    /// 支持的适配层覆盖并置 [`capabilities`](Self::capabilities) 的 `object_acl = true`。
    async fn set_object_acl(&self, _path: &str, _public: bool) -> Result<()> {
        Err(ProviderError::Unsupported("object acl".into()))
    }

    /// 对象的永久公共直链(不签名);仅当对象为公开读时可访问。默认 `None`,支持的适配层返回
    /// `Some(url)`。同步方法(纯 URL 拼接,不发请求)。
    fn public_url(&self, _path: &str) -> Option<String> {
        None
    }

    /// 修改对象的内容类型(`Content-Type`)。通常通过"带新 Content-Type 且元数据指令为
    /// REPLACE 的自我复制"实现。默认 [`ProviderError::Unsupported`];支持的适配层覆盖并在
    /// [`capabilities`](Self::capabilities) 置 `metadata_ops = true`。
    async fn set_content_type(&self, _path: &str, _content_type: &str) -> Result<()> {
        Err(ProviderError::Unsupported("set content type".into()))
    }

    /// 读取对象标签(键值对)。默认 [`ProviderError::Unsupported`];支持的适配层覆盖并在
    /// [`capabilities`](Self::capabilities) 置 `object_tagging = true`。
    async fn object_tags(&self, _path: &str) -> Result<Vec<(String, String)>> {
        Err(ProviderError::Unsupported("object tagging".into()))
    }

    /// 覆盖对象标签(整套替换;空列表即清空)。默认 [`ProviderError::Unsupported`]。
    async fn set_object_tags(&self, _path: &str, _tags: &[(String, String)]) -> Result<()> {
        Err(ProviderError::Unsupported("object tagging".into()))
    }

    /// 新建一个 bucket。默认 [`ProviderError::Unsupported`];支持的适配层覆盖并在
    /// [`capabilities`](Self::capabilities) 置 `bucket_ops = true`。
    async fn create_bucket(&self, _bucket: &str) -> Result<()> {
        Err(ProviderError::Unsupported("create bucket".into()))
    }

    /// 删除一个 bucket(通常要求为空)。默认 [`ProviderError::Unsupported`]。
    async fn delete_bucket(&self, _bucket: &str) -> Result<()> {
        Err(ProviderError::Unsupported("delete bucket".into()))
    }

    /// 读取一个 bucket 的生命周期规则。默认 [`ProviderError::Unsupported`];支持的适配层
    /// 覆盖并在 [`capabilities`](Self::capabilities) 置 `bucket_lifecycle = true`。
    async fn bucket_lifecycle(&self, _bucket: &str) -> Result<Vec<LifecycleRule>> {
        Err(ProviderError::Unsupported("bucket lifecycle".into()))
    }

    /// 设置一个 bucket 的生命周期规则(**整套替换**,不是增量 patch——各家云的
    /// `PUT lifecycle` 语义都是整体覆盖)。默认 [`ProviderError::Unsupported`]。
    async fn set_bucket_lifecycle(&self, _bucket: &str, _rules: &[LifecycleRule]) -> Result<()> {
        Err(ProviderError::Unsupported("bucket lifecycle".into()))
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

    /// 生成一个 `expires_secs` 秒后失效的预签名**上传**链接(持链接者可直接 PUT 上传到该路径)。
    /// 默认不支持;支持预签名的适配层覆盖此方法。
    async fn presign_put(&self, _path: &str, _expires_secs: u64) -> Result<String> {
        Err(ProviderError::Unsupported("presign upload".into()))
    }
}
