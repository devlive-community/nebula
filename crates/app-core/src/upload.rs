//! 可断点续传的大文件上传。
//!
//! 大文件走分片上传;每传完一个分片就把 `(upload_id + 已完成分片)` 持久化到 SQLite,
//! 因此上传中断(网络断开、App 退出)后,重新发起**同一个**「本地文件 → 远端 key」的上传会
//! **续传**——复用同一个 `upload_id`、跳过已传分片,只补剩下的。识别"同一次上传"靠
//! `(账号, 远端路径, 本地路径, 文件大小, 修改时间)` 全部一致。
//!
//! 只对超过一个分片大小的文件启用;小文件、以及 `resumable_upload` 能力为 false 的 provider
//! 回退到整体上传。出错时**不**放弃服务端已上传分片(不 abort),以便下次续传。

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};

use bytes::{Bytes, BytesMut};
use tokio::io::{AsyncReadExt, AsyncSeekExt};

use crate::content_type::guess_content_type;
use crate::store::UploadSessionRow;
use crate::{App, AppError, ProgressFn, Result};

/// 默认分片大小:8 MiB。
pub(crate) const PART_SIZE: u64 = 8 * 1024 * 1024;

impl App {
    /// 上传本地文件到远端,大文件走**可断点续传**的分片上传。
    ///
    /// 中断后重发同一个 `(账号, 远端路径, 本地文件)` 会续传;小文件或 provider 不支持续传时
    /// 回退到整体上传。`progress(已上传字节, 总字节)`。
    ///
    /// `cancel` 置位后,下一个分片前会中止并返回 [`AppError::Cancelled`];已传分片与会话保留,
    /// 之后重发即续传(取消 = 暂停)。
    pub async fn upload_resumable(
        &self,
        account: &str,
        remote_path: &str,
        local_path: &str,
        content_type: Option<&str>,
        cancel: &AtomicBool,
        progress: ProgressFn<'_>,
    ) -> Result<()> {
        self.upload_resumable_parted(
            account,
            remote_path,
            local_path,
            content_type,
            PART_SIZE,
            cancel,
            progress,
        )
        .await
    }

    /// 同 [`upload_resumable`](Self::upload_resumable),但可指定分片大小(测试用小分片验证续传)。
    #[allow(clippy::too_many_arguments)]
    async fn upload_resumable_parted(
        &self,
        account: &str,
        remote_path: &str,
        local_path: &str,
        content_type: Option<&str>,
        part_size: u64,
        cancel: &AtomicBool,
        progress: ProgressFn<'_>,
    ) -> Result<()> {
        let provider = self.provider(account)?;
        // 调用方未显式指定时,按远端路径的扩展名推断,避免对象存储端把 Content-Type
        // 留空(通常回退成 application/octet-stream),导致图片/视频/PDF 无法预览。
        let content_type = content_type.or_else(|| guess_content_type(remote_path));
        let meta = tokio::fs::metadata(local_path).await?;
        let size = meta.len();

        // 小文件或不支持续传:整体上传(读入内存)。
        if size <= part_size || !provider.capabilities().resumable_upload {
            let data = tokio::fs::read(local_path).await?;
            return Ok(provider
                .write_with_progress(remote_path, Bytes::from(data), content_type, progress)
                .await?);
        }

        let mtime = mtime_secs(&meta);
        let key = session_key(account, remote_path, local_path);

        // 恢复匹配的会话,否则新开一个。
        let (upload_id, mut done) = match self.load_session(&key, size, mtime, part_size) {
            Some(resumed) => resumed,
            None => {
                let uid = provider.begin_multipart(remote_path, content_type).await?;
                let parts: Vec<(u32, String)> = Vec::new();
                self.save_session(&key, &uid, size, mtime, part_size, &parts);
                (uid, parts)
            }
        };

        let num_parts = size.div_ceil(part_size) as u32;
        let done_nums: HashSet<u32> = done.iter().map(|(n, _)| *n).collect();
        let mut uploaded: u64 = done_nums
            .iter()
            .map(|n| part_len(*n, num_parts, size, part_size))
            .sum();
        progress(uploaded, size);

        let mut file = tokio::fs::File::open(local_path).await?;
        for n in 1..=num_parts {
            if done_nums.contains(&n) {
                continue;
            }
            // 取消:中止但保留会话与已传分片,重发即续传。
            if cancel.load(Ordering::Relaxed) {
                return Err(AppError::Cancelled);
            }
            let len = part_len(n, num_parts, size, part_size);
            let bytes = read_part(&mut file, (n as u64 - 1) * part_size, len).await?;
            let etag = provider
                .upload_part(remote_path, &upload_id, n, bytes)
                .await?;
            done.push((n, etag));
            self.save_session(&key, &upload_id, size, mtime, part_size, &done);
            uploaded += len;
            progress(uploaded, size);
        }

        done.sort_by_key(|(n, _)| *n);
        provider
            .complete_multipart(remote_path, &upload_id, &done)
            .await?;
        self.delete_session(&key);
        Ok(())
    }

    // ---- 会话持久化(store 缺失时为空操作:不跨重启续传,但上传仍正常)----

    fn load_session(
        &self,
        key: &str,
        size: u64,
        mtime: u64,
        part_size: u64,
    ) -> Option<(String, Vec<(u32, String)>)> {
        let row = self
            .store
            .as_ref()?
            .get_upload_session(key)
            .ok()
            .flatten()?;
        if row.size == size && row.mtime == mtime && row.part_size == part_size {
            let parts = serde_json::from_str(&row.parts).ok()?;
            Some((row.upload_id, parts))
        } else {
            None
        }
    }

    fn save_session(
        &self,
        key: &str,
        upload_id: &str,
        size: u64,
        mtime: u64,
        part_size: u64,
        parts: &[(u32, String)],
    ) {
        if let Some(store) = &self.store {
            let row = UploadSessionRow {
                upload_id: upload_id.to_string(),
                size,
                mtime,
                part_size,
                parts: serde_json::to_string(parts).unwrap_or_else(|_| "[]".into()),
            };
            let _ = store.put_upload_session(key, &row);
        }
    }

    fn delete_session(&self, key: &str) {
        if let Some(store) = &self.store {
            let _ = store.delete_upload_session(key);
        }
    }
}

/// 会话键:对 `(账号, 远端路径, 本地路径)` 取 MD5,稳定且紧凑。
fn session_key(account: &str, remote: &str, local: &str) -> String {
    cloud_core::crypto::md5_hex(format!("{account}\u{0}{remote}\u{0}{local}").as_bytes())
}

/// 本地文件修改时间(秒);取不到时用 0。用于识别文件是否被改过。
fn mtime_secs(meta: &std::fs::Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 第 `n` 个分片(从 1 计)的字节数:最后一片可能不足 `part_size`。
fn part_len(n: u32, num_parts: u32, size: u64, part_size: u64) -> u64 {
    if n == num_parts {
        size - (num_parts as u64 - 1) * part_size
    } else {
        part_size
    }
}

/// 从 `file` 的 `offset` 处读 `len` 字节。
async fn read_part(file: &mut tokio::fs::File, offset: u64, len: u64) -> Result<Bytes> {
    use std::io::SeekFrom;
    file.seek(SeekFrom::Start(offset)).await?;
    let mut buf = BytesMut::zeroed(len as usize);
    file.read_exact(&mut buf).await?;
    Ok(buf.freeze())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MemorySecrets, SecretStore};
    use async_trait::async_trait;
    use nebula_provider::{Capabilities, Entry, ProviderError, StorageProvider};
    use std::sync::{Arc, Mutex};

    /// 记录分片上传的测试 provider,可让某个分片首次失败以模拟中断。
    struct ResumableRec {
        parts: Mutex<Vec<(u32, usize)>>, // (分片号, 字节数)
        completed: Mutex<Option<Vec<(u32, String)>>>,
        fail_part_once: Mutex<Option<u32>>,
    }

    impl ResumableRec {
        fn new(fail_part_once: Option<u32>) -> Self {
            Self {
                parts: Mutex::new(Vec::new()),
                completed: Mutex::new(None),
                fail_part_once: Mutex::new(fail_part_once),
            }
        }
    }

    #[async_trait]
    impl StorageProvider for ResumableRec {
        fn id(&self) -> &str {
            "rec"
        }
        fn capabilities(&self) -> Capabilities {
            Capabilities {
                resumable_upload: true,
                ..Default::default()
            }
        }
        async fn list(&self, _path: &str) -> nebula_provider::Result<Vec<Entry>> {
            Ok(vec![])
        }
        async fn stat(&self, path: &str) -> nebula_provider::Result<Entry> {
            Err(ProviderError::NotFound(path.into()))
        }
        async fn read(&self, _path: &str) -> nebula_provider::Result<Bytes> {
            Ok(Bytes::new())
        }
        async fn write(
            &self,
            _path: &str,
            _data: Bytes,
            _ct: Option<&str>,
        ) -> nebula_provider::Result<()> {
            Ok(())
        }
        async fn delete(&self, _path: &str) -> nebula_provider::Result<()> {
            Ok(())
        }
        async fn begin_multipart(
            &self,
            _path: &str,
            _ct: Option<&str>,
        ) -> nebula_provider::Result<String> {
            Ok("uid".into())
        }
        async fn upload_part(
            &self,
            _path: &str,
            _upload_id: &str,
            part_number: u32,
            data: Bytes,
        ) -> nebula_provider::Result<String> {
            let mut fail = self.fail_part_once.lock().unwrap();
            if *fail == Some(part_number) {
                *fail = None; // 只失败一次
                return Err(ProviderError::Backend("simulated interruption".into()));
            }
            self.parts.lock().unwrap().push((part_number, data.len()));
            Ok(format!("etag{part_number}"))
        }
        async fn complete_multipart(
            &self,
            _path: &str,
            _upload_id: &str,
            parts: &[(u32, String)],
        ) -> nebula_provider::Result<()> {
            *self.completed.lock().unwrap() = Some(parts.to_vec());
            Ok(())
        }
    }

    fn temp_file(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("nebula-upload-{name}"));
        std::fs::write(&p, bytes).unwrap();
        p
    }

    fn app_with_store(name: &str, provider: Arc<ResumableRec>) -> (crate::App, std::path::PathBuf) {
        let db = std::env::temp_dir().join(format!("nebula-updb-{name}.sqlite"));
        let _ = std::fs::remove_file(&db);
        let secrets: Arc<dyn SecretStore> = Arc::new(MemorySecrets::default());
        let app = crate::App::with_store_and_secrets(&db, secrets).unwrap();
        app.add_account(provider);
        (app, db)
    }

    #[tokio::test]
    async fn resumable_upload_sends_all_parts_in_order() {
        let rec = Arc::new(ResumableRec::new(None));
        let (app, db) = app_with_store("all", rec.clone());
        let file = temp_file("all", &[7u8; 10]); // 10 字节,分片 4 → (4,4,2)

        app.upload_resumable_parted(
            "rec",
            "b/k",
            file.to_str().unwrap(),
            None,
            4,
            &AtomicBool::new(false),
            &|_, _| {},
        )
        .await
        .unwrap();

        assert_eq!(*rec.parts.lock().unwrap(), vec![(1, 4), (2, 4), (3, 2)]);
        let completed = rec.completed.lock().unwrap().clone().unwrap();
        assert_eq!(
            completed,
            vec![
                (1, "etag1".to_string()),
                (2, "etag2".to_string()),
                (3, "etag3".to_string())
            ]
        );
        let _ = std::fs::remove_file(&file);
        let _ = std::fs::remove_file(&db);
    }

    #[tokio::test]
    async fn resume_after_interruption_skips_completed_parts() {
        // 让分片 2 首次失败。
        let rec = Arc::new(ResumableRec::new(Some(2)));
        let (app, db) = app_with_store("resume", rec.clone());
        let file = temp_file("resume", &[9u8; 10]); // (4,4,2)
        let path = file.to_str().unwrap();

        // 第一次:分片 1 成功、分片 2 失败 → 整体报错,会话已持久化(含分片 1)。
        assert!(app
            .upload_resumable_parted(
                "rec",
                "b/k",
                path,
                None,
                4,
                &AtomicBool::new(false),
                &|_, _| {}
            )
            .await
            .is_err());
        assert_eq!(*rec.parts.lock().unwrap(), vec![(1, 4)]);
        assert!(rec.completed.lock().unwrap().is_none());

        // 第二次:续传,应只补分片 2、3,分片 1 不再重传。
        app.upload_resumable_parted(
            "rec",
            "b/k",
            path,
            None,
            4,
            &AtomicBool::new(false),
            &|_, _| {},
        )
        .await
        .unwrap();
        assert_eq!(*rec.parts.lock().unwrap(), vec![(1, 4), (2, 4), (3, 2)]);
        assert!(rec.completed.lock().unwrap().is_some());

        let _ = std::fs::remove_file(&file);
        let _ = std::fs::remove_file(&db);
    }

    #[tokio::test]
    async fn cancelled_upload_stops_and_resumes_later() {
        let rec = Arc::new(ResumableRec::new(None));
        let (app, db) = app_with_store("cancel", rec.clone());
        let file = temp_file("cancel", &[5u8; 10]); // (4,4,2)
        let path = file.to_str().unwrap();

        // 取消已置位:第一个分片前就中止,什么都没传。
        let cancel = AtomicBool::new(true);
        let out = app
            .upload_resumable_parted("rec", "b/k", path, None, 4, &cancel, &|_, _| {})
            .await;
        assert!(matches!(out, Err(AppError::Cancelled)));
        assert!(rec.parts.lock().unwrap().is_empty());

        // 清除取消后重发:正常传完。
        let go = AtomicBool::new(false);
        app.upload_resumable_parted("rec", "b/k", path, None, 4, &go, &|_, _| {})
            .await
            .unwrap();
        assert_eq!(*rec.parts.lock().unwrap(), vec![(1, 4), (2, 4), (3, 2)]);
        assert!(rec.completed.lock().unwrap().is_some());

        let _ = std::fs::remove_file(&file);
        let _ = std::fs::remove_file(&db);
    }

    #[tokio::test]
    async fn small_file_falls_back_to_whole_upload() {
        let rec = Arc::new(ResumableRec::new(None));
        let (app, db) = app_with_store("small", rec.clone());
        let file = temp_file("small", b"hi"); // 2 字节 <= 分片 4 → 整体上传

        app.upload_resumable_parted(
            "rec",
            "b/k",
            file.to_str().unwrap(),
            None,
            4,
            &AtomicBool::new(false),
            &|_, _| {},
        )
        .await
        .unwrap();

        // 未走分片路径。
        assert!(rec.parts.lock().unwrap().is_empty());
        assert!(rec.completed.lock().unwrap().is_none());
        let _ = std::fs::remove_file(&file);
        let _ = std::fs::remove_file(&db);
    }
}
