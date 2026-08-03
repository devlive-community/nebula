//! 同步任务的后台调度器:App 里第一个和应用生命周期绑定的长期后台任务。
//!
//! 按 `interval_mins`/`last_run` 定期检查到期任务(和原来前端 `setInterval` 的判断逻辑
//! 完全一样),同时给符合条件的任务(见 [`app_core::sync_watch_eligible`]:
//! `MirrorUp`/`TwoWay`、`interval_mins > 0`、本地目录存在)额外挂一个防抖的本地文件系统
//! 监听,检测到变化并稳定后立刻触发,不必等到下一个整间隔。`MirrorDown` 任务(云→本地)
//! 本地监听帮不上忙,继续完全靠按间隔轮询——对象存储没有跨七家云统一的"云端变更推送"
//! API,这不是本模块能解决的。
//!
//! 进程退出时由操作系统回收(watcher、内部状态都在这个任务里),不做显式的优雅关闭。

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use app_core::{sync_watch_eligible, App, SyncJob};
use notify::{RecursiveMode, Watcher};
use notify_debouncer_full::{new_debouncer, DebounceEventResult, Debouncer, FileIdMap};
use tauri::{AppHandle, Emitter};
use tokio::sync::mpsc;

use crate::{SyncJobFinished, SyncProgress};

/// 到期检查的轮询间隔:比原来前端的 60 秒细,但足够便宜(只读一次 SQLite `sync_jobs` 表)。
const TICK: Duration = Duration::from_secs(5);
/// 本地文件系统事件的防抖窗口:一批改动落定后再触发,避免连续写入误判成多次变化。
const DEBOUNCE: Duration = Duration::from_secs(2);

type JobWatcher = Debouncer<notify::RecommendedWatcher, FileIdMap>;

/// 启动同步任务的后台调度器(只应调用一次,通常在 Tauri `.setup()` 里)。
pub fn spawn_sync_scheduler(app_handle: AppHandle, core: App) {
    tauri::async_runtime::spawn(async move {
        let running: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));
        let mut watchers: HashMap<String, (PathBuf, JobWatcher)> = HashMap::new();
        let (tx, mut rx) = mpsc::unbounded_channel::<String>();

        loop {
            reconcile_watchers(&core, &mut watchers, &tx);

            tokio::select! {
                _ = tokio::time::sleep(TICK) => {
                    trigger_due_jobs(&core, &app_handle, &running).await;
                }
                Some(job_id) = rx.recv() => {
                    trigger_job_by_id(&core, &app_handle, &running, &job_id).await;
                }
            }
        }
    });
}

/// 让当前维护的 watcher 集合和"现在符合条件的任务"保持一致:新符合条件的补建;
/// 任务被删、间隔改成 0、模式改成 `MirrorDown`、或本地目录路径变了的移除 / 重建。
fn reconcile_watchers(
    core: &App,
    watchers: &mut HashMap<String, (PathBuf, JobWatcher)>,
    tx: &mpsc::UnboundedSender<String>,
) {
    let jobs = core.sync_jobs();
    let eligible: HashMap<String, PathBuf> = jobs
        .iter()
        .filter(|j| sync_watch_eligible(j))
        .map(|j| (j.id.clone(), PathBuf::from(&j.spec.local_dir)))
        .collect();

    // 不再符合条件、或本地目录路径变了的:移除(Drop 会停止底层监听),下面按新路径重建。
    watchers.retain(|id, (path, _)| eligible.get(id).is_some_and(|p| p == path));

    for (id, path) in eligible {
        if watchers.contains_key(&id) {
            continue;
        }
        let job_id = id.clone();
        let tx = tx.clone();
        let debouncer = new_debouncer(DEBOUNCE, None, move |result: DebounceEventResult| {
            if result.is_ok() {
                let _ = tx.send(job_id.clone());
            }
        });
        let Ok(mut debouncer) = debouncer else {
            continue;
        };
        if debouncer
            .watcher()
            .watch(&path, RecursiveMode::Recursive)
            .is_ok()
        {
            watchers.insert(id, (path, debouncer));
        }
    }
}

/// 检查全部任务里到期(`now - last_run >= interval_mins * 60`)且未在跑的,逐个触发。
/// 覆盖 `MirrorDown` 任务的全部调度,也覆盖 `MirrorUp`/`TwoWay` 任务"这段时间没检测到
/// 本地变化"时的兜底——和原来前端 `App.tsx` 里 `tick()` 的判断逻辑完全一样。
async fn trigger_due_jobs(
    core: &App,
    app_handle: &AppHandle,
    running: &Arc<Mutex<HashSet<String>>>,
) {
    let now = now_unix();
    for job in core.sync_jobs() {
        if job.interval_mins == 0 {
            continue;
        }
        if now - job.last_run < job.interval_mins as i64 * 60 {
            continue;
        }
        run_job_if_not_running(core, app_handle, running, job).await;
    }
}

/// 本地文件系统监听触发:按 job id 查出最新的任务定义并执行。
async fn trigger_job_by_id(
    core: &App,
    app_handle: &AppHandle,
    running: &Arc<Mutex<HashSet<String>>>,
    job_id: &str,
) {
    if let Some(job) = core.sync_jobs().into_iter().find(|j| j.id == job_id) {
        run_job_if_not_running(core, app_handle, running, job).await;
    }
}

/// 若该任务当前不在"正在跑"集合里,标记后执行一次同步,完成后写回 `last_run`/
/// `last_result` 并移出集合。冲突项(仅双向同步可能出现)传空决议表,保守跳过不处理——
/// 自动运行不替用户猜"保留哪一侧",用户下次手动打开同步面板走预览仍能看到并解决。
async fn run_job_if_not_running(
    core: &App,
    app_handle: &AppHandle,
    running: &Arc<Mutex<HashSet<String>>>,
    mut job: SyncJob,
) {
    {
        let mut set = running.lock().unwrap();
        if set.contains(&job.id) {
            return;
        }
        set.insert(job.id.clone());
    }

    let cancel = AtomicBool::new(false);
    let handle = app_handle.clone();
    let event_id = job.id.clone();
    let result = core
        .sync_run(
            &job.spec,
            &std::collections::BTreeMap::new(),
            &cancel,
            &move |done, total| {
                let _ = handle.emit(
                    "sync-progress",
                    SyncProgress {
                        id: event_id.clone(),
                        done,
                        total,
                    },
                );
            },
        )
        .await;

    job.last_run = now_unix();
    match &result {
        // 摘要格式和原来前端 `App.tsx` 里 tick() 拼的完全一样(↑上传 ↓下载 ✕删除 ⚠失败),
        // 只有真的做了事(有上传/下载/删除)才弹通知——和原来"全 0 就不打扰用户"的判断一致。
        Ok(report) => {
            let had_work =
                report.uploaded + report.downloaded + report.deleted_remote + report.deleted_local
                    > 0;
            let text = format!(
                "↑{} ↓{} ✕{}{}",
                report.uploaded,
                report.downloaded,
                report.deleted_remote + report.deleted_local,
                if report.failed > 0 {
                    format!(" ⚠{}", report.failed)
                } else {
                    String::new()
                }
            );
            job.last_result = text.clone();
            if had_work {
                let _ = app_handle.emit(
                    "sync-job-finished",
                    SyncJobFinished {
                        name: job.name.clone(),
                        result: text,
                        tone: if report.failed > 0 { "warn" } else { "ok" }.to_string(),
                    },
                );
            }
        }
        Err(e) => {
            job.last_result = "失败".to_string();
            let _ = app_handle.emit(
                "sync-job-finished",
                SyncJobFinished {
                    name: job.name.clone(),
                    result: e.to_string(),
                    tone: "err".to_string(),
                },
            );
        }
    }
    let _ = core.save_sync_job(&job);

    running.lock().unwrap().remove(&job.id);
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
