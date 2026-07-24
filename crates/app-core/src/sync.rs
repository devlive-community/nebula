//! 同步与备份的 **Diff 引擎**(纯逻辑,可单测)。
//!
//! 给定本地与云端两侧的文件清单(按相对路径),算出每个文件该做的动作。执行在
//! [`crate`] 的 App 方法里,复用现有的传输 / 秒传 / 限速基建;本模块只决定「做什么」。
//!
//! 三种模式:
//! - [`SyncMode::MirrorUp`] 备份:本地 → 云端(`delete_extra` 时删云端多余);
//! - [`SyncMode::MirrorDown`] 还原:云端 → 本地(`delete_extra` 时删本地多余);
//! - [`SyncMode::TwoWay`] 双向:两边都新增 / 改动都同步过去;需上次同步快照区分「删除」
//!   与「新增」并检测冲突,属阶段 3,本阶段仅按「较新/存在即传」做无状态近似。
//!
//! **同 / 异判定**(保守但避免无谓重传):
//! - 大小不同 → 改动;
//! - 大小相同且两侧哈希都已知 → 按哈希是否相等;
//! - 大小相同但哈希无法取得(如分片 ETag、云端未给 MD5)→ 视为相同(size-only 回退),
//!   避免每次同步都重传大文件。调用方仅在「值得比对」(云端 ETag 是整对象 MD5 且大小一致)
//!   时才读盘算本地 MD5,和秒传 [`crate::dedup`] 一致。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use futures::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;

use crate::{App, AppError, Result};

/// 同步模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncMode {
    /// 备份:本地 → 云端。
    MirrorUp,
    /// 还原:云端 → 本地。
    MirrorDown,
    /// 双向。
    TwoWay,
}

/// 单个文件要执行的动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncAction {
    /// 上传:本地 → 云端。
    Upload,
    /// 下载:云端 → 本地。
    Download,
    /// 删除云端多余对象。
    DeleteRemote,
    /// 删除本地多余文件。
    DeleteLocal,
    /// 无需动作(两侧一致)。
    Skip,
}

/// 本地一侧的文件元信息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalFile {
    pub size: u64,
    /// 内容 MD5(小写十六进制);仅在「值得比对」时才由调用方算出,否则 `None`。
    pub md5: Option<String>,
}

/// 云端一侧的文件元信息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteFile {
    pub size: u64,
    /// 云端 ETag(可能是整对象 MD5,也可能是分片 ETag 或缺失)。
    pub etag: Option<String>,
}

/// Diff 的一条结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffItem {
    /// 相对路径(相对同步根;用 `/` 分隔,跨平台统一)。
    pub rel_path: String,
    pub action: SyncAction,
    /// 本地大小(存在时)。
    pub local_size: Option<u64>,
    /// 云端大小(存在时)。
    pub remote_size: Option<u64>,
}

/// 两侧是否内容相同。见模块文档的判定规则。
fn same(local: &LocalFile, remote: &RemoteFile) -> bool {
    if local.size != remote.size {
        return false;
    }
    match (
        local.md5.as_deref(),
        remote
            .etag
            .as_deref()
            .and_then(crate::integrity::etag_as_md5),
    ) {
        (Some(l), Some(r)) => l.eq_ignore_ascii_case(&r),
        // 哈希取不到 → size-only 回退,视为相同。
        _ => true,
    }
}

/// 计算 diff。`local` / `remote` 均以相对路径为键。`delete_extra` 决定镜像模式下是否
/// 删除目标侧多余文件(双向模式忽略此项)。返回按相对路径升序排列,便于稳定展示 / 测试。
pub fn diff(
    local: &BTreeMap<String, LocalFile>,
    remote: &BTreeMap<String, RemoteFile>,
    mode: SyncMode,
    delete_extra: bool,
) -> Vec<DiffItem> {
    // 相对路径全集。
    let mut paths: Vec<&String> = local.keys().chain(remote.keys()).collect();
    paths.sort_unstable();
    paths.dedup();

    let mut out = Vec::with_capacity(paths.len());
    for p in paths {
        let l = local.get(p);
        let r = remote.get(p);
        let action = match (l, r) {
            (Some(lf), Some(rf)) => {
                if same(lf, rf) {
                    SyncAction::Skip
                } else {
                    match mode {
                        // 两侧都有但不同:镜像按方向传;双向暂按「上传本地」近似(阶段 3 再精确)。
                        SyncMode::MirrorUp | SyncMode::TwoWay => SyncAction::Upload,
                        SyncMode::MirrorDown => SyncAction::Download,
                    }
                }
            }
            (Some(_), None) => match mode {
                // 只有本地:上传;还原模式下按需删本地多余。
                SyncMode::MirrorUp | SyncMode::TwoWay => SyncAction::Upload,
                SyncMode::MirrorDown => {
                    if delete_extra {
                        SyncAction::DeleteLocal
                    } else {
                        SyncAction::Skip
                    }
                }
            },
            (None, Some(_)) => match mode {
                // 只有云端:下载;备份模式下按需删云端多余。
                SyncMode::MirrorDown | SyncMode::TwoWay => SyncAction::Download,
                SyncMode::MirrorUp => {
                    if delete_extra {
                        SyncAction::DeleteRemote
                    } else {
                        SyncAction::Skip
                    }
                }
            },
            (None, None) => SyncAction::Skip, // 不会发生
        };
        out.push(DiffItem {
            rel_path: p.clone(),
            action,
            local_size: l.map(|f| f.size),
            remote_size: r.map(|f| f.size),
        });
    }
    out
}

/// Diff 的动作汇总(给前端预览用:各类多少个、涉及多少字节)。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffSummary {
    pub upload: usize,
    pub download: usize,
    pub delete_remote: usize,
    pub delete_local: usize,
    pub skip: usize,
    /// 需要传输(上传 + 下载)的总字节数。
    pub transfer_bytes: u64,
}

/// 汇总一次 diff 的动作分布。
pub fn summarize(items: &[DiffItem]) -> DiffSummary {
    let mut s = DiffSummary::default();
    for it in items {
        match it.action {
            SyncAction::Upload => {
                s.upload += 1;
                s.transfer_bytes += it.local_size.unwrap_or(0);
            }
            SyncAction::Download => {
                s.download += 1;
                s.transfer_bytes += it.remote_size.unwrap_or(0);
            }
            SyncAction::DeleteRemote => s.delete_remote += 1,
            SyncAction::DeleteLocal => s.delete_local += 1,
            SyncAction::Skip => s.skip += 1,
        }
    }
    s
}

/// 一次同步执行的结果统计。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SyncReport {
    pub uploaded: usize,
    pub downloaded: usize,
    pub deleted_remote: usize,
    pub deleted_local: usize,
    pub skipped: usize,
    /// 出错但已跳过继续的文件数。
    pub failed: usize,
    /// 实际传输的字节数(上传 + 下载)。
    pub bytes: u64,
}

/// 一个同步任务的参数(账号 / 本地目录 / 远端前缀 / 模式 / 是否删多余)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncSpec {
    pub account: String,
    pub local_dir: String,
    pub remote_prefix: String,
    pub mode: SyncMode,
    #[serde(default)]
    pub delete_extra: bool,
}

/// 归一化远端前缀:去掉尾部 `/`,便于统一拼接。
fn norm_prefix(p: &str) -> String {
    p.trim_end_matches('/').to_string()
}

/// 远端对象完整路径 = 前缀 + 相对路径。
fn remote_path(prefix: &str, rel: &str) -> String {
    let prefix = norm_prefix(prefix);
    if prefix.is_empty() {
        rel.to_string()
    } else {
        format!("{prefix}/{rel}")
    }
}

impl App {
    /// 递归遍历本地目录,返回 `相对路径(用 /)-> (字节数, 绝对路径)`。
    async fn scan_local(&self, root: &Path) -> Result<BTreeMap<String, (u64, PathBuf)>> {
        let mut out = BTreeMap::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(dir) = stack.pop() {
            let mut rd = match tokio::fs::read_dir(&dir).await {
                Ok(rd) => rd,
                Err(_) => continue, // 目录不可读则跳过
            };
            while let Some(entry) = rd.next_entry().await? {
                let path = entry.path();
                let ft = entry.file_type().await?;
                if ft.is_dir() {
                    stack.push(path);
                } else if ft.is_file() {
                    let meta = entry.metadata().await?;
                    let rel = path
                        .strip_prefix(root)
                        .unwrap_or(&path)
                        .to_string_lossy()
                        .replace('\\', "/");
                    out.insert(rel, (meta.len(), path));
                }
            }
        }
        Ok(out)
    }

    /// 列出远端前缀下所有对象,返回 `相对路径 -> (字节数, ETag)`。
    async fn scan_remote(
        &self,
        account: &str,
        prefix: &str,
    ) -> Result<BTreeMap<String, (u64, Option<String>)>> {
        let base = norm_prefix(prefix);
        let files = self.list_all_files(account, prefix).await?;
        let mut out = BTreeMap::new();
        for f in files {
            let rel = f
                .path
                .strip_prefix(&base)
                .map(|s| s.trim_start_matches('/').to_string())
                .unwrap_or_else(|| f.path.clone());
            if rel.is_empty() {
                continue;
            }
            out.insert(rel, (f.size, f.etag));
        }
        Ok(out)
    }

    /// 扫两侧、构建 diff 输入(仅对「值得比对」的候选算本地 MD5),返回 diff 结果与两侧原始表。
    async fn sync_diff_maps(
        &self,
        spec: &SyncSpec,
    ) -> Result<(
        Vec<DiffItem>,
        BTreeMap<String, (u64, PathBuf)>,
        BTreeMap<String, (u64, Option<String>)>,
    )> {
        let local_raw = self.scan_local(Path::new(&spec.local_dir)).await?;
        let remote_raw = self.scan_remote(&spec.account, &spec.remote_prefix).await?;

        // 构建云端 map。
        let mut remote: BTreeMap<String, RemoteFile> = BTreeMap::new();
        for (rel, (size, etag)) in &remote_raw {
            remote.insert(
                rel.clone(),
                RemoteFile {
                    size: *size,
                    etag: etag.clone(),
                },
            );
        }
        // 构建本地 map;仅当两侧同大小且云端 ETag 是整对象 MD5 时才算本地 MD5(和秒传一致)。
        let mut local: BTreeMap<String, LocalFile> = BTreeMap::new();
        for (rel, (size, abs)) in &local_raw {
            let md5 = match remote_raw.get(rel) {
                Some((rsize, retag))
                    if rsize == size
                        && retag
                            .as_deref()
                            .and_then(crate::integrity::etag_as_md5)
                            .is_some() =>
                {
                    crate::dedup::file_md5(&abs.to_string_lossy()).await.ok()
                }
                _ => None,
            };
            local.insert(rel.clone(), LocalFile { size: *size, md5 });
        }

        let items = diff(&local, &remote, spec.mode, spec.delete_extra);
        Ok((items, local_raw, remote_raw))
    }

    /// 预览同步:返回每个文件的动作与汇总,不改动任何数据。
    pub async fn sync_preview(&self, spec: &SyncSpec) -> Result<(Vec<DiffItem>, DiffSummary)> {
        let (items, _, _) = self.sync_diff_maps(spec).await?;
        let summary = summarize(&items);
        Ok((items, summary))
    }

    /// 执行同步:按 diff 动作逐个处理。单个文件出错记为 failed 并继续,`cancel` 置位即中止。
    /// `progress(已处理文件数, 总动作数)`。
    pub async fn sync_run(
        &self,
        spec: &SyncSpec,
        cancel: &AtomicBool,
        progress: nebula_provider::ProgressFn<'_>,
    ) -> Result<SyncReport> {
        let account = &spec.account;
        let remote_prefix = &spec.remote_prefix;
        let local_root = Path::new(&spec.local_dir);
        let (items, local_raw, _) = self.sync_diff_maps(spec).await?;

        let actionable: Vec<&DiffItem> = items
            .iter()
            .filter(|i| i.action != SyncAction::Skip)
            .collect();
        let total = actionable.len() as u64;
        let mut report = SyncReport::default();
        let noop: nebula_provider::ProgressFn = &|_, _| {};

        for (done, it) in actionable.iter().enumerate() {
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            let rkey = remote_path(remote_prefix, &it.rel_path);
            let result: Result<()> = match it.action {
                SyncAction::Upload => {
                    let abs = local_raw
                        .get(&it.rel_path)
                        .map(|(_, p)| p.to_string_lossy().to_string());
                    match abs {
                        Some(abs) => self
                            .upload_resumable(account, &rkey, &abs, None, cancel, noop)
                            .await
                            .map(|_| {
                                report.uploaded += 1;
                                report.bytes += it.local_size.unwrap_or(0);
                            }),
                        None => Ok(()),
                    }
                }
                SyncAction::Download => {
                    let dest = local_root.join(rel_to_native(&it.rel_path));
                    self.download_to_file(account, &rkey, &dest, cancel)
                        .await
                        .map(|_| {
                            report.downloaded += 1;
                            report.bytes += it.remote_size.unwrap_or(0);
                        })
                }
                SyncAction::DeleteRemote => self.delete(account, &rkey).await.map(|_| {
                    report.deleted_remote += 1;
                }),
                SyncAction::DeleteLocal => {
                    let abs = local_root.join(rel_to_native(&it.rel_path));
                    tokio::fs::remove_file(&abs)
                        .await
                        .map_err(AppError::from)
                        .map(|_| {
                            report.deleted_local += 1;
                        })
                }
                SyncAction::Skip => Ok(()),
            };
            if result.is_err() {
                report.failed += 1;
            }
            progress((done + 1) as u64, total);
        }
        Ok(report)
    }

    /// 流式下载远端对象到本地文件(先写 `.part` 再原子重命名,建好父目录)。
    async fn download_to_file(
        &self,
        account: &str,
        remote_path: &str,
        dest: &Path,
        cancel: &AtomicBool,
    ) -> Result<()> {
        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let part = dest.with_extension("nebula-part");
        let (_, mut stream) = self.provider(account)?.read_stream(remote_path).await?;
        let mut file = tokio::fs::File::create(&part).await?;
        while let Some(chunk) = stream.next().await {
            if cancel.load(Ordering::Relaxed) {
                let _ = tokio::fs::remove_file(&part).await;
                return Err(AppError::InvalidInput("cancelled".into()));
            }
            let chunk = chunk?;
            file.write_all(&chunk).await?;
        }
        file.flush().await?;
        drop(file);
        tokio::fs::rename(&part, dest).await?;
        Ok(())
    }
}

/// 相对路径(用 `/`)转成本地原生分隔符的相对 PathBuf。
fn rel_to_native(rel: &str) -> PathBuf {
    rel.split('/').collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lf(size: u64, md5: Option<&str>) -> LocalFile {
        LocalFile {
            size,
            md5: md5.map(|s| s.to_string()),
        }
    }
    fn rf(size: u64, etag: Option<&str>) -> RemoteFile {
        RemoteFile {
            size,
            etag: etag.map(|s| s.to_string()),
        }
    }
    fn local(pairs: &[(&str, LocalFile)]) -> BTreeMap<String, LocalFile> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }
    fn remote(pairs: &[(&str, RemoteFile)]) -> BTreeMap<String, RemoteFile> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }
    fn action_of<'a>(items: &'a [DiffItem], p: &str) -> Option<&'a SyncAction> {
        items.iter().find(|i| i.rel_path == p).map(|i| &i.action)
    }

    const MD5_A: &str = "0cc175b9c0f1b6a831c399e269772661"; // "a"
    const MD5_B: &str = "92eb5ffee6ae2fec3ad71c777531578f"; // "b"

    #[test]
    fn same_by_md5_when_available() {
        let l = local(&[("f", lf(1, Some(MD5_A)))]);
        let r = remote(&[("f", rf(1, Some(MD5_A)))]);
        assert_eq!(
            action_of(&diff(&l, &r, SyncMode::MirrorUp, false), "f"),
            Some(&SyncAction::Skip)
        );
    }

    #[test]
    fn changed_when_md5_differs() {
        let l = local(&[("f", lf(1, Some(MD5_A)))]);
        let r = remote(&[("f", rf(1, Some(MD5_B)))]);
        assert_eq!(
            action_of(&diff(&l, &r, SyncMode::MirrorUp, false), "f"),
            Some(&SyncAction::Upload)
        );
        assert_eq!(
            action_of(&diff(&l, &r, SyncMode::MirrorDown, false), "f"),
            Some(&SyncAction::Download)
        );
    }

    #[test]
    fn size_only_fallback_treats_equal_size_as_same() {
        // 云端分片 ETag(无法当 MD5)+ 本地未算 md5 → size 相同即视为一致。
        let l = local(&[("f", lf(100, None))]);
        let r = remote(&[("f", rf(100, Some("abc-2")))]);
        assert_eq!(
            action_of(&diff(&l, &r, SyncMode::MirrorUp, false), "f"),
            Some(&SyncAction::Skip)
        );
    }

    #[test]
    fn different_size_is_changed() {
        let l = local(&[("f", lf(100, None))]);
        let r = remote(&[("f", rf(200, None))]);
        assert_eq!(
            action_of(&diff(&l, &r, SyncMode::MirrorUp, false), "f"),
            Some(&SyncAction::Upload)
        );
    }

    #[test]
    fn mirror_up_uploads_new_and_deletes_extra() {
        let l = local(&[("only_local", lf(1, Some(MD5_A)))]);
        let r = remote(&[("only_remote", rf(1, Some(MD5_A)))]);
        let d = diff(&l, &r, SyncMode::MirrorUp, true);
        assert_eq!(action_of(&d, "only_local"), Some(&SyncAction::Upload));
        assert_eq!(
            action_of(&d, "only_remote"),
            Some(&SyncAction::DeleteRemote)
        );
        // 不删多余时:云端多余的保留(Skip)。
        let d2 = diff(&l, &r, SyncMode::MirrorUp, false);
        assert_eq!(action_of(&d2, "only_remote"), Some(&SyncAction::Skip));
    }

    #[test]
    fn mirror_down_downloads_new_and_deletes_extra() {
        let l = local(&[("only_local", lf(1, Some(MD5_A)))]);
        let r = remote(&[("only_remote", rf(1, Some(MD5_A)))]);
        let d = diff(&l, &r, SyncMode::MirrorDown, true);
        assert_eq!(action_of(&d, "only_remote"), Some(&SyncAction::Download));
        assert_eq!(action_of(&d, "only_local"), Some(&SyncAction::DeleteLocal));
    }

    #[test]
    fn two_way_pushes_both_sides() {
        let l = local(&[("only_local", lf(1, Some(MD5_A)))]);
        let r = remote(&[("only_remote", rf(1, Some(MD5_A)))]);
        let d = diff(&l, &r, SyncMode::TwoWay, false);
        assert_eq!(action_of(&d, "only_local"), Some(&SyncAction::Upload));
        assert_eq!(action_of(&d, "only_remote"), Some(&SyncAction::Download));
    }

    #[test]
    fn summary_counts_and_bytes() {
        let l = local(&[("a", lf(10, Some(MD5_A))), ("b", lf(20, None))]);
        let r = remote(&[("a", rf(10, Some(MD5_B))), ("c", rf(5, None))]);
        // MirrorUp+delete: a 改动→Upload(10), b 只在本地→Upload(20), c 只在云端→DeleteRemote
        let d = diff(&l, &r, SyncMode::MirrorUp, true);
        let s = summarize(&d);
        assert_eq!(s.upload, 2);
        assert_eq!(s.delete_remote, 1);
        assert_eq!(s.transfer_bytes, 30);
    }
}
