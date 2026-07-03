import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import { faCloud, faCloudArrowUp, faPlus } from "@fortawesome/free-solid-svg-icons";
import type { DownloadProgress, Entry, Transfer, UploadProgress } from "./types";
import * as api from "./api";
import { baseName, joinRemote, parentPath } from "./util";
import { Sidebar } from "./components/Sidebar";
import { AccountForm } from "./components/AccountForm";
import { Breadcrumb } from "./components/Breadcrumb";
import { Toolbar } from "./components/Toolbar";
import { FileList } from "./components/FileList";
import { ConfirmDialog } from "./components/ConfirmDialog";
import { PromptDialog } from "./components/PromptDialog";

export default function App() {
  const [accounts, setAccounts] = useState<string[]>([]);
  const [current, setCurrent] = useState<string | null>(null);
  const [path, setPath] = useState("");
  const [entries, setEntries] = useState<Entry[]>([]);
  const [loading, setLoading] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [showForm, setShowForm] = useState(false);
  const [pendingDelete, setPendingDelete] = useState<Entry | null>(null);
  const [renameTarget, setRenameTarget] = useState<Entry | null>(null);
  const [showNewFolder, setShowNewFolder] = useState(false);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [pendingBatchDelete, setPendingBatchDelete] = useState(false);
  const [dragOver, setDragOver] = useState(false);
  const [transfer, setTransfer] = useState<Transfer | null>(null);
  const [filter, setFilter] = useState("");
  const [sortKey, setSortKey] = useState<"name" | "size" | "modified">("name");
  const [sortDir, setSortDir] = useState<"asc" | "desc">("asc");

  // 切换账号 / 目录时清空过滤词与选择。
  useEffect(() => {
    setFilter("");
    setSelected(new Set());
  }, [current, path]);

  const visibleEntries = useMemo(() => {
    const f = filter.trim().toLowerCase();
    const filtered = f
      ? entries.filter((e) => e.name.toLowerCase().includes(f))
      : entries;
    const dir = sortDir === "asc" ? 1 : -1;
    return [...filtered].sort((a, b) => {
      // 目录始终排在文件前面。
      if (a.kind !== b.kind) return a.kind === "directory" ? -1 : 1;
      let cmp = 0;
      if (sortKey === "name") cmp = a.name.localeCompare(b.name);
      else if (sortKey === "size") cmp = a.size - b.size;
      else cmp = (a.last_modified ?? "").localeCompare(b.last_modified ?? "");
      return cmp * dir;
    });
  }, [entries, filter, sortKey, sortDir]);

  const toggleSort = (key: "name" | "size" | "modified") => {
    if (key === sortKey) {
      setSortDir((d) => (d === "asc" ? "desc" : "asc"));
    } else {
      setSortKey(key);
      setSortDir("asc");
    }
  };

  const visibleFiles = useMemo(
    () => visibleEntries.filter((e) => e.kind === "file"),
    [visibleEntries],
  );
  const allSelected =
    visibleFiles.length > 0 && visibleFiles.every((f) => selected.has(f.path));

  const toggleSelect = (p: string) =>
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(p)) next.delete(p);
      else next.add(p);
      return next;
    });

  const toggleSelectAll = () =>
    setSelected((prev) => {
      const next = new Set(prev);
      if (allSelected) visibleFiles.forEach((f) => next.delete(f.path));
      else visibleFiles.forEach((f) => next.add(f.path));
      return next;
    });

  const clearSelection = () => setSelected(new Set());

  useEffect(() => {
    const unUpload = listen<UploadProgress>("upload-progress", (e) => {
      setTransfer({
        label: "上传",
        path: e.payload.path,
        done: e.payload.uploaded,
        total: e.payload.total,
      });
    });
    const unDownload = listen<DownloadProgress>("download-progress", (e) => {
      setTransfer({
        label: "下载",
        path: e.payload.path,
        done: e.payload.downloaded,
        total: e.payload.total,
      });
    });
    return () => {
      unUpload.then((off) => off());
      unDownload.then((off) => off());
    };
  }, []);

  const refreshAccounts = useCallback(async () => {
    const list = await api.listAccounts();
    setAccounts(list);
    setCurrent((cur) => cur ?? list[0] ?? null);
  }, []);

  useEffect(() => {
    refreshAccounts();
  }, [refreshAccounts]);

  const load = useCallback(async () => {
    if (!current) {
      setEntries([]);
      return;
    }
    setLoading(true);
    setError(null);
    try {
      setEntries(await api.browse(current, path));
    } catch (e) {
      setError(String(e));
      setEntries([]);
    } finally {
      setLoading(false);
    }
  }, [current, path]);

  useEffect(() => {
    load();
  }, [load]);

  // 拖拽上传:用 ref 持有最新处理逻辑,拖放监听只注册一次。
  const onDropRef = useRef<(paths: string[]) => void>(() => {});
  onDropRef.current = async (paths: string[]) => {
    if (!current || !path || paths.length === 0) return;
    setBusy(true);
    setError(null);
    try {
      for (const local of paths) {
        await api.uploadFile(current, joinRemote(path, baseName(local)), local);
      }
      await load();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
      setTransfer(null);
    }
  };

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    getCurrentWebview()
      .onDragDropEvent((event) => {
        const t = event.payload.type;
        if (t === "enter" || t === "over") setDragOver(true);
        else if (t === "leave") setDragOver(false);
        else if (t === "drop") {
          setDragOver(false);
          onDropRef.current(event.payload.paths);
        }
      })
      .then((f) => {
        unlisten = f;
      });
    return () => unlisten?.();
  }, []);

  const selectAccount = (id: string) => {
    setCurrent(id);
    setPath("");
  };

  const addAccount = async (
    id: string,
    ak: string,
    sk: string,
    endpoint: string,
  ) => {
    setShowForm(false);
    setError(null);
    try {
      await api.addAliyunAccount(id, ak, sk, endpoint);
      await refreshAccounts();
      setCurrent(id);
      setPath("");
    } catch (e) {
      setError(String(e));
    }
  };

  const removeAccount = async (id: string) => {
    await api.removeAccount(id);
    if (current === id) {
      setCurrent(null);
      setPath("");
      setEntries([]);
    }
    await refreshAccounts();
  };

  const doBatchDelete = async () => {
    setPendingBatchDelete(false);
    if (!current || selected.size === 0) return;
    setBusy(true);
    setError(null);
    try {
      for (const p of selected) await api.deletePath(current, p);
      clearSelection();
      await load();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const batchDownload = async () => {
    if (!current || selected.size === 0) return;
    const dir = await open({ directory: true, title: "选择下载到的文件夹" });
    if (typeof dir !== "string") return;
    setBusy(true);
    setError(null);
    try {
      for (const p of selected) {
        await api.downloadFile(current, p, `${dir}/${baseName(p)}`);
      }
      clearSelection();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
      setTransfer(null);
    }
  };

  const upload = async () => {
    if (!current || !path) return;
    const selected = await open({ multiple: false, title: "选择要上传的文件" });
    if (typeof selected !== "string") return;
    const remote = joinRemote(path, baseName(selected));
    setBusy(true);
    setError(null);
    try {
      await api.uploadFile(current, remote, selected);
      await load();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
      setTransfer(null);
    }
  };

  const download = async (entry: Entry) => {
    if (!current) return;
    const target = await save({ defaultPath: entry.name });
    if (typeof target !== "string") return;
    setBusy(true);
    setError(null);
    try {
      await api.downloadFile(current, entry.path, target);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
      setTransfer(null);
    }
  };

  const doDelete = async () => {
    const entry = pendingDelete;
    setPendingDelete(null);
    if (!current || !entry) return;
    setBusy(true);
    setError(null);
    try {
      await api.deletePath(current, entry.path);
      await load();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const doRename = async (newName: string) => {
    const entry = renameTarget;
    setRenameTarget(null);
    if (!current || !entry || newName === entry.name) return;
    const to = joinRemote(parentPath(entry.path), newName);
    setBusy(true);
    setError(null);
    try {
      await api.rename(current, entry.path, to);
      await load();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const createFolder = async (name: string) => {
    setShowNewFolder(false);
    if (!current || !path) return;
    const folderPath = joinRemote(path, name).replace(/\/+$/, "") + "/";
    setBusy(true);
    setError(null);
    try {
      await api.createFolder(current, folderPath);
      await load();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="layout">
      <Sidebar
        accounts={accounts}
        current={current}
        onSelect={selectAccount}
        onAdd={() => setShowForm(true)}
        onRemove={removeAccount}
      />

      <main className="main">
        {dragOver && current && path !== "" && (
          <div className="drop-overlay">
            <FontAwesomeIcon icon={faCloudArrowUp} className="drop-overlay__icon" />
            <span>松开以上传到当前目录</span>
          </div>
        )}
        {current ? (
          <>
            <div className="main__header">
              <Breadcrumb path={path} onNavigate={setPath} />
              <Toolbar
                canGoUp={path !== ""}
                canUpload={path !== ""}
                busy={busy}
                filter={filter}
                onFilter={setFilter}
                onUp={() => setPath(parentPath(path))}
                onRefresh={load}
                onUpload={upload}
                onNewFolder={() => setShowNewFolder(true)}
              />
            </div>

            {error && <div className="error-banner">{error}</div>}

            {selected.size > 0 && (
              <div className="batch-bar">
                <span className="batch-bar__count">已选 {selected.size} 项</span>
                <div className="batch-bar__spacer" />
                <button className="btn" onClick={batchDownload}>
                  批量下载
                </button>
                <button
                  className="btn btn--danger"
                  onClick={() => setPendingBatchDelete(true)}
                >
                  批量删除
                </button>
                <button className="btn" onClick={clearSelection}>
                  取消选择
                </button>
              </div>
            )}

            <FileList
              entries={visibleEntries}
              loading={loading}
              sortKey={sortKey}
              sortDir={sortDir}
              selected={selected}
              allSelected={allSelected}
              onSort={toggleSort}
              onToggleSelect={toggleSelect}
              onToggleSelectAll={toggleSelectAll}
              onOpenDir={(e) => setPath(e.path.endsWith("/") ? e.path : e.path + "/")}
              onDownload={download}
              onRename={setRenameTarget}
              onDelete={setPendingDelete}
            />

            {transfer && transfer.total > 0 && (
              <div className="upload-progress">
                <div className="upload-progress__info">
                  <span className="upload-progress__name">
                    {transfer.label} {baseName(transfer.path)}
                  </span>
                  <span className="upload-progress__pct">
                    {Math.round((transfer.done / transfer.total) * 100)}%
                  </span>
                </div>
                <div className="upload-progress__track">
                  <div
                    className="upload-progress__fill"
                    style={{
                      width: `${(transfer.done / transfer.total) * 100}%`,
                    }}
                  />
                </div>
              </div>
            )}
          </>
        ) : (
          <div className="empty-state">
            <FontAwesomeIcon icon={faCloud} className="empty-state__icon" />
            <h2>欢迎使用 Nebula</h2>
            <p>添加一个云账号开始管理你的对象存储</p>
            <button className="btn btn--primary" onClick={() => setShowForm(true)}>
              <FontAwesomeIcon icon={faPlus} /> 添加账号
            </button>
          </div>
        )}
      </main>

      {showForm && (
        <AccountForm onSubmit={addAccount} onClose={() => setShowForm(false)} />
      )}

      {pendingDelete && (
        <ConfirmDialog
          title="删除确认"
          message={`确定删除 ${pendingDelete.name}?此操作不可恢复。`}
          danger
          confirmLabel="删除"
          onConfirm={doDelete}
          onCancel={() => setPendingDelete(null)}
        />
      )}

      {showNewFolder && (
        <PromptDialog
          title="新建文件夹"
          placeholder="文件夹名称"
          submitLabel="创建"
          onSubmit={createFolder}
          onCancel={() => setShowNewFolder(false)}
        />
      )}

      {renameTarget && (
        <PromptDialog
          title="重命名"
          placeholder="新名称"
          initial={renameTarget.name}
          submitLabel="重命名"
          onSubmit={doRename}
          onCancel={() => setRenameTarget(null)}
        />
      )}

      {pendingBatchDelete && (
        <ConfirmDialog
          title="批量删除"
          message={`确定删除选中的 ${selected.size} 项?此操作不可恢复。`}
          danger
          confirmLabel="删除"
          onConfirm={doBatchDelete}
          onCancel={() => setPendingBatchDelete(false)}
        />
      )}
    </div>
  );
}
