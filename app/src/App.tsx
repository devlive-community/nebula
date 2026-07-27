import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import { faCloudArrowUp, faPlus } from "@fortawesome/free-solid-svg-icons";
import type {
  AccountInfo,
  Bookmark,
  DownloadProgress,
  Entry,
  RenamePlan,
  FolderProgress,
  Settings,
  StorageBreakdown,
  TransferItem,
  TransferProgress,
  UploadProgress,
} from "./types";
import * as api from "./api";
import {
  baseName,
  formatBytes,
  isBucket,
  joinRemote,
  parentPath,
  previewKind,
  runPool,
} from "./util";
import { useI18n } from "./i18n";
import { Sidebar } from "./components/Sidebar";
import { AccountForm } from "./components/AccountForm";
import { Breadcrumb } from "./components/Breadcrumb";
import { Bookmarks } from "./components/Bookmarks";
import {
  CommandPalette,
  type PaletteCommand,
} from "./components/CommandPalette";
import { BatchRenameDialog } from "./components/BatchRenameDialog";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import { Toolbar } from "./components/Toolbar";
import { FileList } from "./components/FileList";
import { FileGrid } from "./components/FileGrid";
import { ConfirmDialog } from "./components/ConfirmDialog";
import { PromptDialog } from "./components/PromptDialog";
import { MoveCopyDialog } from "./components/MoveCopyDialog";
import { MigrateDialog } from "./components/MigrateDialog";
import {
  RestoreDialog,
  StorageClassDialog,
} from "./components/StorageClassDialog";
import { ShareDialog } from "./components/ShareDialog";
import { FileDetails } from "./components/FileDetails";
import { TagsDialog } from "./components/TagsDialog";
import { BatchTagsDialog } from "./components/BatchTagsDialog";
import { NewTextFileDialog } from "./components/NewTextFileDialog";
import { StatsDialog } from "./components/StatsDialog";
import { CleanupDialog } from "./components/CleanupDialog";
import { TransferPanel } from "./components/TransferPanel";
import { SearchResults } from "./components/SearchResults";
import { SettingsDialog } from "./components/SettingsDialog";
import { SyncDialog } from "./components/SyncDialog";
import { DuplicatesDialog } from "./components/DuplicatesDialog";
import { LargestFilesDialog } from "./components/LargestFilesDialog";
import { matchBinding, resolveBindings, type Bindings } from "./shortcuts";
import { ContextMenu, type MenuItem } from "./components/ContextMenu";
import { PreviewModal } from "./components/PreviewModal";
import { AboutDialog } from "./components/AboutDialog";
import { UpdateDialog } from "./components/UpdateDialog";
import { checkForUpdate, type Update } from "./update";
import { Logo } from "./components/Logo";

/** 在独立窗口打开一张图片(单张)。label 以 image- 开头以匹配 capability 权限。 */
function openImageWindow(account: string, entry: Entry) {
  const q = new URLSearchParams({
    view: "image",
    account,
    path: entry.path,
    name: entry.name,
    etag: entry.etag ?? "",
    size: String(entry.size),
  });
  const label = `image-${Date.now()}-${Math.floor(Math.random() * 1e4)}`;
  const win = new WebviewWindow(label, {
    url: `index.html?${q.toString()}`,
    title: entry.name,
    width: 1100,
    height: 760,
    resizable: true,
  });
  win.once("tauri://error", (e) => console.error("open image window failed", e));
}

/** 在独立窗口打开一个 PDF 编辑器(页面管理)。label 以 pdf- 开头以匹配 capability。 */
function openPdfWindow(account: string, entry: Entry) {
  const q = new URLSearchParams({
    view: "pdf",
    account,
    path: entry.path,
    name: entry.name,
  });
  const label = `pdf-${Date.now()}-${Math.floor(Math.random() * 1e4)}`;
  const win = new WebviewWindow(label, {
    url: `index.html?${q.toString()}`,
    title: entry.name,
    width: 1180,
    height: 800,
    resizable: true,
  });
  win.once("tauri://error", (e) => console.error("open pdf window failed", e));
}

export default function App() {
  const { t } = useI18n();
  const [accounts, setAccounts] = useState<AccountInfo[]>([]);
  const [current, setCurrent] = useState<string | null>(null);
  // 界面偏好(侧栏宽 / 视图 / 主题)持久化在 SQLite(ui_prefs 表)。先用默认值渲染,
  // 挂载后从后端加载并回填;prefsHydrated 为真前不回写,避免用默认值覆盖已存的偏好。
  const prefsHydrated = useRef(false);
  const [sidebarWidth, setSidebarWidth] = useState<number>(240);
  const [path, setPath] = useState("");
  const [entries, setEntries] = useState<Entry[]>([]);
  const [cursor, setCursor] = useState<string | null>(null);
  const [loadingMore, setLoadingMore] = useState(false);
  const [loading, setLoading] = useState(false);
  const [search, setSearch] = useState<{
    query: string;
    results: Entry[];
    loading: boolean;
    truncated: boolean;
    /** 内容搜索时:路径 → 命中片段。 */
    snippets?: Record<string, string>;
  } | null>(null);
  // 搜索过滤:minSize 字节(0=不限),ext 扩展名(空=不限)。
  const [searchFilter, setSearchFilter] = useState({ minSize: 0, ext: "" });
  // 内容搜索模式(搜文件内容而非文件名)。
  const [contentSearch, setContentSearch] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<{
    tone: "ok" | "warn" | "err" | "info";
    text: string;
  } | null>(null);
  const [showForm, setShowForm] = useState(false);
  const [editInfo, setEditInfo] = useState<AccountInfo | null>(null);
  const [pendingDelete, setPendingDelete] = useState<Entry | null>(null);
  const [renameTarget, setRenameTarget] = useState<Entry | null>(null);
  const [moveCopyTarget, setMoveCopyTarget] = useState<Entry | null>(null);
  const [migrateTarget, setMigrateTarget] = useState<Entry | null>(null);
  const [storageClassTarget, setStorageClassTarget] = useState<Entry | null>(
    null,
  );
  const [restoreTarget, setRestoreTarget] = useState<Entry | null>(null);
  const [shareUrl, setShareUrl] = useState<{
    url: string;
    upload: boolean;
  } | null>(null);
  const [detailsEntry, setDetailsEntry] = useState<Entry | null>(null);
  const [editTypeTarget, setEditTypeTarget] = useState<Entry | null>(null);
  const [tagsTarget, setTagsTarget] = useState<Entry | null>(null);
  const [statsTarget, setStatsTarget] = useState<Entry | null>(null);
  const [statsData, setStatsData] = useState<StorageBreakdown | null>(null);
  const [cleanupTarget, setCleanupTarget] = useState<Entry | null>(null);
  // 收藏夹持久化在 SQLite(bookmarks 表);启动时加载,增删即时写回。
  const [bookmarks, setBookmarks] = useState<Bookmark[]>([]);
  useEffect(() => {
    api.getBookmarks().then(setBookmarks).catch(() => {});
  }, []);
  const isBookmarked =
    !!current && bookmarks.some((b) => b.account === current && b.path === path);
  const toggleBookmark = async () => {
    if (!current) return;
    if (isBookmarked) {
      await api.removeBookmark(current, path);
      setBookmarks((bm) =>
        bm.filter((b) => !(b.account === current && b.path === path)),
      );
    } else {
      await api.addBookmark(current, path);
      setBookmarks((bm) => [{ account: current, path }, ...bm]);
    }
  };
  const removeBookmark = async (account: string, p: string) => {
    await api.removeBookmark(account, p);
    setBookmarks((bm) =>
      bm.filter((b) => !(b.account === account && b.path === p)),
    );
  };
  const jumpBookmark = (account: string, p: string) => {
    setCurrent(account);
    setPath(p);
  };
  const [showPalette, setShowPalette] = useState(false);
  // 最近访问持久化在 SQLite(recent_locations 表);启动加载,导航时记录。
  const [recents, setRecents] = useState<Bookmark[]>([]);
  useEffect(() => {
    api.getRecentLocations().then(setRecents).catch(() => {});
  }, []);
  // 停留 800ms 才记一次,避免快速穿行目录时频繁写库。
  useEffect(() => {
    if (!current) return;
    const acct = current;
    const p = path;
    const id = setTimeout(() => {
      api
        .recordVisit(acct, p)
        .then(() => api.getRecentLocations())
        .then(setRecents)
        .catch(() => {});
    }, 800);
    return () => clearTimeout(id);
  }, [current, path]);
  const [showNewFolder, setShowNewFolder] = useState(false);
  const [showNewBucket, setShowNewBucket] = useState(false);
  const [settings, setSettings] = useState<Settings>({
    share_expiry_secs: 3600,
    concurrency: 3,
    rate_limit_kib_per_sec: 0,
  });
  const [showSettings, setShowSettings] = useState(false);
  // 备份/同步对话框的云端前缀(null = 未打开);从命令面板用当前路径,从右键用该目录路径。
  const [syncPrefix, setSyncPrefix] = useState<string | null>(null);
  // 查找重复文件对话框的根路径(null = 未打开)。
  const [dupRoot, setDupRoot] = useState<string | null>(null);
  // 大文件排行对话框的根路径(null = 未打开)。
  const [largeRoot, setLargeRoot] = useState<string | null>(null);
  // 自定义快捷键(动作 id → 绑定串;仅存用户改过的项),持久化在 ui_prefs。
  const [shortcuts, setShortcuts] = useState<Bindings>({});
  const [showAbout, setShowAbout] = useState(false);
  const [update, setUpdate] = useState<Update | null>(null);
  const [updateFlash, setUpdateFlash] = useState<string | null>(null);
  const [menu, setMenu] = useState<{ x: number; y: number; entry: Entry } | null>(
    null,
  );
  const [preview, setPreview] = useState<{
    name: string;
    kind: "image" | "video" | "audio" | "pdf" | "text";
    url?: string;
    text?: string;
    truncated?: boolean;
  } | null>(null);
  const [view, setView] = useState<"list" | "grid">("list");
  const [thumbs, setThumbs] = useState<Record<string, string>>({});

  useEffect(() => {
    if (prefsHydrated.current) api.setPref("view", view).catch(() => {});
  }, [view]);

  const openDir = (entry: Entry) =>
    setPath(entry.path.endsWith("/") ? entry.path : entry.path + "/");

  // 预览:图片 / 视频用预签名链接加载;文本类走后端读前 256 KiB(绕开云端 CORS 并限体积);
  // 非可预览类型退回详情。
  const openPreview = async (entry: Entry) => {
    const kind = previewKind(entry.name);
    if (!current || !kind) {
      setDetailsEntry(entry);
      return;
    }
    // 图片在独立窗口打开(只这一张):Rust 解码/缩放,前端做缩放/平移/旋转/EXIF。
    if (kind === "image") {
      openImageWindow(current, entry);
      return;
    }
    // PDF 在独立窗口打开页面编辑器:后端 lopdf 解析/组装,前端 pdf.js 出缩略图。
    if (kind === "pdf") {
      openPdfWindow(current, entry);
      return;
    }
    setError(null);
    try {
      if (kind === "text") {
        const { text, truncated } = await api.readPreview(
          current,
          entry.path,
          256 * 1024,
        );
        setPreview({ name: entry.name, kind, text, truncated });
      } else {
        const url = await api.presign(current, entry.path, 600);
        setPreview({ name: entry.name, kind, url });
      }
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
  const [batchStorageClass, setBatchStorageClass] = useState(false);
  const [batchRestore, setBatchRestore] = useState(false);
  const [showBatchRename, setShowBatchRename] = useState(false);
  const [showBatchTags, setShowBatchTags] = useState(false);
  const [showBatchMove, setShowBatchMove] = useState(false);
  const [showNewText, setShowNewText] = useState(false);
  const [dragOver, setDragOver] = useState(false);
  // 主题偏好:深色 / 浅色 / 跟随系统。实际生效主题由偏好 + 系统色推导。
  const [themePref, setThemePref] = useState<"dark" | "light" | "system">("system");
  const [systemDark, setSystemDark] = useState(
    () =>
      typeof window !== "undefined" &&
      window.matchMedia("(prefers-color-scheme: dark)").matches,
  );
  const theme: "dark" | "light" =
    themePref === "system" ? (systemDark ? "dark" : "light") : themePref;
  // 跟随系统时监听系统日夜切换。
  useEffect(() => {
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const on = (e: MediaQueryListEvent) => setSystemDark(e.matches);
    mq.addEventListener("change", on);
    return () => mq.removeEventListener("change", on);
  }, []);

  useEffect(() => {
    document.documentElement.setAttribute("data-theme", theme);
  }, [theme]);
  // 持久化偏好(而非推导结果)。
  useEffect(() => {
    if (prefsHydrated.current) api.setPref("theme", themePref).catch(() => {});
  }, [themePref]);
  // 切换按钮 / 命令:在当前生效主题的基础上翻转成一个显式主题。
  const toggleTheme = () =>
    setThemePref(theme === "dark" ? "light" : "dark");

  // 导航历史(前进 / 后退):监听 current + path 变化入栈,前进后退时不重复入栈。
  const navHistory = useRef<{ account: string | null; path: string }[]>([]);
  const navIdx = useRef(-1);
  const navBack = useRef(false);
  const [navVer, setNavVer] = useState(0);
  useEffect(() => {
    if (navBack.current) {
      navBack.current = false;
      return;
    }
    const cut = navHistory.current.slice(0, navIdx.current + 1);
    const last = cut[cut.length - 1];
    if (last && last.account === current && last.path === path) return;
    cut.push({ account: current, path });
    navHistory.current = cut;
    navIdx.current = cut.length - 1;
    setNavVer((v) => v + 1);
  }, [current, path]);
  const goHistory = (delta: number) => {
    const target = navIdx.current + delta;
    if (target < 0 || target >= navHistory.current.length) return;
    navIdx.current = target;
    navBack.current = true;
    const e = navHistory.current[target];
    if (e.account !== current) setCurrent(e.account);
    setPath(e.path);
    setNavVer((v) => v + 1);
  };
  void navVer;
  const canBack = navIdx.current > 0;
  const canForward = navIdx.current < navHistory.current.length - 1;
  // 快捷键改动后持久化(仅存用户覆盖的项)。
  useEffect(() => {
    if (prefsHydrated.current)
      api.setPref("shortcuts", JSON.stringify(shortcuts)).catch(() => {});
  }, [shortcuts]);
  const [transfers, setTransfers] = useState<Record<string, TransferItem>>({});

  // 每个传输的上次采样,用于算瞬时速度(EMA 平滑)。不入 state,不持久化。
  const speedRef = useRef<Record<string, { t: number; done: number; speed: number }>>(
    {},
  );
  const updateProgress = (id: string, done: number, total: number) => {
    const now = Date.now();
    const p = speedRef.current[id];
    let speed = 0;
    if (p && now > p.t && done >= p.done) {
      const inst = ((done - p.done) * 1000) / (now - p.t);
      speed = p.speed > 0 ? p.speed * 0.7 + inst * 0.3 : inst;
    }
    speedRef.current[id] = { t: now, done, speed };
    setTransfers((prev) =>
      prev[id] ? { ...prev, [id]: { ...prev[id], done, total, speed } } : prev,
    );
  };

  // 传输列表持久化:只在**状态**变化时写库(进度 tick 不写),完成即删。
  const persistedRef = useRef<Record<string, TransferItem["status"]>>({});
  useEffect(() => {
    const prev = persistedRef.current;
    const next: Record<string, TransferItem["status"]> = {};
    for (const [id, item] of Object.entries(transfers)) {
      next[id] = item.status;
      if (prev[id] === item.status) continue; // 状态没变(含进度 tick)→ 不写库
      if (item.status === "done") void api.deleteTransfer(id);
      else void api.saveTransfer(item);
    }
    // 面板里被移除(清除已完成)→ 从库删
    for (const id of Object.keys(prev)) {
      if (!(id in transfers)) void api.deleteTransfer(id);
    }
    persistedRef.current = next;
  }, [transfers]);

  // 启动时恢复未完成的传输;关 App 时还在传的(active)标为「已中断」,可手动续传。
  useEffect(() => {
    void (async () => {
      const rows = await api.listTransfers();
      if (rows.length === 0) return;
      const restored: Record<string, TransferItem> = {};
      const persisted: Record<string, TransferItem["status"]> = {};
      for (const r of rows) {
        persisted[r.id] = r.status;
        restored[r.id] = {
          ...r,
          status: r.status === "active" ? "interrupted" : r.status,
        };
      }
      persistedRef.current = persisted;
      setTransfers(restored);
    })();
  }, []);

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
      let skipped = false;
      if (t.kind === "上传")
        skipped = await api.uploadFile(t.account, t.remote, t.local, t.id);
      else skipped = await api.downloadFile(t.account, t.remote, t.local, t.id);
      setTransfers((prev) =>
        prev[t.id]
          ? {
              ...prev,
              [t.id]: {
                ...prev[t.id],
                status: "done",
                done: prev[t.id].total,
                skipped,
              },
            }
          : prev,
      );
    } catch (e) {
      // 后端以 "已取消" 作为取消信号:标记为已取消,不弹错误横幅。
      const msg = String(e);
      const cancelled = msg === "已取消";
      setTransfers((prev) =>
        prev[t.id]
          ? { ...prev, [t.id]: { ...prev[t.id], status: cancelled ? "cancelled" : "error" } }
          : prev,
      );
      if (!cancelled) setError(msg);
    }
  };

  const retryTransfer = (id: string) => {
    const t = transfers[id];
    if (!t) return;
    // 单文件上传 / 下载:直接重发(会从断点续传)。
    if (t.kind === "上传" || t.kind === "下载") {
      void startTransfer({ ...t, kind: t.kind });
    } else if (t.kind === "下载文件夹") {
      // 文件夹下载:用记下的账号 / 路径 / 本地目录重跑,无需再次选目录。
      void runFolderDownload(t.account, t.remote, t.local, t.name);
    } else if (t.kind === "迁移" && t.srcAccount && t.srcPath) {
      // 迁移从头重跑(非断点续传);源端信息仅本会话在内存中,重启后不可用。
      void runMigrateSingle(
        t.srcAccount,
        t.srcPath,
        t.total,
        t.account,
        t.remote,
        t.name,
      );
    } else if (t.kind === "迁移文件夹" && t.srcAccount && t.srcPath) {
      void runMigrateFolder(t.srcAccount, t.srcPath, t.account, t.remote, t.name);
    }
  };

  const cancelTransfer = (id: string) => {
    // 乐观标记为已取消(即时反馈),再通知后端在下个分片 / 数据块中止。
    setTransfers((prev) =>
      prev[id] && prev[id].status === "active"
        ? { ...prev, [id]: { ...prev[id], status: "cancelled" } }
        : prev,
    );
    void api.cancelTransfer(id);
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
  // 排序偏好持久化(重启后记住按什么列、升降序)。
  useEffect(() => {
    if (prefsHydrated.current) {
      api.setPref("sort_key", sortKey).catch(() => {});
      api.setPref("sort_dir", sortDir).catch(() => {});
    }
  }, [sortKey, sortDir]);

  // 切换账号 / 目录时清空过滤词、选择、详情与搜索结果。
  useEffect(() => {
    setFilter("");
    setSelected(new Set());
    setDetailsEntry(null);
    setSearch(null);
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

  // 网格视图下,为可见图片生成缩略图。走 Rust 侧缩略图(只下载一次、只回传小图,
  // 并按 ETag 缓存),不再让浏览器为每张缩略图拉取整张原图。并发上限保护。
  useEffect(() => {
    if (view !== "grid" || !current) return;
    const imgs = visibleEntries.filter(
      (e) => e.kind === "file" && previewKind(e.name) === "image",
    );
    if (imgs.length === 0) {
      setThumbs({});
      return;
    }
    let cancelled = false;
    setThumbs({});
    void runPool(imgs, 6, async (e) => {
      if (cancelled) return;
      try {
        const d = await api.imageThumb(current, e.path, e.etag, 160);
        if (!cancelled) setThumbs((m) => ({ ...m, [e.path]: d.data_url }));
      } catch {
        // 单张失败忽略,不影响其它缩略图
      }
    });
    return () => {
      cancelled = true;
    };
  }, [view, current, visibleEntries]);
  const allSelected =
    visibleFiles.length > 0 && visibleFiles.every((f) => selected.has(f.path));

  // 上次点选的锚点路径(Shift 范围多选用)。
  const selAnchor = useRef<string | null>(null);
  const toggleSelect = (p: string, shift?: boolean) => {
    // 当前视图有序文件路径:搜索结果里用搜索列表,否则用可见文件。
    const ordered = (
      search ? search.results.filter((e) => e.kind === "file") : visibleFiles
    ).map((f) => f.path);
    if (shift && selAnchor.current) {
      const a = ordered.indexOf(selAnchor.current);
      const b = ordered.indexOf(p);
      if (a !== -1 && b !== -1) {
        const [lo, hi] = a < b ? [a, b] : [b, a];
        setSelected((prev) => {
          const next = new Set(prev);
          for (let i = lo; i <= hi; i++) next.add(ordered[i]);
          return next;
        });
        return;
      }
    }
    selAnchor.current = p;
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(p)) next.delete(p);
      else next.add(p);
      return next;
    });
  };

  const toggleSelectAll = () =>
    setSelected((prev) => {
      const next = new Set(prev);
      if (allSelected) visibleFiles.forEach((f) => next.delete(f.path));
      else visibleFiles.forEach((f) => next.add(f.path));
      return next;
    });

  const clearSelection = () => setSelected(new Set());

  // 搜索结果的全选 / 取消全选(作用于当前搜索命中,而非目录列表)。
  const toggleSelectAllSearch = () =>
    setSelected((prev) => {
      const results = search?.results ?? [];
      const allSel =
        results.length > 0 && results.every((r) => prev.has(r.path));
      const next = new Set(prev);
      for (const r of results) {
        if (allSel) next.delete(r.path);
        else next.add(r.path);
      }
      return next;
    });

  useEffect(() => {
    const unUpload = listen<UploadProgress>("upload-progress", (e) => {
      updateProgress(e.payload.path, e.payload.uploaded, e.payload.total);
    });
    const unDownload = listen<DownloadProgress>("download-progress", (e) => {
      updateProgress(e.payload.path, e.payload.downloaded, e.payload.total);
    });
    const unTransfer = listen<TransferProgress>("transfer-progress", (e) => {
      updateProgress(`migrate:${e.payload.to}`, e.payload.transferred, e.payload.total);
    });
    const unFolder = listen<FolderProgress>("folder-progress", (e) => {
      const { op, path, done, total } = e.payload;
      updateProgress(`folder:${op}:${path}`, done, total);
    });
    return () => {
      unUpload.then((off) => off());
      unDownload.then((off) => off());
      unTransfer.then((off) => off());
      unFolder.then((off) => off());
    };
  }, []);

  // 完成类提示(通过 / 警告 / 失败)几秒后自动消失;"info"(进行中)保留到被替换。
  useEffect(() => {
    if (!notice || notice.tone === "info") return;
    const t = setTimeout(() => setNotice(null), 6000);
    return () => clearTimeout(t);
  }, [notice]);

  const refreshAccounts = useCallback(async () => {
    const list = await api.listAccountInfos();
    setAccounts(list);
    setCurrent((cur) => cur ?? list[0]?.id ?? null);
  }, []);

  useEffect(() => {
    refreshAccounts();
  }, [refreshAccounts]);

  useEffect(() => {
    if (prefsHydrated.current)
      api.setPref("sidebar_width", String(sidebarWidth)).catch(() => {});
  }, [sidebarWidth]);

  // 挂载后一次性从 SQLite 加载界面偏好并回填,完成后才允许上面的 effect 回写。
  useEffect(() => {
    (async () => {
      try {
        const [sw, v, th, sc, sk, sd] = await Promise.all([
          api.getPref("sidebar_width"),
          api.getPref("view"),
          api.getPref("theme"),
          api.getPref("shortcuts"),
          api.getPref("sort_key"),
          api.getPref("sort_dir"),
        ]);
        const n = Number(sw);
        if (n >= 180 && n <= 480) setSidebarWidth(n);
        if (v === "list" || v === "grid") setView(v);
        if (th === "dark" || th === "light" || th === "system") setThemePref(th);
        if (sk === "name" || sk === "size" || sk === "modified") setSortKey(sk);
        if (sd === "asc" || sd === "desc") setSortDir(sd);
        if (sc) {
          try {
            setShortcuts(JSON.parse(sc));
          } catch {
            // 忽略损坏的偏好
          }
        }
      } catch {
        // 忽略:无存储时保持默认值
      } finally {
        prefsHydrated.current = true;
      }
    })();
  }, []);

  // 定时同步:应用打开时,每分钟检查已保存任务,到点的静默执行并更新 last_run。
  const runningJobs = useRef<Set<string>>(new Set());
  useEffect(() => {
    const tick = async () => {
      let jobs;
      try {
        jobs = await api.syncJobs();
      } catch {
        return;
      }
      const now = Math.floor(Date.now() / 1000);
      for (const j of jobs) {
        if (
          j.interval_mins > 0 &&
          !runningJobs.current.has(j.id) &&
          now - j.last_run >= j.interval_mins * 60
        ) {
          runningJobs.current.add(j.id);
          api
            .syncRun(`job-run-${j.id}`, j.spec)
            .then((rep) => {
              const result = `↑${rep.uploaded} ↓${rep.downloaded} ✕${
                rep.deleted_remote + rep.deleted_local
              }${rep.failed ? ` ⚠${rep.failed}` : ""}`;
              void api.saveSyncJob({
                ...j,
                last_run: Math.floor(Date.now() / 1000),
                last_result: result,
              });
              if (rep.uploaded + rep.downloaded + rep.deleted_remote + rep.deleted_local > 0)
                setNotice({
                  tone: rep.failed ? "warn" : "ok",
                  text: t("定时同步「{name}」完成:{result}", {
                    name: j.name,
                    result,
                  }),
                });
            })
            .catch((e) => {
              void api.saveSyncJob({
                ...j,
                last_run: Math.floor(Date.now() / 1000),
                last_result: t("失败"),
              });
              setNotice({
                tone: "err",
                text: t("定时同步「{name}」失败:{msg}", {
                  name: j.name,
                  msg: String(e),
                }),
              });
            })
            .finally(() => {
              runningJobs.current.delete(j.id);
            });
        }
      }
    };
    const id = setInterval(tick, 60_000);
    void tick();
    return () => clearInterval(id);
  }, []);

  // 拖拽侧栏右边缘调整宽度(限制在 180–480px)。
  const startResize = (e: React.MouseEvent) => {
    e.preventDefault();
    const onMove = (ev: MouseEvent) => {
      setSidebarWidth(Math.min(480, Math.max(180, ev.clientX)));
    };
    const onUp = () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
      document.body.classList.remove("resizing");
    };
    document.body.classList.add("resizing");
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
  };

  const load = useCallback(async () => {
    if (!current) {
      setEntries([]);
      setCursor(null);
      return;
    }
    setLoading(true);
    setError(null);
    try {
      const page = await api.browsePage(current, path, null);
      setEntries(page.entries);
      setCursor(page.cursor);
    } catch (e) {
      setError(String(e));
      setEntries([]);
      setCursor(null);
    } finally {
      setLoading(false);
    }
  }, [current, path]);

  // 滚到底部且本页已渲染完时,拉取下一页并追加。
  const loadMore = useCallback(async () => {
    if (!current || !cursor || loadingMore) return;
    setLoadingMore(true);
    try {
      const page = await api.browsePage(current, path, cursor);
      setEntries((prev) => [...prev, ...page.entries]);
      setCursor(page.cursor);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoadingMore(false);
    }
  }, [current, path, cursor, loadingMore]);

  useEffect(() => {
    load();
  }, [load]);

  // 从当前目录递归搜索(全桶或子树,取决于所在层级)。
  const runSearch = useCallback(
    async (
      query: string,
      filter: { minSize: number; ext: string },
      content = false,
    ) => {
      if (!current || path === "") return;
      setSelected(new Set()); // 选择按视图隔离:进入搜索先清空目录里的选中
      setSearch({ query, results: [], loading: true, truncated: false });
      try {
        if (content) {
          const res = await api.searchContent(current, path, query, 200);
          setSearch({
            query,
            results: res.hits.map((h) => h.entry),
            snippets: Object.fromEntries(
              res.hits.map((h) => [h.entry.path, h.snippet]),
            ),
            loading: false,
            truncated: res.truncated,
          });
        } else {
          const res = await api.search(
            current,
            path,
            query,
            filter.minSize || null,
            filter.ext.trim() || null,
            500,
          );
          setSearch({
            query,
            results: res.entries,
            loading: false,
            truncated: res.truncated,
          });
        }
      } catch (e) {
        setError(String(e));
        setSearch(null);
      }
    },
    [current, path],
  );

  // 改变过滤条件时,用当前关键词重跑搜索。
  const applySearchFilter = (minSize: number, ext: string) => {
    setSearchFilter({ minSize, ext });
    if (search) void runSearch(search.query, { minSize, ext }, contentSearch);
  };
  // 切换「按名 / 按内容」搜索并重跑。
  const toggleContentSearch = () => {
    const next = !contentSearch;
    setContentSearch(next);
    if (search) void runSearch(search.query, searchFilter, next);
  };

  // 导出文件夹清单为 CSV(盘点 / 审计)。
  const exportManifest = async (root: string) => {
    if (!current) return;
    const base = root === "" ? "root" : root.split("/").filter(Boolean).pop();
    const dest = await save({
      defaultPath: `${base}-manifest.csv`,
      filters: [{ name: "CSV", extensions: ["csv"] }],
    });
    if (typeof dest !== "string") return;
    try {
      await api.exportManifest(current, root, dest);
      setNotice({ tone: "ok", text: t("✓ 已导出清单") });
    } catch (e) {
      setNotice({ tone: "err", text: String(e) });
    }
  };

  // 打开搜索结果:跳到其所在目录并退出搜索。
  const openSearchResult = (entry: Entry) => {
    setSearch(null);
    setPath(parentPath(entry.path));
  };

  // 原生菜单点击 → 前端动作。用 ref 持最新逻辑,事件监听只注册一次。
  const onMenuRef = useRef<(action: string) => void>(() => {});
  onMenuRef.current = (action: string) => {
    if (action === "about") setShowAbout(true);
    else if (action === "settings") setShowSettings(true);
    else if (action === "add-account") setShowForm(true);
    else if (action === "toggle-theme") toggleTheme();
    else if (action === "refresh") void load();
    else if (action === "check-update") void runUpdateCheck(true);
  };
  useEffect(() => {
    const un = listen<string>("menu-action", (e) => onMenuRef.current(e.payload));
    return () => {
      un.then((off) => off());
    };
  }, []);

  // 检查更新:`manual` 为菜单手动触发(会给出"已是最新/失败"反馈);启动时静默检查。
  const runUpdateCheck = async (manual: boolean) => {
    if (manual) setUpdateFlash("正在检查更新…");
    try {
      const found = await checkForUpdate();
      if (found) {
        setUpdate(found);
        if (manual) setUpdateFlash(null);
      } else if (manual) {
        setUpdateFlash("当前已是最新版本");
      }
    } catch (e) {
      if (manual) setUpdateFlash(`检查更新失败:${e}`);
    }
  };

  // 启动后静默检查一次新版本(失败不打扰)。
  useEffect(() => {
    void runUpdateCheck(false);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // 更新提示条几秒后自动消失(下载弹窗打开时不显示)。
  useEffect(() => {
    if (!updateFlash) return;
    const t = setTimeout(() => setUpdateFlash(null), 4000);
    return () => clearTimeout(t);
  }, [updateFlash]);

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
    vendor: "aliyun" | "huawei" | "qiniu" | "aws" | "r2" | "minio" | "tencent",
    id: string,
    ak: string,
    sk: string,
    endpoint: string,
    customDomain: string,
  ) => {
    setShowForm(false);
    setEditInfo(null);
    setError(null);
    try {
      const adders = {
        aliyun: api.addAliyunAccount,
        huawei: api.addHuaweiAccount,
        qiniu: api.addQiniuAccount,
        aws: api.addAwsAccount,
        r2: api.addR2Account,
        minio: api.addMinioAccount,
        tencent: api.addTencentAccount,
      };
      await adders[vendor](id, ak, sk, endpoint);
      await api.setAccountDomain(id, customDomain.trim());
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
    const deleted = new Set(selected);
    try {
      for (const p of deleted) await api.deletePath(current, p);
      clearSelection();
      // 搜索视图里把已删的命中去掉,避免残留。
      setSearch((s) =>
        s ? { ...s, results: s.results.filter((r) => !deleted.has(r.path)) } : s,
      );
      await load();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  // 批量设置 ACL(公开读 / 私有)。
  const batchAcl = async (isPublic: boolean) => {
    if (!current || selected.size === 0) return;
    setBusy(true);
    try {
      const ok = await api.setAclBatch(
        `acl-${Date.now()}`,
        current,
        [...selected],
        isPublic,
      );
      setNotice({
        tone: "ok",
        text: isPublic
          ? t("✓ 已把 {n} 项设为公开读", { n: String(ok) })
          : t("✓ 已把 {n} 项设为私有", { n: String(ok) }),
      });
      clearSelection();
    } catch (e) {
      setNotice({ tone: "err", text: String(e) });
    }
    setBusy(false);
  };

  // 批量移动 / 复制选中对象到目标目录。
  const batchMoveCopy = async (dstDir: string, isMove: boolean) => {
    setShowBatchMove(false);
    if (!current || selected.size === 0) return;
    setBusy(true);
    try {
      const ok = await api.moveCopyBatch(
        `movecopy-${Date.now()}`,
        current,
        [...selected],
        dstDir,
        isMove,
      );
      setNotice({
        tone: "ok",
        text: isMove
          ? t("✓ 已移动 {n} 项", { n: String(ok) })
          : t("✓ 已复制 {n} 项", { n: String(ok) }),
      });
      clearSelection();
      await load();
    } catch (e) {
      setNotice({ tone: "err", text: String(e) });
    }
    setBusy(false);
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

  const doBatchStorageClass = async (storageClass: string) => {
    setBatchStorageClass(false);
    if (!current || selected.size === 0) return;
    const paths = [...selected];
    setError(null);
    setNotice({ tone: "info", text: `正在转换 ${paths.length} 个对象的存储类型…` });
    let ok = 0;
    let fail = 0;
    await runPool(paths, settings.concurrency, async (p) => {
      try {
        await api.setStorageClass(current, p, storageClass);
        ok += 1;
      } catch {
        fail += 1;
      }
    });
    clearSelection();
    setNotice({
      tone: fail ? "warn" : "ok",
      text: `存储类型转换:成功 ${ok}${fail ? `,失败 ${fail}` : ""}`,
    });
    await load();
  };

  const doBatchRestore = async (days: number) => {
    setBatchRestore(false);
    if (!current || selected.size === 0) return;
    const paths = [...selected];
    setError(null);
    setNotice({ tone: "info", text: `正在发起取回 ${paths.length} 个对象…` });
    let ok = 0;
    let fail = 0;
    await runPool(paths, settings.concurrency, async (p) => {
      try {
        await api.restoreObject(current, p, days);
        ok += 1;
      } catch {
        fail += 1;
      }
    });
    clearSelection();
    setNotice({
      tone: fail ? "warn" : "ok",
      text: `已发起取回:成功 ${ok}${fail ? `,失败 ${fail}(可能非归档对象)` : ""}`,
    });
  };

  // 批量重命名:按计划逐个改名(复用单个 rename 的 copy+delete),失败项单独统计。
  const applyBatchRename = async (plan: RenamePlan[]) => {
    setShowBatchRename(false);
    if (!current || plan.length === 0) return;
    setError(null);
    setNotice({ tone: "info", text: t("正在重命名 {n} 个对象…", { n: plan.length }) });
    let ok = 0;
    let fail = 0;
    await runPool(plan, settings.concurrency, async (p) => {
      try {
        await api.rename(current, p.from, p.to);
        ok += 1;
      } catch {
        fail += 1;
      }
    });
    clearSelection();
    setNotice({
      tone: fail ? "warn" : "ok",
      text: fail
        ? t("重命名:成功 {ok},失败 {fail}", { ok, fail })
        : t("✓ 已重命名 {n} 个对象", { n: ok }),
    });
    await load();
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

  // 跑一次文件夹下载(供首次发起与"继续/重试"复用,复用同一个传输 id)。
  const runFolderDownload = async (
    account: string,
    remote: string,
    dir: string,
    name: string,
  ) => {
    const id = `folder:download:${remote}`;
    setTransfers((prev) => ({
      ...prev,
      [id]: {
        id,
        kind: "下载文件夹",
        name,
        account,
        remote,
        local: dir,
        done: 0,
        total: 0,
        status: "active",
      },
    }));
    try {
      const skipped = await api.downloadFolder(account, remote, dir, id);
      setTransfers((prev) =>
        prev[id]
          ? { ...prev, [id]: { ...prev[id], status: "done", done: prev[id].total } }
          : prev,
      );
      if (skipped > 0) {
        setNotice({
          tone: "ok",
          text: t("✓ {name} 下载完成({n} 个未改动已跳过)", { name, n: skipped }),
        });
      }
    } catch (e) {
      const msg = String(e);
      const cancelled = msg === "已取消";
      setTransfers((prev) =>
        prev[id]
          ? { ...prev, [id]: { ...prev[id], status: cancelled ? "cancelled" : "error" } }
          : prev,
      );
      if (!cancelled) setError(msg);
    }
  };

  const downloadFolderEntry = async (entry: Entry) => {
    if (!current) return;
    const dir = await open({ directory: true, title: "选择下载到的目录" });
    if (typeof dir !== "string") return;
    await runFolderDownload(current, entry.path, dir, entry.name);
  };

  const doDelete = async () => {
    const entry = pendingDelete;
    setPendingDelete(null);
    if (!current || !entry) return;
    setBusy(true);
    setError(null);
    try {
      if (isBucket(entry)) await api.deleteBucket(current, entry.path);
      else if (entry.kind === "directory")
        await api.deleteFolder(current, entry.path);
      else await api.deletePath(current, entry.path);
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
    // 文件夹重命名是整目录递归移动(可能很多对象)→ 走传输面板任务;单对象直接改名。
    if (entry.kind === "directory") {
      const dstRoot = joinRemote(parentPath(entry.path), newName);
      void runMoveFolder(entry.path, dstRoot, newName, "重命名文件夹");
      return;
    }
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

  // 应用内拖拽移动:拖动的文件路径集合。
  const dragPathsRef = useRef<string[]>([]);
  const onDragStartFile = (entry: Entry) => {
    dragPathsRef.current =
      selected.has(entry.path) && selected.size > 0
        ? [...selected]
        : [entry.path];
  };
  const onDropDir = async (dir: Entry) => {
    const srcs = dragPathsRef.current;
    dragPathsRef.current = [];
    if (!current || srcs.length === 0) return;
    const targetDir = dir.path.endsWith("/") ? dir.path : dir.path + "/";
    const moves = srcs
      .map((from) => ({ from, to: targetDir + baseName(from) }))
      .filter((m) => m.to !== m.from);
    if (moves.length === 0) return;
    setBusy(true);
    setError(null);
    await runPool(moves, settings.concurrency, async (m) => {
      try {
        await api.rename(current, m.from, m.to);
      } catch (e) {
        setError(String(e));
      }
    });
    clearSelection();
    await load();
    setBusy(false);
  };

  const doMoveCopy = async (mode: "copy" | "move", to: string) => {
    const entry = moveCopyTarget;
    setMoveCopyTarget(null);
    if (!current || !entry) return;
    // 文件夹是整目录递归 → 走传输面板任务;单对象走即时的服务端复制 / 改名。
    if (entry.kind === "directory") {
      if (mode === "copy") void runCopyFolder(entry.path, to, entry.name);
      else void runMoveFolder(entry.path, to, entry.name, "移动文件夹");
      return;
    }
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

  // 文件夹移动 / 重命名:同账号服务端复制到新前缀后删原对象,进度按文件数。
  const runMoveFolder = async (
    srcRoot: string,
    dstRoot: string,
    name: string,
    kind: "重命名文件夹" | "移动文件夹" = "移动文件夹",
  ) => {
    const fid = `folder:move:${srcRoot}`;
    setTransfers((prev) => ({
      ...prev,
      [fid]: {
        id: fid,
        kind,
        name,
        account: current ?? "",
        remote: dstRoot,
        local: `${srcRoot} → ${dstRoot}`,
        done: 0,
        total: 0,
        status: "active",
      },
    }));
    try {
      await api.moveFolder(current!, srcRoot, dstRoot, fid);
      setTransfers((prev) =>
        prev[fid]
          ? { ...prev, [fid]: { ...prev[fid], status: "done", done: prev[fid].total } }
          : prev,
      );
      await load();
    } catch (e) {
      const msg = String(e);
      const cancelled = msg === "已取消";
      setTransfers((prev) =>
        prev[fid]
          ? { ...prev, [fid]: { ...prev[fid], status: cancelled ? "cancelled" : "error" } }
          : prev,
      );
      if (!cancelled) setError(msg);
    }
  };

  // 文件夹复制:同账号服务端复制到新前缀(保留源),进度按文件数。
  const runCopyFolder = async (srcRoot: string, dstRoot: string, name: string) => {
    const fid = `folder:copy:${srcRoot}`;
    setTransfers((prev) => ({
      ...prev,
      [fid]: {
        id: fid,
        kind: "复制文件夹",
        name,
        account: current ?? "",
        remote: dstRoot,
        local: `${srcRoot} → ${dstRoot}`,
        done: 0,
        total: 0,
        status: "active",
      },
    }));
    try {
      await api.copyFolder(current!, srcRoot, dstRoot, fid);
      setTransfers((prev) =>
        prev[fid]
          ? { ...prev, [fid]: { ...prev[fid], status: "done", done: prev[fid].total } }
          : prev,
      );
      await load();
    } catch (e) {
      const msg = String(e);
      const cancelled = msg === "已取消";
      setTransfers((prev) =>
        prev[fid]
          ? { ...prev, [fid]: { ...prev[fid], status: cancelled ? "cancelled" : "error" } }
          : prev,
      );
      if (!cancelled) setError(msg);
    }
  };

  // 文件夹迁移:逐文件流式中转,进度按文件数(folder-progress)。可被面板重试复用。
  const runMigrateFolder = async (
    srcAccount: string,
    srcRoot: string,
    dstAccount: string,
    dstDir: string,
    name: string,
  ) => {
    const fid = `folder:migrate:${srcRoot}`;
    setTransfers((prev) => ({
      ...prev,
      [fid]: {
        id: fid,
        kind: "迁移文件夹",
        name,
        account: dstAccount,
        remote: dstDir,
        local: `${srcAccount} → ${dstAccount}`,
        srcAccount,
        srcPath: srcRoot,
        done: 0,
        total: 0,
        status: "active",
      },
    }));
    try {
      await api.migrateFolder(srcAccount, srcRoot, dstAccount, dstDir, fid);
      setTransfers((prev) =>
        prev[fid]
          ? { ...prev, [fid]: { ...prev[fid], status: "done", done: prev[fid].total } }
          : prev,
      );
      if (dstAccount === current) await load();
    } catch (e) {
      const msg = String(e);
      const cancelled = msg === "已取消";
      setTransfers((prev) =>
        prev[fid]
          ? { ...prev, [fid]: { ...prev[fid], status: cancelled ? "cancelled" : "error" } }
          : prev,
      );
      if (!cancelled) setError(msg);
    }
  };

  // 单对象迁移:任务 id 与后端 transfer-progress 的 `to` 对齐,用于实时进度。可被面板重试复用。
  const runMigrateSingle = async (
    srcAccount: string,
    srcPath: string,
    size: number,
    dstAccount: string,
    dstPath: string,
    name: string,
  ) => {
    const id = `migrate:${dstPath}`;
    setTransfers((prev) => ({
      ...prev,
      [id]: {
        id,
        kind: "迁移",
        name,
        account: dstAccount,
        remote: dstPath,
        local: `${srcAccount} → ${dstAccount}`,
        srcAccount,
        srcPath,
        done: 0,
        total: size,
        status: "active",
      },
    }));
    try {
      await api.copyAcross(srcAccount, srcPath, dstAccount, dstPath, id);
      setTransfers((prev) =>
        prev[id]
          ? { ...prev, [id]: { ...prev[id], status: "done", done: prev[id].total } }
          : prev,
      );
      // 目标恰为当前视图时刷新以显示新对象。
      if (dstAccount === current) await load();
    } catch (e) {
      const msg = String(e);
      const cancelled = msg === "已取消";
      setTransfers((prev) =>
        prev[id]
          ? { ...prev, [id]: { ...prev[id], status: cancelled ? "cancelled" : "error" } }
          : prev,
      );
      if (!cancelled) setError(msg);
    }
  };

  const doMigrate = async (dstAccount: string, dstPath: string) => {
    const entry = migrateTarget;
    setMigrateTarget(null);
    if (!current || !entry) return;
    if (entry.kind === "directory") {
      await runMigrateFolder(current, entry.path, dstAccount, dstPath, entry.name);
    } else {
      await runMigrateSingle(
        current,
        entry.path,
        entry.size,
        dstAccount,
        dstPath,
        baseName(dstPath),
      );
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
      setShareUrl({ url, upload: false });
    } catch (e) {
      setError(String(e));
    }
  };

  // 设为公开读 / 私有(预置 ACL)。
  const setAcl = async (entry: Entry, isPublic: boolean) => {
    if (!current) return;
    setError(null);
    try {
      await api.setObjectAcl(current, entry.path, isPublic);
      setNotice({
        tone: "ok",
        text: isPublic
          ? t("✓ {name} 已设为公开读", { name: entry.name })
          : t("✓ {name} 已设为私有", { name: entry.name }),
      });
    } catch (e) {
      setError(String(e));
    }
  };

  // 复制对象的永久公共直链(需对象为公开读)。
  const copyPublicUrl = async (entry: Entry) => {
    if (!current) return;
    setError(null);
    try {
      const url = await api.publicUrl(current, entry.path);
      if (!url) {
        setError(t("该云暂不支持公共直链"));
        return;
      }
      await navigator.clipboard.writeText(url);
      setNotice({ tone: "ok", text: t("✓ 已复制公共链接") });
    } catch (e) {
      setError(String(e));
    }
  };

  const shareUpload = async (entry: Entry) => {
    if (!current) return;
    setError(null);
    try {
      const url = await api.presignPut(
        current,
        entry.path,
        settings.share_expiry_secs,
      );
      setShareUrl({ url, upload: true });
    } catch (e) {
      setError(String(e));
    }
  };

  const verifyEntry = async (entry: Entry) => {
    if (!current) return;
    setError(null);
    setNotice({ tone: "info", text: `正在校验 ${entry.name}…` });
    try {
      const r = await api.verifyObject(current, entry.path);
      if (r.status === "verified") {
        setNotice({ tone: "ok", text: `✓ ${entry.name} 完整性校验通过` });
      } else if (r.status === "mismatch") {
        setNotice({
          tone: "err",
          text: `✗ ${entry.name} 校验失败:内容与远端 ETag 不一致,文件可能已损坏`,
        });
      } else {
        setNotice({ tone: "warn", text: `${entry.name} 无法校验:${r.reason}` });
      }
    } catch (e) {
      setNotice(null);
      setError(String(e));
    }
  };

  // 文件夹级批量操作:创建一个传输面板任务并跑,进度经 folder-progress 汇入。
  const runFolderOp = async (
    id: string,
    kind: TransferItem["kind"],
    name: string,
    op: () => Promise<void>,
  ) => {
    setTransfers((prev) => ({
      ...prev,
      [id]: {
        id,
        kind,
        name,
        account: current ?? "",
        remote: name,
        local: "",
        done: 0,
        total: 0,
        status: "active",
      },
    }));
    try {
      await op();
      setTransfers((prev) =>
        prev[id]
          ? { ...prev, [id]: { ...prev[id], status: "done", done: prev[id].total } }
          : prev,
      );
      await load();
    } catch (e) {
      setError(String(e));
      setTransfers((prev) =>
        prev[id] ? { ...prev, [id]: { ...prev[id], status: "error" } } : prev,
      );
    }
  };

  // 打开详情:先用列表数据即时显示,再 stat 补上真实 Content-Type(列表不返回)。
  const openDetails = async (entry: Entry) => {
    setDetailsEntry(entry);
    if (!current || entry.kind !== "file") return;
    try {
      const full = await api.statPath(current, entry.path);
      setDetailsEntry((cur) =>
        cur && cur.path === entry.path
          ? { ...cur, content_type: full.content_type }
          : cur,
      );
    } catch {
      /* stat 失败就保留列表数据 */
    }
  };

  const doSetContentType = async (contentType: string) => {
    const entry = editTypeTarget;
    setEditTypeTarget(null);
    if (!current || !entry) return;
    setError(null);
    setNotice({ tone: "info", text: `正在修改 ${entry.name} 的类型…` });
    try {
      await api.setContentType(current, entry.path, contentType);
      setDetailsEntry((cur) =>
        cur && cur.path === entry.path
          ? { ...cur, content_type: contentType }
          : cur,
      );
      setNotice({ tone: "ok", text: `✓ ${entry.name} 类型已改为 ${contentType}` });
    } catch (e) {
      setNotice(null);
      setError(String(e));
    }
  };

  const showFolderStats = async (entry: Entry) => {
    if (!current) return;
    setError(null);
    setStatsTarget(entry); // 先开弹窗显示"统计中…"
    setStatsData(null);
    try {
      const s = await api.storageBreakdown(current, entry.path);
      setStatsData(s);
    } catch (e) {
      setStatsTarget(null);
      setError(String(e));
    }
  };

  const doSetStorageClass = async (storageClass: string) => {
    const entry = storageClassTarget;
    setStorageClassTarget(null);
    if (!current || !entry) return;
    setError(null);
    if (entry.kind === "directory") {
      await runFolderOp(
        `folder:storage-class:${entry.path}`,
        "转换存储类型",
        entry.name,
        () => api.setStorageClassFolder(current, entry.path, storageClass),
      );
      return;
    }
    setNotice({ tone: "info", text: `正在转换 ${entry.name} 的存储类型…` });
    try {
      await api.setStorageClass(current, entry.path, storageClass);
      setNotice({ tone: "ok", text: `✓ ${entry.name} 已转为 ${storageClass}` });
      await load();
    } catch (e) {
      setNotice(null);
      setError(String(e));
    }
  };

  const doRestore = async (days: number) => {
    const entry = restoreTarget;
    setRestoreTarget(null);
    if (!current || !entry) return;
    setError(null);
    if (entry.kind === "directory") {
      await runFolderOp(
        `folder:restore:${entry.path}`,
        "取回归档",
        entry.name,
        () => api.restoreFolder(current, entry.path, days),
      );
      return;
    }
    setNotice({ tone: "info", text: `正在发起取回 ${entry.name}…` });
    try {
      await api.restoreObject(current, entry.path, days);
      setNotice({
        tone: "ok",
        text: `已发起取回 ${entry.name},解冻完成后即可下载`,
      });
    } catch (e) {
      setNotice(null);
      setError(String(e));
    }
  };

  const createBucket = async (name: string) => {
    setShowNewBucket(false);
    if (!current || !name.trim()) return;
    setBusy(true);
    setError(null);
    try {
      await api.createBucket(current, name.trim());
      await load();
      setNotice({ tone: "ok", text: `✓ 已新建 Bucket ${name.trim()}` });
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

  const anyModalOpen =
    showForm ||
    !!pendingDelete ||
    !!renameTarget ||
    !!moveCopyTarget ||
    !!migrateTarget ||
    !!storageClassTarget ||
    !!restoreTarget ||
    batchStorageClass ||
    batchRestore ||
    showBatchRename ||
    showBatchTags ||
    showBatchMove ||
    showNewText ||
    !!shareUrl ||
    showNewFolder ||
    showNewBucket ||
    !!editTypeTarget ||
    !!tagsTarget ||
    !!statsTarget ||
    !!cleanupTarget ||
    pendingBatchDelete ||
    showPalette ||
    showSettings;

  onKeyRef.current = (e: KeyboardEvent) => {
    const el = e.target as HTMLElement | null;
    const typing =
      !!el && (el.tagName === "INPUT" || el.tagName === "TEXTAREA");

    const keys = resolveBindings(shortcuts);
    // 命令面板全局开关(即使正在输入框里也可唤起)。
    if (matchBinding(e, keys.palette)) {
      e.preventDefault();
      setShowPalette((v) => !v);
      return;
    }

    if (e.key === "Escape") {
      if (showSettings) setShowSettings(false);
      else if (shareUrl) setShareUrl(null);
      else if (moveCopyTarget) setMoveCopyTarget(null);
      else if (migrateTarget) setMigrateTarget(null);
      else if (storageClassTarget) setStorageClassTarget(null);
      else if (restoreTarget) setRestoreTarget(null);
      else if (batchStorageClass) setBatchStorageClass(false);
      else if (batchRestore) setBatchRestore(false);
      else if (showBatchRename) setShowBatchRename(false);
      else if (showBatchTags) setShowBatchTags(false);
      else if (showBatchMove) setShowBatchMove(false);
      else if (showNewText) setShowNewText(false);
      else if (renameTarget) setRenameTarget(null);
      else if (showNewFolder) setShowNewFolder(false);
      else if (showNewBucket) setShowNewBucket(false);
      else if (editTypeTarget) setEditTypeTarget(null);
      else if (tagsTarget) setTagsTarget(null);
      else if (statsTarget) {
        setStatsTarget(null);
        setStatsData(null);
      } else if (cleanupTarget) setCleanupTarget(null);
      else if (showForm) setShowForm(false);
      else if (pendingBatchDelete) setPendingBatchDelete(false);
      else if (pendingDelete) setPendingDelete(null);
      else if (detailsEntry) setDetailsEntry(null);
      return;
    }

    if (typing || anyModalOpen || !current) return;

    // 导航历史:Alt/⌘ + ← / →(前进后退)。
    if ((e.altKey || e.metaKey) && e.key === "ArrowLeft") {
      e.preventDefault();
      goHistory(-1);
      return;
    }
    if ((e.altKey || e.metaKey) && e.key === "ArrowRight") {
      e.preventDefault();
      goHistory(1);
      return;
    }

    if (matchBinding(e, keys.selectAll)) {
      e.preventDefault();
      setSelected(new Set(visibleFiles.map((f) => f.path)));
    } else if (matchBinding(e, keys.deleteSelected) && selected.size > 0) {
      e.preventDefault();
      setPendingBatchDelete(true);
    } else if (matchBinding(e, keys.parent) && path !== "") {
      e.preventDefault();
      setPath(parentPath(path));
    } else if (matchBinding(e, keys.refresh)) {
      e.preventDefault();
      void load();
    } else if (matchBinding(e, keys.upload) && path !== "") {
      e.preventDefault();
      void upload();
    } else if (matchBinding(e, keys.newFolder) && path !== "") {
      e.preventDefault();
      setShowNewFolder(true);
    } else if (matchBinding(e, keys.sync)) {
      e.preventDefault();
      setSyncPrefix(path);
    }
  };

  // 命令面板可执行的动作,按当前上下文启用(复用工具栏 / 菜单的现有处理函数)。
  const paletteCommands: PaletteCommand[] = [];
  if (current)
    paletteCommands.push({ id: "refresh", label: t("刷新"), run: () => void load() });
  if (path !== "") {
    paletteCommands.push({
      id: "up",
      label: t("上一层"),
      run: () => setPath(parentPath(path)),
    });
    paletteCommands.push({ id: "upload", label: t("上传"), run: () => void upload() });
    paletteCommands.push({
      id: "upload-folder",
      label: t("上传文件夹"),
      run: () => void uploadFolder(),
    });
    paletteCommands.push({
      id: "new-folder",
      label: t("新建文件夹"),
      run: () => setShowNewFolder(true),
    });
    paletteCommands.push({
      id: "new-text",
      label: t("新建文本文件"),
      run: () => setShowNewText(true),
    });
  }
  if (path === "" && current)
    paletteCommands.push({
      id: "new-bucket",
      label: t("新建 Bucket"),
      run: () => setShowNewBucket(true),
    });
  paletteCommands.push({
    id: "toggle-view",
    label: view === "list" ? t("网格视图") : t("列表视图"),
    run: () => setView((v) => (v === "list" ? "grid" : "list")),
  });
  paletteCommands.push({
    id: "toggle-theme",
    label: t("切换主题"),
    run: () => toggleTheme(),
  });
  if (current)
    paletteCommands.push({
      id: "sync",
      label: t("备份 / 同步"),
      run: () => setSyncPrefix(path),
    });
  if (current && path !== "")
    paletteCommands.push({
      id: "duplicates",
      label: t("查找重复文件"),
      run: () => setDupRoot(path),
    });
  if (current && path !== "")
    paletteCommands.push({
      id: "largest",
      label: t("大文件排行"),
      run: () => setLargeRoot(path),
    });
  if (current && path !== "")
    paletteCommands.push({
      id: "export-manifest",
      label: t("导出清单 CSV"),
      run: () => void exportManifest(path),
    });
  paletteCommands.push({
    id: "settings",
    label: t("设置"),
    run: () => setShowSettings(true),
  });
  paletteCommands.push({
    id: "add-account",
    label: t("添加账号"),
    run: () => setShowForm(true),
  });
  paletteCommands.push({ id: "about", label: t("关于"), run: () => setShowAbout(true) });
  paletteCommands.push({
    id: "check-update",
    label: t("检查更新"),
    run: () => void runUpdateCheck(true),
  });

  return (
    <div className="layout">
      <Sidebar
        accounts={accounts}
        current={current}
        theme={theme}
        width={sidebarWidth}
        onSelect={selectAccount}
        onAdd={() => setShowForm(true)}
        onEdit={editAccount}
        onRemove={removeAccount}
        onToggleTheme={toggleTheme}
        onSettings={() => setShowSettings(true)}
      />

      <div
        className="resizer"
        onMouseDown={startResize}
        title="拖拽调整宽度"
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
              <Bookmarks
                bookmarks={bookmarks}
                isBookmarked={isBookmarked}
                canBookmark={!!current}
                onToggle={toggleBookmark}
                onJump={jumpBookmark}
                onRemove={removeBookmark}
              />
              <Toolbar
                canBack={canBack}
                canForward={canForward}
                onBack={() => goHistory(-1)}
                onForward={() => goHistory(1)}
                canGoUp={path !== ""}
                canUpload={path !== ""}
                busy={busy}
                filter={filter}
                view={view}
                canSearch={path !== "" && !!current}
                atRoot={path === "" && !!current}
                onNewBucket={() => setShowNewBucket(true)}
                onFilter={setFilter}
                onSearch={(q) => runSearch(q, searchFilter, contentSearch)}
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

            {notice && (
              <div className={`notice-banner notice-banner--${notice.tone}`}>
                {notice.text}
              </div>
            )}

            {selected.size > 0 && (
              <div className="batch-bar">
                <span className="batch-bar__count">
                  {t("已选 {n} 项", { n: selected.size })}
                </span>
                <div className="batch-bar__spacer" />
                <button className="btn" onClick={batchDownload}>
                  {t("批量下载")}
                </button>
                <button className="btn" onClick={() => setBatchStorageClass(true)}>
                  {t("转换存储类型")}
                </button>
                <button className="btn" onClick={() => setBatchRestore(true)}>
                  {t("取回归档")}
                </button>
                <button className="btn" onClick={() => setShowBatchRename(true)}>
                  {t("批量重命名")}
                </button>
                <button className="btn" onClick={() => setShowBatchMove(true)}>
                  {t("批量移动 / 复制")}
                </button>
                <button className="btn" onClick={() => setShowBatchTags(true)}>
                  {t("批量标签")}
                </button>
                <button className="btn" onClick={() => batchAcl(true)}>
                  {t("批量公开")}
                </button>
                <button className="btn" onClick={() => batchAcl(false)}>
                  {t("批量私有")}
                </button>
                <button
                  className="btn btn--danger"
                  onClick={() => setPendingBatchDelete(true)}
                >
                  {t("批量删除")}
                </button>
                <button className="btn" onClick={clearSelection}>
                  {t("取消选择")}
                </button>
              </div>
            )}

            {search ? (
              <SearchResults
                query={search.query}
                root={path}
                results={search.results}
                loading={search.loading}
                truncated={search.truncated}
                minSize={searchFilter.minSize}
                ext={searchFilter.ext}
                selected={selected}
                onFilter={applySearchFilter}
                onOpen={openSearchResult}
                onToggleSelect={toggleSelect}
                onToggleSelectAll={toggleSelectAllSearch}
                snippets={search.snippets}
                contentMode={contentSearch}
                onToggleContent={toggleContentSearch}
                onClear={() => {
                  setSearch(null);
                  clearSelection();
                }}
              />
            ) : view === "list" ? (
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
                onDragStartFile={onDragStartFile}
                onDropDir={onDropDir}
                onReachEnd={loadMore}
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
                onDragStartFile={onDragStartFile}
                onDropDir={onDropDir}
                onReachEnd={loadMore}
              />
            )}

            {detailsEntry && (
              <FileDetails
                entry={detailsEntry}
                onClose={() => setDetailsEntry(null)}
                onDownload={download}
                onShare={share}
                onEditType={setEditTypeTarget}
                onEditTags={setTagsTarget}
              />
            )}

            {!loading && (
              <div className="statusbar">
                {t("{n} 个目录", { n: dirCount })} ·{" "}
                {t("{n} 个文件", { n: visibleFiles.length })} ·{" "}
                {t("共 {size}", { size: formatBytes(totalSize) })}
                {filter && t(" · 已过滤")}
                {selected.size > 0 && t(" · 已选 {n}", { n: selected.size })}
              </div>
            )}

            {Object.keys(transfers).length > 0 && (
              <TransferPanel
                items={Object.values(transfers)}
                onClear={clearTransfers}
                onRetry={retryTransfer}
                onCancel={cancelTransfer}
              />
            )}
          </>
        ) : (
          <div className="empty-state">
            <Logo size={72} />
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
                  vendor: editInfo.vendor,
                  accessKeyId: editInfo.access_key_id,
                  endpoint: editInfo.endpoint,
                  customDomain: editInfo.custom_domain,
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
          title={t("删除确认")}
          message={
            isBucket(pendingDelete)
              ? t("确定删除 Bucket {name}?Bucket 需为空,此操作不可恢复。", {
                  name: pendingDelete.name,
                })
              : pendingDelete.kind === "directory"
                ? t(
                    "确定删除整个文件夹 {name}?其下所有对象都会被递归删除,此操作不可恢复。",
                    { name: pendingDelete.name },
                  )
                : t("确定删除 {name}?此操作不可恢复。", {
                    name: pendingDelete.name,
                  })
          }
          danger
          confirmLabel={t("删除")}
          onConfirm={doDelete}
          onCancel={() => setPendingDelete(null)}
        />
      )}

      {showNewFolder && (
        <PromptDialog
          title={t("新建文件夹")}
          placeholder={t("文件夹名称")}
          submitLabel={t("创建")}
          onSubmit={createFolder}
          onCancel={() => setShowNewFolder(false)}
        />
      )}

      {showNewBucket && (
        <PromptDialog
          title={t("新建 Bucket")}
          placeholder={t("Bucket 名称(全局唯一,小写字母 / 数字 / 连字符)")}
          submitLabel={t("创建")}
          onSubmit={createBucket}
          onCancel={() => setShowNewBucket(false)}
        />
      )}

      {editTypeTarget && (
        <PromptDialog
          title={t("修改内容类型")}
          placeholder={t("如 image/png、application/pdf")}
          initial={editTypeTarget.content_type ?? ""}
          submitLabel={t("保存")}
          onSubmit={doSetContentType}
          onCancel={() => setEditTypeTarget(null)}
        />
      )}

      {tagsTarget && current && (
        <TagsDialog
          account={current}
          path={tagsTarget.path}
          name={tagsTarget.name}
          onSaved={(tags) => {
            setTagsTarget(null);
            setNotice({
              tone: "ok",
              text: t("✓ {name} 标签已保存({n})", {
                name: tagsTarget.name,
                n: tags.length,
              }),
            });
          }}
          onCancel={() => setTagsTarget(null)}
          onError={(msg) => {
            setTagsTarget(null);
            setError(msg);
          }}
        />
      )}

      {showBatchTags && current && (
        <BatchTagsDialog
          account={current}
          paths={[...selected]}
          onClose={() => setShowBatchTags(false)}
          onDone={() => {
            setShowBatchTags(false);
            setNotice({
              tone: "ok",
              text: t("✓ 已给 {n} 项打标签", { n: String(selected.size) }),
            });
          }}
        />
      )}

      {showBatchMove && current && (
        <MoveCopyDialog
          account={current}
          from=""
          batchCount={selected.size}
          onCopy={(dir) => void batchMoveCopy(dir, false)}
          onMove={(dir) => void batchMoveCopy(dir, true)}
          onCancel={() => setShowBatchMove(false)}
        />
      )}

      {showNewText && current && (
        <NewTextFileDialog
          account={current}
          dir={path}
          onClose={() => setShowNewText(false)}
          onCreated={() => {
            setNotice({ tone: "ok", text: t("✓ 已创建文本文件") });
            void load();
          }}
        />
      )}

      {statsTarget && (
        <StatsDialog
          name={statsTarget.name}
          data={statsData}
          onClose={() => {
            setStatsTarget(null);
            setStatsData(null);
          }}
        />
      )}

      {cleanupTarget && current && (
        <CleanupDialog
          account={current}
          bucket={cleanupTarget.path.replace(/\/+$/, "")}
          onClose={() => setCleanupTarget(null)}
          onCleaned={(n) => {
            setCleanupTarget(null);
            setNotice({
              tone: "ok",
              text:
                n > 0
                  ? t("✓ 已清理 {n} 个残留分片上传", { n })
                  : t("没有残留的分片上传,一切干净。"),
            });
          }}
          onError={(msg) => {
            setCleanupTarget(null);
            setError(msg);
          }}
        />
      )}

      {renameTarget && (
        <PromptDialog
          title={t("重命名")}
          placeholder={t("新名称")}
          initial={renameTarget.name}
          submitLabel={t("重命名")}
          onSubmit={doRename}
          onCancel={() => setRenameTarget(null)}
        />
      )}

      {pendingBatchDelete && (
        <ConfirmDialog
          title={t("批量删除")}
          message={t("确定删除选中的 {n} 项?此操作不可恢复。", {
            n: selected.size,
          })}
          danger
          confirmLabel={t("删除")}
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

      {migrateTarget && current && (
        <MigrateDialog
          accounts={accounts}
          srcAccount={current}
          from={migrateTarget.path}
          isFolder={migrateTarget.kind === "directory"}
          onConfirm={doMigrate}
          onCancel={() => setMigrateTarget(null)}
        />
      )}

      {storageClassTarget && current && (
        <StorageClassDialog
          vendor={accounts.find((a) => a.id === current)?.vendor ?? ""}
          name={storageClassTarget.name}
          current={storageClassTarget.storage_class}
          onConfirm={doSetStorageClass}
          onCancel={() => setStorageClassTarget(null)}
        />
      )}

      {restoreTarget && (
        <RestoreDialog
          name={restoreTarget.name}
          onConfirm={doRestore}
          onCancel={() => setRestoreTarget(null)}
        />
      )}

      {batchStorageClass && current && (
        <StorageClassDialog
          vendor={accounts.find((a) => a.id === current)?.vendor ?? ""}
          name={`已选 ${selected.size} 项`}
          current={null}
          onConfirm={doBatchStorageClass}
          onCancel={() => setBatchStorageClass(false)}
        />
      )}

      {batchRestore && (
        <RestoreDialog
          name={`已选 ${selected.size} 项`}
          onConfirm={doBatchRestore}
          onCancel={() => setBatchRestore(false)}
        />
      )}

      {showBatchRename && (
        <BatchRenameDialog
          paths={[...selected]}
          onApply={applyBatchRename}
          onCancel={() => setShowBatchRename(false)}
        />
      )}

      {shareUrl && (
        <ShareDialog
          url={shareUrl.url}
          upload={shareUrl.upload}
          minutes={Math.round(settings.share_expiry_secs / 60)}
          onClose={() => setShareUrl(null)}
        />
      )}

      {showSettings && (
        <SettingsDialog
          settings={settings}
          shortcuts={shortcuts}
          onShortcutsChange={setShortcuts}
          themePref={themePref}
          onThemeChange={setThemePref}
          onSave={saveSettings}
          onClose={() => setShowSettings(false)}
        />
      )}

      {syncPrefix !== null && (
        <SyncDialog
          accounts={accounts}
          defaultAccount={current}
          defaultPrefix={syncPrefix}
          onClose={() => setSyncPrefix(null)}
        />
      )}

      {dupRoot !== null && current && (
        <DuplicatesDialog
          account={current}
          root={dupRoot}
          onClose={() => setDupRoot(null)}
          onDeleted={() => void load()}
        />
      )}

      {largeRoot !== null && current && (
        <LargestFilesDialog
          account={current}
          root={largeRoot}
          onClose={() => setLargeRoot(null)}
          onOpen={(p) => {
            setLargeRoot(null);
            setPath(parentPath(p));
          }}
        />
      )}

      {showAbout && <AboutDialog onClose={() => setShowAbout(false)} />}

      {showPalette && (
        <CommandPalette
          accounts={accounts}
          bookmarks={bookmarks}
          recents={recents}
          commands={paletteCommands}
          onJump={jumpBookmark}
          onClose={() => setShowPalette(false)}
        />
      )}

      {update && (
        <UpdateDialog update={update} onClose={() => setUpdate(null)} />
      )}

      {updateFlash && <div className="update-flash">{updateFlash}</div>}

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
          text={preview.text}
          truncated={preview.truncated}
          name={preview.name}
          kind={preview.kind}
          onClose={() => setPreview(null)}
        />
      )}

    </div>
  );

  function contextItems(entry: Entry): MenuItem[] {
    // Bucket 不是普通文件夹:打开,或删除 Bucket(需为空);不做整桶递归操作(危险)。
    if (isBucket(entry)) {
      return [
        { label: t("打开"), onClick: () => openDir(entry) },
        { label: t("统计信息"), onClick: () => showFolderStats(entry) },
        { label: t("备份 / 同步"), onClick: () => setSyncPrefix(entry.path) },
        { label: t("清理未完成上传"), onClick: () => setCleanupTarget(entry) },
        {
          label: t("删除 Bucket"),
          danger: true,
          onClick: () => setPendingDelete(entry),
        },
      ];
    }
    if (entry.kind === "directory") {
      const dirItems: MenuItem[] = [
        { label: t("打开"), onClick: () => openDir(entry) },
        { label: t("统计信息"), onClick: () => showFolderStats(entry) },
        { label: t("下载文件夹"), onClick: () => downloadFolderEntry(entry) },
        { label: t("备份 / 同步"), onClick: () => setSyncPrefix(entry.path) },
        { label: t("查找重复文件"), onClick: () => setDupRoot(entry.path) },
        { label: t("大文件排行"), onClick: () => setLargeRoot(entry.path) },
        { label: t("导出清单 CSV"), onClick: () => void exportManifest(entry.path) },
        { label: t("重命名"), onClick: () => setRenameTarget(entry) },
        { label: t("复制 / 移动到"), onClick: () => setMoveCopyTarget(entry) },
      ];
      if (accounts.length > 1) {
        dirItems.push({
          label: t("迁移到其他账号"),
          onClick: () => setMigrateTarget(entry),
        });
      }
      dirItems.push(
        { label: t("转换存储类型"), onClick: () => setStorageClassTarget(entry) },
        { label: t("取回归档"), onClick: () => setRestoreTarget(entry) },
      );
      dirItems.push({
        label: t("删除文件夹"),
        danger: true,
        onClick: () => setPendingDelete(entry),
      });
      return dirItems;
    }
    const items: MenuItem[] = [];
    if (previewKind(entry.name)) {
      items.push({ label: t("预览"), onClick: () => openPreview(entry) });
    }
    items.push(
      { label: t("详情"), onClick: () => openDetails(entry) },
      { label: t("下载"), onClick: () => download(entry) },
      { label: t("校验完整性"), onClick: () => verifyEntry(entry) },
      { label: t("转换存储类型"), onClick: () => setStorageClassTarget(entry) },
      { label: t("取回归档"), onClick: () => setRestoreTarget(entry) },
      { label: t("重命名"), onClick: () => setRenameTarget(entry) },
      { label: t("复制 / 移动到"), onClick: () => setMoveCopyTarget(entry) },
    );
    if (accounts.length > 1) {
      items.push({
        label: t("迁移到其他账号"),
        onClick: () => setMigrateTarget(entry),
      });
    }
    items.push(
      { label: t("分享链接"), onClick: () => share(entry) },
      { label: t("上传链接"), onClick: () => shareUpload(entry) },
      { label: t("复制公共链接"), onClick: () => copyPublicUrl(entry) },
      { label: t("设为公开读"), onClick: () => setAcl(entry, true) },
      { label: t("设为私有"), onClick: () => setAcl(entry, false) },
      { label: t("删除"), danger: true, onClick: () => setPendingDelete(entry) },
    );
    return items;
  }
}
