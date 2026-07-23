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

use serde::{Deserialize, Serialize};

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
