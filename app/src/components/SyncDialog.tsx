import { useEffect, useRef, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { listen } from "@tauri-apps/api/event";
import * as api from "../api";
import { useI18n } from "../i18n";
import { formatBytes } from "../util";
import { Select } from "./Select";
import { Checkbox } from "./Checkbox";
import type {
  AccountInfo,
  ConflictChoice,
  SyncDiffItem,
  SyncDiffSummary,
  SyncJob,
  SyncMode,
  SyncReport,
  SyncSpec,
} from "../types";

interface Props {
  accounts: AccountInfo[];
  defaultAccount: string | null;
  defaultPrefix: string;
  onClose: () => void;
}

interface SyncProgressEvent {
  id: string;
  done: number;
  total: number;
}

const ACTION_LABEL: Record<string, string> = {
  upload: "↑",
  download: "↓",
  delete_remote: "✕云",
  delete_local: "✕本地",
  conflict: "⚠",
  skip: "=",
};

/** 把 Unix 秒格式化成简短的相对时间。`tr` 为 i18n 翻译函数。 */
function relTime(tr: (k: string, v?: Record<string, string>) => string, epochSecs: number): string {
  const s = Math.max(0, Math.floor(Date.now() / 1000) - epochSecs);
  if (s < 60) return tr("刚刚");
  if (s < 3600) return tr("{n} 分钟前", { n: String(Math.floor(s / 60)) });
  if (s < 86400) return tr("{n} 小时前", { n: String(Math.floor(s / 3600)) });
  return tr("{n} 天前", { n: String(Math.floor(s / 86400)) });
}

/**
 * 备份 / 同步对话框:本地目录 ⇄ 云端前缀。先预览 diff(不改数据),确认后执行。
 * 模式:备份(本地→云)/ 还原(云→本地)/ 双向。可删除目标侧多余文件。
 */
export function SyncDialog({ accounts, defaultAccount, defaultPrefix, onClose }: Props) {
  const { t } = useI18n();
  const [account, setAccount] = useState(defaultAccount ?? accounts[0]?.id ?? "");
  const [localDir, setLocalDir] = useState("");
  const [remotePrefix, setRemotePrefix] = useState(defaultPrefix);
  const [mode, setMode] = useState<SyncMode>("mirror_up");
  const [deleteExtra, setDeleteExtra] = useState(false);
  const [excludes, setExcludes] = useState(".DS_Store\nnode_modules/**\n*.tmp");
  // 已保存任务 + 当前任务名 / 定时间隔(分钟,0=手动)。
  const [jobs, setJobs] = useState<SyncJob[]>([]);
  const [jobName, setJobName] = useState("");
  const [intervalMins, setIntervalMins] = useState(0);
  const [jobId, setJobId] = useState<string | null>(null);
  // 双向冲突的逐文件决议(rel_path → 保留哪边)。
  const [resolutions, setResolutions] = useState<Record<string, ConflictChoice>>({});

  const [busy, setBusy] = useState(false);
  const [items, setItems] = useState<SyncDiffItem[] | null>(null);
  const [summary, setSummary] = useState<SyncDiffSummary | null>(null);
  const [progress, setProgress] = useState<{ done: number; total: number } | null>(null);
  const [report, setReport] = useState<SyncReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const runId = useRef<string>("");
  const unlisten = useRef<(() => void) | null>(null);

  useEffect(() => () => unlisten.current?.(), []);

  const loadJobs = () => {
    api.syncJobs().then(setJobs).catch(() => {});
  };
  useEffect(loadJobs, []);

  const spec = (): SyncSpec => ({
    account,
    local_dir: localDir,
    remote_prefix: remotePrefix,
    mode,
    delete_extra: deleteExtra,
    excludes: excludes
      .split("\n")
      .map((s) => s.trim())
      .filter(Boolean),
  });

  const pickDir = async () => {
    const picked = await openDialog({ directory: true, multiple: false });
    if (typeof picked === "string") setLocalDir(picked);
  };

  const preview = async () => {
    if (!account || !localDir) {
      setError(t("请选择账号与本地目录"));
      return;
    }
    setError(null);
    setBusy(true);
    setReport(null);
    try {
      const [its, sum] = await api.syncPreview(spec());
      setItems(its);
      setSummary(sum);
    } catch (e) {
      setError(String(e));
    }
    setBusy(false);
  };

  const run = async () => {
    setBusy(true);
    setError(null);
    setReport(null);
    setProgress({ done: 0, total: 0 });
    runId.current = `sync-${Date.now()}`;
    unlisten.current = await listen<SyncProgressEvent>("sync-progress", (e) => {
      if (e.payload.id === runId.current)
        setProgress({ done: e.payload.done, total: e.payload.total });
    });
    try {
      const rep = await api.syncRun(runId.current, spec(), resolutions);
      setReport(rep);
      // 执行后重新预览,反映最新状态。
      const [its, sum] = await api.syncPreview(spec());
      setItems(its);
      setSummary(sum);
    } catch (e) {
      setError(String(e));
    }
    unlisten.current?.();
    unlisten.current = null;
    setBusy(false);
    setProgress(null);
  };

  const cancel = () => {
    if (runId.current) void api.cancelTransfer(runId.current);
  };

  // 保存当前设置为一个命名任务(可定时)。
  const saveJob = async () => {
    if (!jobName.trim() || !account || !localDir) {
      setError(t("请填任务名、账号与本地目录"));
      return;
    }
    const existing = jobs.find((j) => j.id === jobId);
    const job: SyncJob = {
      id: jobId ?? `job-${Date.now()}`,
      name: jobName.trim(),
      spec: spec(),
      interval_mins: intervalMins,
      last_run: existing?.last_run ?? 0,
      last_result: existing?.last_result ?? "",
    };
    setError(null);
    try {
      await api.saveSyncJob(job);
      setJobId(job.id);
      loadJobs();
    } catch (e) {
      setError(String(e));
    }
  };
  const loadJob = (j: SyncJob) => {
    setJobId(j.id);
    setJobName(j.name);
    setAccount(j.spec.account);
    setLocalDir(j.spec.local_dir);
    setRemotePrefix(j.spec.remote_prefix);
    setMode(j.spec.mode);
    setDeleteExtra(j.spec.delete_extra);
    setExcludes(j.spec.excludes.join("\n"));
    setIntervalMins(j.interval_mins);
    setItems(null);
    setSummary(null);
    setReport(null);
  };
  const removeJob = async (id: string) => {
    await api.deleteSyncJob(id).catch(() => {});
    if (jobId === id) setJobId(null);
    loadJobs();
  };

  // 已解决(选了保留哪边)的冲突数——这些会变成实际要传的工作。
  const resolvedCount = Object.values(resolutions).filter(
    (c) => c !== "skip",
  ).length;
  const hasWork =
    !!summary &&
    (summary.upload + summary.download + summary.delete_remote + summary.delete_local > 0 ||
      resolvedCount > 0);

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal modal--wide" onClick={(e) => e.stopPropagation()}>
        <div className="modal__header">
          <h3>{t("备份 / 同步")}</h3>
        </div>
        <div className="modal__body">
          <label className="field">
            <span>{t("账号")}</span>
            <Select
              value={account}
              options={accounts.map((a) => ({ value: a.id, label: a.id }))}
              onChange={setAccount}
            />
          </label>
          <label className="field">
            <span>{t("本地目录")}</span>
            <div className="sync__dir">
              <input value={localDir} readOnly placeholder={t("选择本地文件夹…")} />
              <button className="btn" onClick={pickDir}>
                {t("选择…")}
              </button>
            </div>
          </label>
          <label className="field">
            <span>{t("云端前缀")}</span>
            <input
              value={remotePrefix}
              onChange={(e) => setRemotePrefix(e.target.value)}
              placeholder="bucket/backup/"
            />
          </label>
          <label className="field">
            <span>{t("模式")}</span>
            <Select
              value={mode}
              options={[
                { value: "mirror_up", label: t("备份(本地 → 云端)") },
                { value: "mirror_down", label: t("还原(云端 → 本地)") },
                { value: "two_way", label: t("双向同步") },
              ]}
              onChange={(v) => setMode(v as SyncMode)}
            />
          </label>
          {mode !== "two_way" && (
            <label className="field field--check">
              <Checkbox checked={deleteExtra} onChange={() => setDeleteExtra((v) => !v)} />
              <span>
                {mode === "mirror_up"
                  ? t("删除云端多余对象(镜像)")
                  : t("删除本地多余文件(镜像)")}
              </span>
            </label>
          )}

          <label className="field">
            <span>{t("排除规则")}</span>
            <textarea
              className="sync__excludes"
              rows={3}
              value={excludes}
              onChange={(e) => setExcludes(e.target.value)}
              placeholder={t("每行一条 glob,如 .DS_Store 或 node_modules/**")}
            />
          </label>

          <label className="field">
            <span>{t("任务名(保存用)")}</span>
            <div className="sync__dir">
              <input
                value={jobName}
                onChange={(e) => setJobName(e.target.value)}
                placeholder={t("如:照片备份")}
              />
              <Select
                value={String(intervalMins)}
                options={[
                  { value: "0", label: t("手动") },
                  { value: "15", label: t("每 15 分钟") },
                  { value: "60", label: t("每小时") },
                  { value: "360", label: t("每 6 小时") },
                  { value: "1440", label: t("每天") },
                ]}
                onChange={(v) => setIntervalMins(Number(v))}
              />
              <button className="btn" onClick={saveJob}>
                {jobId ? t("更新任务") : t("保存任务")}
              </button>
            </div>
          </label>

          {jobs.length > 0 && (
            <div className="sync__jobs">
              {jobs.map((j) => (
                <div className="sync__job" key={j.id}>
                  <button className="sync__job-load" onClick={() => loadJob(j)}>
                    <span className="sync__job-name">{j.name}</span>
                    <span className="sync__job-meta">
                      {j.interval_mins > 0
                        ? t("每 {n} 分钟", { n: String(j.interval_mins) })
                        : t("手动")}
                      {j.last_run > 0 &&
                        ` · ${t("上次")} ${relTime(t, j.last_run)}${
                          j.last_result ? ` · ${j.last_result}` : ""
                        }`}
                    </span>
                  </button>
                  <button
                    className="sync__job-del"
                    title={t("删除")}
                    onClick={() => removeJob(j.id)}
                  >
                    ✕
                  </button>
                </div>
              ))}
            </div>
          )}

          {summary && (
            <div className="sync__summary">
              <span className="sync__stat sync__stat--up">↑ {summary.upload}</span>
              <span className="sync__stat sync__stat--down">↓ {summary.download}</span>
              <span className="sync__stat sync__stat--del">
                ✕ {summary.delete_remote + summary.delete_local}
              </span>
              {summary.conflict > 0 && (
                <span className="sync__stat sync__stat--conflict">
                  ⚠ {summary.conflict}
                </span>
              )}
              <span className="sync__stat">= {summary.skip}</span>
              <span className="sync__stat sync__stat--bytes">
                {formatBytes(summary.transfer_bytes)}
              </span>
            </div>
          )}

          {items && items.length > 0 && (
            <div className="sync__list">
              {items
                .filter((it) => it.action !== "skip")
                .slice(0, 500)
                .map((it) => (
                  <div className="sync__row" key={it.rel_path}>
                    <span className={`sync__act sync__act--${it.action}`}>
                      {ACTION_LABEL[it.action]}
                    </span>
                    <span className="sync__path" title={it.rel_path}>
                      {it.rel_path}
                    </span>
                    {it.action === "conflict" && (
                      <span className="sync__resolve">
                        {(
                          [
                            ["keep_local", t("留本地")],
                            ["keep_remote", t("留云端")],
                            ["skip", t("跳过")],
                          ] as [ConflictChoice, string][]
                        ).map(([c, label]) => (
                          <button
                            key={c}
                            className={`sync__choice ${resolutions[it.rel_path] === c ? "sync__choice--on" : ""}`}
                            onClick={() =>
                              setResolutions((r) => ({ ...r, [it.rel_path]: c }))
                            }
                          >
                            {label}
                          </button>
                        ))}
                      </span>
                    )}
                  </div>
                ))}
            </div>
          )}
          {items && !hasWork && !report && (
            <div className="sync__empty">{t("两侧已一致,无需同步")}</div>
          )}

          {progress && (
            <div className="sync__progress">
              <div className="sync__bar">
                <div
                  className="sync__fill"
                  style={{
                    width: `${progress.total > 0 ? (progress.done / progress.total) * 100 : 0}%`,
                  }}
                />
              </div>
              <span>
                {progress.done} / {progress.total}
              </span>
            </div>
          )}
          {report && (
            <div className="sync__report">
              {t("完成:上传 {u} · 下载 {d} · 删除 {x} · 失败 {f} · 共 {b}", {
                u: String(report.uploaded),
                d: String(report.downloaded),
                x: String(report.deleted_remote + report.deleted_local),
                f: String(report.failed),
                b: formatBytes(report.bytes),
              })}
            </div>
          )}
          {error && <div className="sync__error">{error}</div>}
        </div>
        <div className="modal__footer">
          <button className="btn" onClick={onClose}>
            {t("关闭")}
          </button>
          {busy && progress ? (
            <button className="btn btn--danger" onClick={cancel}>
              {t("取消同步")}
            </button>
          ) : (
            <>
              <button className="btn" disabled={busy} onClick={preview}>
                {busy ? t("处理中…") : t("预览")}
              </button>
              <button
                className="btn btn--primary"
                disabled={busy || !hasWork}
                onClick={run}
              >
                {t("开始同步")}
              </button>
            </>
          )}
        </div>
      </div>
    </div>
  );
}
