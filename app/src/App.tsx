import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import { faCloud, faCloudArrowUp, faPlus } from "@fortawesome/free-solid-svg-icons";
import type {
  AccountInfo,
  DownloadProgress,
  Entry,
  Settings,
  TransferItem,
  UploadProgress,
} from "./types";
import * as api from "./api";
import {
  baseName,
  formatBytes,
  joinRemote,
  parentPath,
  previewKind,
  runPool,
} from "./util";
import { Sidebar } from "./components/Sidebar";
import { AccountForm } from "./components/AccountForm";
import { Breadcrumb } from "./components/Breadcrumb";
import { Toolbar } from "./components/Toolbar";
import { FileList } from "./components/FileList";
import { FileGrid } from "./components/FileGrid";
import { ConfirmDialog } from "./components/ConfirmDialog";
import { PromptDialog } from "./components/PromptDialog";
import { MoveCopyDialog } from "./components/MoveCopyDialog";
import { ShareDialog } from "./components/ShareDialog";
import { FileDetails } from "./components/FileDetails";
import { TransferPanel } from "./components/TransferPanel";
import { SettingsDialog } from "./components/SettingsDialog";
import { ContextMenu, type MenuItem } from "./components/ContextMenu";
import { PreviewModal } from "./components/PreviewModal";

export default function App() {
  const [accounts, setAccounts] = useState<string[]>([]);
  const [current, setCurrent] = useState<string | null>(null);
  const [path, setPath] = useState("");
  const [entries, setEntries] = useState<Entry[]>([]);
  const [loading, setLoading] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [showForm, setShowForm] = useState(false);
  const [editInfo, setEditInfo] = useState<AccountInfo | null>(null);
  const [pendingDelete, setPendingDelete] = useState<Entry | null>(null);
  const [renameTarget, setRenameTarget] = useState<Entry | null>(null);
  const [moveCopyTarget, setMoveCopyTarget] = useState<Entry | null>(null);
  const [shareUrl, setShareUrl] = useState<string | null>(null);
  const [detailsEntry, setDetailsEntry] = useState<Entry | null>(null);
  const [showNewFolder, setShowNewFolder] = useState(false);
  const [settings, setSettings] = useState<Settings>({
    share_expiry_secs: 3600,
    concurrency: 3,
  });
  const [showSettings, setShowSettings] = useState(false);
  const [menu, setMenu] = useState<{ x: number; y: number; entry: Entry } | null>(
    null,
  );
  const [preview, setPreview] = useState<{
    url: string;
    name: string;
    kind: "image" | "video";
  } | null>(null);
  const [view, setView] = useState<"list" | "grid">(
    () => (localStorage.getItem("nebula-view") as "list" | "grid") || "list",
  );
  const [thumbs, setThumbs] = useState<Record<string, string>>({});

  useEffect(() => {
    localStorage.setItem("nebula-view", view);
  }, [view]);

  const openDir = (entry: Entry) =>
    setPath(entry.path.endsWith("/") ? entry.path : entry.path + "/");

  // 预览图片 / 视频:用预签名链接加载;非可预览类型退回详情。
  const openPreview = async (entry: Entry) => {
    const kind = previewKind(entry.name);
    if (!current || !kind) {
      setDetailsEntry(entry);
      return;
    }
    setError(null);
    try {
      const url = await api.presign(current, entry.path, 600);
      setPreview({ url, name: entry.name, kind });
    } catch (e) {
      setError(String(e));
    }
  };

  useEffect(() => {
    api.getSettings().then(setSettings).catch(() => {});
  }, []);

  const saveSettings = async (next: Settings) => {
    setShowSettings(false);
    setSettings(next);
    try {
      await api.saveSettings(next);
    } catch (e) {
      setError(String(e));
    }
  };
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [pendingBatchDelete, setPendingBatchDelete] = useState(false);
  const [dragOver, setDragOver] = useState(false);
  const [theme, setTheme] = useState<"dark" | "light">(
    () => (localStorage.getItem("nebula-theme") as "dark" | "light") || "dark",
  );

  useEffect(() => {
    document.documentElement.setAttribute("data-theme", theme);
    localStorage.setItem("nebula-theme", theme);
  }, [theme]);
  const [transfers, setTransfers] = useState<Record<string, TransferItem>>({});

  const updateProgress = (id: string, done: number, total: number) =>
    setTransfers((prev) =>
      prev[id] ? { ...prev, [id]: { ...prev[id], done, total } } : prev,
    );

  type TransferSpec = {
    id: string;
    kind: "上传" | "下载";
    name: string;
    account: string;
    remote: string;
    local: string;
  };

  const startTransfer = async (t: TransferSpec) => {
    setTransfers((prev) => ({
      ...prev,
      [t.id]: { ...t, done: 0, total: 0, status: "active" },
    }));
    try {
      if (t.kind === "上传") await api.uploadFile(t.account, t.remote, t.local);
      else await api.downloadFile(t.account, t.remote, t.local);
      setTransfers((prev) =>
        prev[t.id]
          ? { ...prev, [t.id]: { ...prev[t.id], status: "done", done: prev[t.id].total } }
          : prev,
      );
    } catch (e) {
      setError(String(e));
      setTransfers((prev) =>
        prev[t.id] ? { ...prev, [t.id]: { ...prev[t.id], status: "error" } } : prev,
      );
    }
  };

  const retryTransfer = (id: string) => {
    const t = transfers[id];
    if (t) void startTransfer(t);
  };

  const clearTransfers = () =>
    setTransfers((prev) =>
      Object.fromEntries(
        Object.entries(prev).filter(([, v]) => v.status === "active"),
      ),
    );
  const [filter, setFilter] = useState("");
  const [sortKey, setSortKey] = useState<"name" | "size" | "modified">("name");
  const [sortDir, setSortDir] = useState<"asc" | "desc">("asc");

  // 切换账号 / 目录时清空过滤词、选择与详情。
  useEffect(() => {
    setFilter("");
    setSelected(new Set());
    setDetailsEntry(null);
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
  const dirCount = visibleEntries.length - visibleFiles.length;
  const totalSize = visibleFiles.reduce((sum, e) => sum + e.size, 0);

  // 网格视图下,为可见图片批量生成缩略图预签名链接。
  useEffect(() => {
    if (view !== "grid" || !current) return;
    const imgs = visibleEntries
      .filter((e) => e.kind === "file" && previewKind(e.name) === "image")
      .map((e) => e.path);
    if (imgs.length === 0) {
      setThumbs({});
      return;
    }
    let cancelled = false;
    api
      .presignBatch(current, imgs, 600)
      .then((urls) => {
        if (cancelled) return;
        const map: Record<string, string> = {};
        imgs.forEach((p, i) => (map[p] = urls[i]));
        setThumbs(map);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [view, current, visibleEntries]);
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
      updateProgress(e.payload.path, e.payload.uploaded, e.payload.total);
    });
    const unDownload = listen<DownloadProgress>("download-progress", (e) => {
      updateProgress(e.payload.path, e.payload.downloaded, e.payload.total);
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
  onDropRef.current = (paths: string[]) => {
    void uploadLocalPaths(paths);
  };

  // 全局键盘快捷键:用 ref 持最新逻辑,监听只注册一次。
  const onKeyRef = useRef<(e: KeyboardEvent) => void>(() => {});
  useEffect(() => {
    const handler = (e: KeyboardEvent) => onKeyRef.current(e);
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, []);

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
    setEditInfo(null);
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

  const editAccount = async (id: string) => {
    try {
      const info = await api.getAccount(id);
      if (info) setEditInfo(info);
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
    await runPool([...selected], settings.concurrency, (p) =>
      startTransfer({
        id: p,
        kind: "下载",
        name: baseName(p),
        account: current,
        remote: p,
        local: `${dir}/${baseName(p)}`,
      }),
    );
    clearSelection();
    setBusy(false);
  };

  // 上传一组本地路径(文件或文件夹)到当前目录,文件夹递归、保留相对路径。
  const uploadLocalPaths = async (localPaths: string[]) => {
    if (!current || !path || localPaths.length === 0) return;
    setBusy(true);
    let entries: { local: string; rel: string }[];
    try {
      entries = await api.expandUploadPaths(localPaths);
    } catch (e) {
      setError(String(e));
      setBusy(false);
      return;
    }
    await runPool(entries, settings.concurrency, (en) =>
      startTransfer({
        id: joinRemote(path, en.rel),
        kind: "上传",
        name: en.rel,
        account: current,
        remote: joinRemote(path, en.rel),
        local: en.local,
      }),
    );
    await load();
    setBusy(false);
  };

  const toPathList = (sel: string | string[] | null): string[] =>
    Array.isArray(sel) ? sel : typeof sel === "string" ? [sel] : [];

  const upload = async () => {
    if (!current || !path) return;
    const sel = await open({ multiple: true, title: "选择要上传的文件" });
    await uploadLocalPaths(toPathList(sel));
  };

  const uploadFolder = async () => {
    if (!current || !path) return;
    const sel = await open({
      directory: true,
      multiple: true,
      title: "选择要上传的文件夹",
    });
    await uploadLocalPaths(toPathList(sel));
  };

  const download = async (entry: Entry) => {
    if (!current) return;
    const target = await save({ defaultPath: entry.name });
    if (typeof target !== "string") return;
    setBusy(true);
    await startTransfer({
      id: entry.path,
      kind: "下载",
      name: entry.name,
      account: current,
      remote: entry.path,
      local: target,
    });
    setBusy(false);
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

  const doMoveCopy = async (mode: "copy" | "move", to: string) => {
    const entry = moveCopyTarget;
    setMoveCopyTarget(null);
    if (!current || !entry) return;
    setBusy(true);
    setError(null);
    try {
      if (mode === "copy") await api.copy(current, entry.path, to);
      else await api.rename(current, entry.path, to);
      await load();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const share = async (entry: Entry) => {
    if (!current) return;
    setError(null);
    try {
      const url = await api.presign(
        current,
        entry.path,
        settings.share_expiry_secs,
      );
      setShareUrl(url);
    } catch (e) {
      setError(String(e));
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

  const anyModalOpen =
    showForm ||
    !!pendingDelete ||
    !!renameTarget ||
    !!moveCopyTarget ||
    !!shareUrl ||
    showNewFolder ||
    pendingBatchDelete ||
    showSettings;

  onKeyRef.current = (e: KeyboardEvent) => {
    const el = e.target as HTMLElement | null;
    const typing =
      !!el && (el.tagName === "INPUT" || el.tagName === "TEXTAREA");

    if (e.key === "Escape") {
      if (showSettings) setShowSettings(false);
      else if (shareUrl) setShareUrl(null);
      else if (moveCopyTarget) setMoveCopyTarget(null);
      else if (renameTarget) setRenameTarget(null);
      else if (showNewFolder) setShowNewFolder(false);
      else if (showForm) setShowForm(false);
      else if (pendingBatchDelete) setPendingBatchDelete(false);
      else if (pendingDelete) setPendingDelete(null);
      else if (detailsEntry) setDetailsEntry(null);
      return;
    }

    if (typing || anyModalOpen || !current) return;
    const mod = e.metaKey || e.ctrlKey;

    if (mod && e.key.toLowerCase() === "a") {
      e.preventDefault();
      setSelected(new Set(visibleFiles.map((f) => f.path)));
    } else if ((e.key === "Delete" || e.key === "Backspace") && selected.size > 0) {
      e.preventDefault();
      setPendingBatchDelete(true);
    } else if (mod && e.key === "ArrowUp" && path !== "") {
      e.preventDefault();
      setPath(parentPath(path));
    }
  };

  return (
    <div className="layout">
      <Sidebar
        accounts={accounts}
        current={current}
        theme={theme}
        onSelect={selectAccount}
        onAdd={() => setShowForm(true)}
        onEdit={editAccount}
        onRemove={removeAccount}
        onToggleTheme={() => setTheme((t) => (t === "dark" ? "light" : "dark"))}
        onSettings={() => setShowSettings(true)}
      />

      <main className="main">
        {dragOver && current && path !== "" && (
          <div className="drop-overlay">
            <FontAwesomeIcon icon={faCloudArrowUp} className="drop-overlay__icon" />
            <span>松开以上传到当前目录(文件夹将递归上传)</span>
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
                view={view}
                onFilter={setFilter}
                onUp={() => setPath(parentPath(path))}
                onRefresh={load}
                onToggleView={() =>
                  setView((v) => (v === "list" ? "grid" : "list"))
                }
                onUpload={upload}
                onUploadFolder={uploadFolder}
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

            {view === "list" ? (
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
                onOpenDir={openDir}
                onOpenFile={openPreview}
                onOpenDetails={setDetailsEntry}
                onContext={(entry, x, y) => setMenu({ x, y, entry })}
              />
            ) : (
              <FileGrid
                entries={visibleEntries}
                loading={loading}
                thumbs={thumbs}
                selected={selected}
                onToggleSelect={toggleSelect}
                onOpenDir={openDir}
                onOpenFile={openPreview}
                onContext={(entry, x, y) => setMenu({ x, y, entry })}
              />
            )}

            {detailsEntry && (
              <FileDetails
                entry={detailsEntry}
                onClose={() => setDetailsEntry(null)}
                onDownload={download}
                onShare={share}
              />
            )}

            {!loading && (
              <div className="statusbar">
                {dirCount} 个目录 · {visibleFiles.length} 个文件 · 共{" "}
                {formatBytes(totalSize)}
                {filter && " · 已过滤"}
                {selected.size > 0 && ` · 已选 ${selected.size}`}
              </div>
            )}

            {Object.keys(transfers).length > 0 && (
              <TransferPanel
                items={Object.values(transfers)}
                onClear={clearTransfers}
                onRetry={retryTransfer}
              />
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

      {(showForm || editInfo) && (
        <AccountForm
          initial={
            editInfo
              ? {
                  id: editInfo.id,
                  accessKeyId: editInfo.access_key_id,
                  endpoint: editInfo.endpoint,
                }
              : undefined
          }
          onSubmit={addAccount}
          onClose={() => {
            setShowForm(false);
            setEditInfo(null);
          }}
        />
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

      {moveCopyTarget && current && (
        <MoveCopyDialog
          account={current}
          from={moveCopyTarget.path}
          onCopy={(to) => doMoveCopy("copy", to)}
          onMove={(to) => doMoveCopy("move", to)}
          onCancel={() => setMoveCopyTarget(null)}
        />
      )}

      {shareUrl && (
        <ShareDialog
          url={shareUrl}
          minutes={Math.round(settings.share_expiry_secs / 60)}
          onClose={() => setShareUrl(null)}
        />
      )}

      {showSettings && (
        <SettingsDialog
          settings={settings}
          onSave={saveSettings}
          onClose={() => setShowSettings(false)}
        />
      )}

      {menu && (
        <ContextMenu
          x={menu.x}
          y={menu.y}
          items={contextItems(menu.entry)}
          onClose={() => setMenu(null)}
        />
      )}

      {preview && (
        <PreviewModal
          url={preview.url}
          name={preview.name}
          kind={preview.kind}
          onClose={() => setPreview(null)}
        />
      )}
    </div>
  );

  function contextItems(entry: Entry): MenuItem[] {
    if (entry.kind === "directory") {
      return [{ label: "打开", onClick: () => openDir(entry) }];
    }
    const items: MenuItem[] = [];
    if (previewKind(entry.name)) {
      items.push({ label: "预览", onClick: () => openPreview(entry) });
    }
    items.push(
      { label: "详情", onClick: () => setDetailsEntry(entry) },
      { label: "下载", onClick: () => download(entry) },
      { label: "重命名", onClick: () => setRenameTarget(entry) },
      { label: "复制 / 移动到", onClick: () => setMoveCopyTarget(entry) },
      { label: "分享链接", onClick: () => share(entry) },
      { label: "删除", danger: true, onClick: () => setPendingDelete(entry) },
    );
    return items;
  }
}
