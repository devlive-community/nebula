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
    /// 冲突(双向:两侧自上次同步后都改了)——不动数据,交用户处理。
    Conflict,
    /// 无需动作(两侧一致)。
    Skip,
}

/// 双向冲突的用户决议:保留哪一侧,或跳过不动。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictChoice {
    /// 保留本地(上传覆盖云端)。
    KeepLocal,
    /// 保留云端(下载覆盖本地)。
    KeepRemote,
    /// 跳过,保持冲突。
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

/// 上次成功同步时记录的一个文件签名(用于双向区分「删除」与「新增」)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestEntry {
    pub size: u64,
    /// 当时的内容哈希(整对象 MD5);取不到则 `None`(size-only)。
    pub hash: Option<String>,
}

/// 某一侧相对上次同步的变化。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Change {
    /// 与上次一致(或从未记录且当前也不存在)。
    Unchanged,
    /// 新增(上次没有,现在有)。
    Added,
    /// 修改(上次有,现在也有但内容不同)。
    Modified,
    /// 删除(上次有,现在没了)。
    Deleted,
}

/// 本地当前状态相对 manifest 的变化。
fn local_change(cur: Option<&LocalFile>, m: Option<&ManifestEntry>) -> Change {
    match (cur, m) {
        (None, None) => Change::Unchanged,
        (Some(_), None) => Change::Added,
        (None, Some(_)) => Change::Deleted,
        (Some(l), Some(m)) => {
            if l.size != m.size {
                Change::Modified
            } else {
                match (l.md5.as_deref(), m.hash.as_deref()) {
                    (Some(a), Some(b)) if !a.eq_ignore_ascii_case(b) => Change::Modified,
                    _ => Change::Unchanged, // size 相同且哈希取不到 → 视为未变
                }
            }
        }
    }
}

/// 云端当前状态相对 manifest 的变化。
fn remote_change(cur: Option<&RemoteFile>, m: Option<&ManifestEntry>) -> Change {
    match (cur, m) {
        (None, None) => Change::Unchanged,
        (Some(_), None) => Change::Added,
        (None, Some(_)) => Change::Deleted,
        (Some(r), Some(m)) => {
            if r.size != m.size {
                Change::Modified
            } else {
                match (
                    r.etag.as_deref().and_then(crate::integrity::etag_as_md5),
                    m.hash.as_deref(),
                ) {
                    (Some(a), Some(b)) if !a.eq_ignore_ascii_case(b) => Change::Modified,
                    _ => Change::Unchanged,
                }
            }
        }
    }
}

/// 双向同步的 diff:借助上次同步的 `manifest` 区分「某边删除」与「另一边新增」,
/// 两侧自上次后都改动则判为冲突(不动数据)。
pub fn diff_two_way(
    local: &BTreeMap<String, LocalFile>,
    remote: &BTreeMap<String, RemoteFile>,
    manifest: &BTreeMap<String, ManifestEntry>,
) -> Vec<DiffItem> {
    let mut paths: Vec<&String> = local
        .keys()
        .chain(remote.keys())
        .chain(manifest.keys())
        .collect();
    paths.sort_unstable();
    paths.dedup();

    let mut out = Vec::with_capacity(paths.len());
    for p in paths {
        let l = local.get(p);
        let r = remote.get(p);
        let m = manifest.get(p);
        let lc = local_change(l, m);
        let rc = remote_change(r, m);
        use Change::*;
        let action = match (lc, rc) {
            // 两侧都没变。
            (Unchanged, Unchanged) => SyncAction::Skip,
            // 一侧变、另一侧没变:把变化推过去。
            (Added, Unchanged) | (Modified, Unchanged) => SyncAction::Upload,
            (Unchanged, Added) | (Unchanged, Modified) => SyncAction::Download,
            (Deleted, Unchanged) => SyncAction::DeleteRemote,
            (Unchanged, Deleted) => SyncAction::DeleteLocal,
            // 两侧都删了:无事(manifest 里清掉即可)。
            (Deleted, Deleted) => SyncAction::Skip,
            // 两侧都有内容改动:若恰好一致则跳过,否则冲突。
            (Added, Added) | (Added, Modified) | (Modified, Added) | (Modified, Modified) => {
                match (l, r) {
                    (Some(lf), Some(rf)) if same(lf, rf) => SyncAction::Skip,
                    _ => SyncAction::Conflict,
                }
            }
            // 一边删一边改:冲突,别自动丢数据。
            (Deleted, Added) | (Deleted, Modified) | (Added, Deleted) | (Modified, Deleted) => {
                SyncAction::Conflict
            }
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
    #[serde(default)]
    pub conflict: usize,
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
            SyncAction::Conflict => s.conflict += 1,
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

/// 一个同步任务的参数(账号 / 本地目录 / 远端前缀 / 模式 / 是否删多余 / 排除规则)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncSpec {
    pub account: String,
    pub local_dir: String,
    pub remote_prefix: String,
    pub mode: SyncMode,
    #[serde(default)]
    pub delete_extra: bool,
    /// 排除规则(glob):命中的相对路径两侧都忽略。如 `.DS_Store`、`node_modules/**`、`*.tmp`。
    #[serde(default)]
    pub excludes: Vec<String>,
}

/// 一条 glob 规则是否命中某相对路径(用 `/` 分隔)。
///
/// 支持:`*`(不跨 `/` 的任意串)、`**`(跨目录任意串)、`?`(单字符)。规则不含 `/`
/// 时对**任意路径段**匹配(如 `.DS_Store` 命中任何目录下的该文件;`*.tmp` 命中任何 .tmp)。
fn glob_match(pattern: &str, path: &str) -> bool {
    // 不含 `/` 的规则:对每个路径段单独尝试,命中任一即算命中。
    if !pattern.contains('/') {
        return path.split('/').any(|seg| glob_seg(pattern, seg)) || glob_seg(pattern, path);
    }
    glob_seg(pattern, path)
}

/// 在单个字符串上做 glob 匹配(`**` 跨 `/`,`*` 不跨 `/`,`?` 单字符)。
fn glob_seg(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    glob_rec(&p, &t)
}

fn glob_rec(p: &[char], t: &[char]) -> bool {
    if p.is_empty() {
        return t.is_empty();
    }
    match p[0] {
        '*' => {
            // `**` 跨目录:吃掉任意(含 /)。
            if p.get(1) == Some(&'*') {
                let rest = &p[2..];
                // 允许吃掉前导 `/`。
                let rest = if rest.first() == Some(&'/') {
                    &rest[1..]
                } else {
                    rest
                };
                if glob_rec(rest, t) {
                    return true;
                }
                return !t.is_empty() && glob_rec(p, &t[1..]);
            }
            // 单 `*`:不跨 `/`。
            if glob_rec(&p[1..], t) {
                return true;
            }
            !t.is_empty() && t[0] != '/' && glob_rec(p, &t[1..])
        }
        '?' => !t.is_empty() && t[0] != '/' && glob_rec(&p[1..], &t[1..]),
        c => !t.is_empty() && t[0] == c && glob_rec(&p[1..], &t[1..]),
    }
}

/// 相对路径是否被任一排除规则命中。
fn is_excluded(rel: &str, excludes: &[String]) -> bool {
    excludes
        .iter()
        .any(|pat| !pat.trim().is_empty() && glob_match(pat.trim(), rel))
}

/// 一条已保存的同步任务:参数 + 名字 + 定时间隔 + 上次运行时间。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncJob {
    pub id: String,
    pub name: String,
    pub spec: SyncSpec,
    /// 定时间隔(分钟);0 = 仅手动。
    #[serde(default)]
    pub interval_mins: u32,
    /// 上次运行的 Unix 秒;0 = 从未。
    #[serde(default)]
    pub last_run: i64,
}

/// [`SyncMode`] ↔ 存储字符串。
fn mode_to_str(m: SyncMode) -> &'static str {
    match m {
        SyncMode::MirrorUp => "mirror_up",
        SyncMode::MirrorDown => "mirror_down",
        SyncMode::TwoWay => "two_way",
    }
}
fn mode_from_str(s: &str) -> SyncMode {
    match s {
        "mirror_down" => SyncMode::MirrorDown,
        "two_way" => SyncMode::TwoWay,
        _ => SyncMode::MirrorUp,
    }
}

impl App {
    /// 列出所有已保存的同步任务。
    pub fn sync_jobs(&self) -> Vec<SyncJob> {
        let Some(store) = &self.store else {
            return Vec::new();
        };
        store
            .list_sync_jobs()
            .unwrap_or_default()
            .into_iter()
            .map(|r| SyncJob {
                id: r.id,
                name: r.name,
                spec: SyncSpec {
                    account: r.account,
                    local_dir: r.local_dir,
                    remote_prefix: r.remote_prefix,
                    mode: mode_from_str(&r.mode),
                    delete_extra: r.delete_extra,
                    excludes: serde_json::from_str(&r.excludes).unwrap_or_default(),
                },
                interval_mins: r.interval_mins,
                last_run: r.last_run,
            })
            .collect()
    }

    /// 保存(新增或覆盖)一条同步任务。
    pub fn save_sync_job(&self, job: &SyncJob) -> Result<()> {
        if let Some(store) = &self.store {
            let row = crate::store::SyncJobRow {
                id: job.id.clone(),
                name: job.name.clone(),
                account: job.spec.account.clone(),
                local_dir: job.spec.local_dir.clone(),
                remote_prefix: job.spec.remote_prefix.clone(),
                mode: mode_to_str(job.spec.mode).to_string(),
                delete_extra: job.spec.delete_extra,
                excludes: serde_json::to_string(&job.spec.excludes).unwrap_or_else(|_| "[]".into()),
                interval_mins: job.interval_mins,
                last_run: job.last_run,
            };
            store
                .put_sync_job(&row)
                .map_err(|e| AppError::Image(e.to_string()))?;
        }
        Ok(())
    }

    /// 删除一条同步任务(连带其 manifest)。
    pub fn delete_sync_job(&self, id: &str) -> Result<()> {
        if let Some(store) = &self.store {
            store
                .delete_sync_job(id)
                .map_err(|e| AppError::Image(e.to_string()))?;
        }
        Ok(())
    }
}

/// 某同步任务的稳定标识(账号 + 本地目录 + 远端前缀),用作 manifest 的 job 键。
fn sync_job_key(spec: &SyncSpec) -> String {
    format!(
        "{}\u{0}{}\u{0}{}",
        spec.account,
        spec.local_dir,
        norm_prefix(&spec.remote_prefix)
    )
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
        let mut local_raw = self.scan_local(Path::new(&spec.local_dir)).await?;
        let mut remote_raw = self.scan_remote(&spec.account, &spec.remote_prefix).await?;

        // 排除规则:命中的相对路径两侧都剔除,不参与 diff / 执行。
        if !spec.excludes.is_empty() {
            local_raw.retain(|rel, _| !is_excluded(rel, &spec.excludes));
            remote_raw.retain(|rel, _| !is_excluded(rel, &spec.excludes));
        }

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
        // 双向模式:载入上次同步快照(manifest),用于区分「删除」与「新增」。
        let two_way = spec.mode == SyncMode::TwoWay;
        let manifest = if two_way {
            self.load_manifest(&sync_job_key(spec))
        } else {
            BTreeMap::new()
        };

        // 构建本地 map。需要算本地 MD5 的场景:
        // (a) 和秒传一致——两侧同大小且云端 ETag 是整对象 MD5;
        // (b) 双向——manifest 里有该文件的哈希且大小一致(用于判断本地是否改过)。
        let mut local: BTreeMap<String, LocalFile> = BTreeMap::new();
        for (rel, (size, abs)) in &local_raw {
            let worth_remote = matches!(
                remote_raw.get(rel),
                Some((rsize, retag))
                    if rsize == size
                        && retag.as_deref().and_then(crate::integrity::etag_as_md5).is_some()
            );
            let worth_manifest = matches!(
                manifest.get(rel),
                Some(m) if m.size == *size && m.hash.is_some()
            );
            let md5 = if worth_remote || worth_manifest {
                crate::dedup::file_md5(&abs.to_string_lossy()).await.ok()
            } else {
                None
            };
            local.insert(rel.clone(), LocalFile { size: *size, md5 });
        }

        let items = if two_way {
            diff_two_way(&local, &remote, &manifest)
        } else {
            diff(&local, &remote, spec.mode, spec.delete_extra)
        };
        Ok((items, local_raw, remote_raw))
    }

    /// 载入某同步任务的 manifest。无 store 或无记录返回空表。
    fn load_manifest(&self, job: &str) -> BTreeMap<String, ManifestEntry> {
        let mut out = BTreeMap::new();
        if let Some(store) = &self.store {
            if let Ok(rows) = store.sync_manifest_load(job) {
                for (rel, size, hash) in rows {
                    out.insert(rel, ManifestEntry { size, hash });
                }
            }
        }
        out
    }

    /// 保存某同步任务的 manifest(整体替换)。
    fn save_manifest(&self, job: &str, man: &BTreeMap<String, ManifestEntry>) {
        if let Some(store) = &self.store {
            let rows: Vec<(String, u64, Option<String>)> = man
                .iter()
                .map(|(rel, e)| (rel.clone(), e.size, e.hash.clone()))
                .collect();
            let _ = store.sync_manifest_replace(job, &rows);
        }
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
        resolutions: &BTreeMap<String, ConflictChoice>,
        cancel: &AtomicBool,
        progress: nebula_provider::ProgressFn<'_>,
    ) -> Result<SyncReport> {
        let account = &spec.account;
        let remote_prefix = &spec.remote_prefix;
        let local_root = Path::new(&spec.local_dir);
        let two_way = spec.mode == SyncMode::TwoWay;
        let (mut items, local_raw, remote_raw) = self.sync_diff_maps(spec).await?;

        // 应用冲突决议:把用户选了「保留本地 / 云端」的冲突项转成上传 / 下载;选跳过则保持冲突。
        for it in &mut items {
            if it.action == SyncAction::Conflict {
                match resolutions.get(&it.rel_path) {
                    Some(ConflictChoice::KeepLocal) => it.action = SyncAction::Upload,
                    Some(ConflictChoice::KeepRemote) => it.action = SyncAction::Download,
                    _ => {}
                }
            }
        }

        // 冲突与跳过都不算「工作」;只对真正要传 / 删的项计进度与执行。
        let actionable: Vec<&DiffItem> = items
            .iter()
            .filter(|i| !matches!(i.action, SyncAction::Skip | SyncAction::Conflict))
            .collect();
        let total = actionable.len() as u64;
        let mut report = SyncReport::default();
        let noop: nebula_provider::ProgressFn = &|_, _| {};
        // 双向:记录每个动作是否成功,收尾时据此更新 manifest。
        let mut ok_set: std::collections::HashSet<String> = std::collections::HashSet::new();

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
                // 冲突不动数据(交用户处理);跳过无事。
                SyncAction::Conflict | SyncAction::Skip => Ok(()),
            };
            if result.is_err() {
                report.failed += 1;
            } else {
                ok_set.insert(it.rel_path.clone());
            }
            progress((done + 1) as u64, total);
        }

        // 双向:根据本次结果重建 manifest(记录已达成一致的状态,冲突 / 失败保留旧记录)。
        if two_way && !cancel.load(Ordering::Relaxed) {
            let job = sync_job_key(spec);
            let mut man = self.load_manifest(&job);
            let etag_hash = |rel: &str| -> Option<String> {
                remote_raw
                    .get(rel)
                    .and_then(|(_, e)| e.as_deref())
                    .and_then(crate::integrity::etag_as_md5)
            };
            for it in &items {
                let rel = &it.rel_path;
                match it.action {
                    // 两侧已一致:记录约定签名(首次同步时把已相同的文件纳入 manifest)。
                    SyncAction::Skip => {
                        if it.local_size.is_some() && it.remote_size.is_some() {
                            man.insert(
                                rel.clone(),
                                ManifestEntry {
                                    size: it.remote_size.unwrap_or(0),
                                    hash: etag_hash(rel),
                                },
                            );
                        }
                    }
                    SyncAction::Upload if ok_set.contains(rel) => {
                        man.insert(
                            rel.clone(),
                            ManifestEntry {
                                size: it.local_size.unwrap_or(0),
                                hash: None, // 上传后不回取 ETag,按 size-only 记录
                            },
                        );
                    }
                    SyncAction::Download if ok_set.contains(rel) => {
                        man.insert(
                            rel.clone(),
                            ManifestEntry {
                                size: it.remote_size.unwrap_or(0),
                                hash: etag_hash(rel),
                            },
                        );
                    }
                    SyncAction::DeleteRemote | SyncAction::DeleteLocal if ok_set.contains(rel) => {
                        man.remove(rel);
                    }
                    // 冲突 / 失败:保留旧记录不动。
                    _ => {}
                }
            }
            self.save_manifest(&job, &man);
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

    fn me(size: u64, hash: Option<&str>) -> ManifestEntry {
        ManifestEntry {
            size,
            hash: hash.map(|s| s.to_string()),
        }
    }
    fn manifest(pairs: &[(&str, ManifestEntry)]) -> BTreeMap<String, ManifestEntry> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    #[test]
    fn two_way_propagates_one_sided_change() {
        // 上次两侧一致(MD5_A)。本地改成 MD5_B,云端没变 → 上传。
        let man = manifest(&[("f", me(1, Some(MD5_A)))]);
        let l = local(&[("f", lf(2, Some(MD5_B)))]); // size 变了 → Modified
        let r = remote(&[("f", rf(1, Some(MD5_A)))]);
        assert_eq!(
            action_of(&diff_two_way(&l, &r, &man), "f"),
            Some(&SyncAction::Upload)
        );
    }

    #[test]
    fn two_way_deletion_propagates() {
        // 上次有,本地删了,云端没动 → 删云端。
        let man = manifest(&[("f", me(1, Some(MD5_A)))]);
        let l = local(&[]);
        let r = remote(&[("f", rf(1, Some(MD5_A)))]);
        assert_eq!(
            action_of(&diff_two_way(&l, &r, &man), "f"),
            Some(&SyncAction::DeleteRemote)
        );
    }

    #[test]
    fn two_way_new_on_remote_downloads() {
        // manifest 里没有,只有云端有 → 下载(新增)。
        let man = manifest(&[]);
        let l = local(&[]);
        let r = remote(&[("f", rf(1, Some(MD5_A)))]);
        assert_eq!(
            action_of(&diff_two_way(&l, &r, &man), "f"),
            Some(&SyncAction::Download)
        );
    }

    #[test]
    fn two_way_both_changed_is_conflict() {
        // 上次一致,本地改成 B、云端改成不同内容(size 不同)→ 冲突。
        let man = manifest(&[("f", me(1, Some(MD5_A)))]);
        let l = local(&[("f", lf(2, Some(MD5_B)))]);
        let r = remote(&[("f", rf(3, Some("deadbeef00000000000000000000dead")))]);
        assert_eq!(
            action_of(&diff_two_way(&l, &r, &man), "f"),
            Some(&SyncAction::Conflict)
        );
    }

    #[test]
    fn two_way_delete_vs_modify_is_conflict() {
        // 本地删,云端改 → 冲突,不自动丢数据。
        let man = manifest(&[("f", me(1, Some(MD5_A)))]);
        let l = local(&[]);
        let r = remote(&[("f", rf(2, Some(MD5_B)))]);
        assert_eq!(
            action_of(&diff_two_way(&l, &r, &man), "f"),
            Some(&SyncAction::Conflict)
        );
    }

    #[test]
    fn two_way_unchanged_skips() {
        let man = manifest(&[("f", me(1, Some(MD5_A)))]);
        let l = local(&[("f", lf(1, Some(MD5_A)))]);
        let r = remote(&[("f", rf(1, Some(MD5_A)))]);
        assert_eq!(
            action_of(&diff_two_way(&l, &r, &man), "f"),
            Some(&SyncAction::Skip)
        );
    }

    #[test]
    fn glob_bare_name_matches_any_segment() {
        assert!(is_excluded(".DS_Store", &[".DS_Store".into()]));
        assert!(is_excluded("sub/dir/.DS_Store", &[".DS_Store".into()]));
        assert!(is_excluded("a/b.tmp", &["*.tmp".into()]));
        assert!(!is_excluded("a/b.txt", &["*.tmp".into()]));
    }

    #[test]
    fn glob_double_star_crosses_dirs() {
        assert!(is_excluded(
            "node_modules/x/y.js",
            &["node_modules/**".into()]
        ));
        assert!(is_excluded(
            "a/node_modules/z",
            &["**/node_modules/**".into()]
        ));
        assert!(!is_excluded("src/app.js", &["node_modules/**".into()]));
    }

    #[test]
    fn glob_single_star_does_not_cross_slash() {
        assert!(is_excluded("build/out.o", &["build/*.o".into()]));
        assert!(!is_excluded("build/sub/out.o", &["build/*.o".into()]));
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
