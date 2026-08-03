import { useCallback, useEffect, useRef, useState } from "react";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import {
  faXmark,
  faSave,
  faSpinner,
  faTrash,
  faRotateLeft,
  faRotateRight,
  faObjectGroup,
  faFileExport,
  faCheckDouble,
  faChevronLeft,
  faChevronRight,
  faMagnifyingGlassPlus,
  faMagnifyingGlassMinus,
  faListOl,
  faStamp,
  faFileLines,
  faCopy,
  faDownload,
  faCompress,
} from "@fortawesome/free-solid-svg-icons";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { save as saveDialog, open as openDialog } from "@tauri-apps/plugin-dialog";
import * as pdfjs from "pdfjs-dist";
import workerUrl from "pdfjs-dist/build/pdf.worker.min.mjs?url";
import * as api from "../api";
import { useI18n } from "../i18n";
import { formatBytes } from "../util";
import { Select } from "./Select";
import { Checkbox } from "./Checkbox";
import { Tooltip } from "./Tooltip";
import type {
  PdfAssembly,
  PdfNumberPos,
  PdfPageNumbers,
  PdfWatermark,
} from "../types";

pdfjs.GlobalWorkerOptions.workerSrc = workerUrl;

interface Props {
  account: string;
  path: string;
  name: string;
}

/** 工作集中的一页:来自第几个源文档、该文档第几页(0 起)、绝对旋转角、稳定 key。 */
interface PageItem {
  id: string;
  doc: number;
  page: number;
  rotate: number;
}

function baseName(p: string): string {
  const i = p.lastIndexOf("/");
  return i >= 0 ? p.slice(i + 1) : p;
}

/** 另存路径:扩展名前加 -edited。 */
function editedPath(p: string): string {
  const slash = p.lastIndexOf("/");
  const dir = slash >= 0 ? p.slice(0, slash + 1) : "";
  const base = slash >= 0 ? p.slice(slash + 1) : p;
  const dot = base.lastIndexOf(".");
  const stem = dot > 0 ? base.slice(0, dot) : base;
  return `${dir}${stem}-edited.pdf`;
}

/**
 * 独立窗口里的 PDF 页面编辑器:删除 / 重排(拖拽)/ 旋转 / 合并 / 提取。
 * 解析与组装在 Rust(lopdf)侧,前端用 pdf.js 渲染缩略图与交互。
 */
export function PdfWindow({ account, path, name }: Props) {
  const { t } = useI18n();
  const [pages, setPages] = useState<PageItem[]>([]);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [thumbs, setThumbs] = useState<Map<string, string>>(new Map());
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [saveMenu, setSaveMenu] = useState(false);
  const [toast, setToast] = useState<string | null>(null);
  const [dirty, setDirty] = useState(false);
  // 阅读视图:reader 为正在阅读的页在 pages 里的下标(null = 缩略图管理视图)。
  const [reader, setReader] = useState<number | null>(null);
  const [zoom, setZoom] = useState(1);
  const [readerBusy, setReaderBusy] = useState(false);
  // 加页码对话框。
  const [numOpen, setNumOpen] = useState(false);
  const [numPos, setNumPos] = useState<PdfNumberPos>("bottom_center");
  const [numStart, setNumStart] = useState(1);
  const [numFormat, setNumFormat] = useState("{n}");
  const [numSize, setNumSize] = useState(12);
  // 水印对话框。
  const [wmOpen, setWmOpen] = useState(false);
  const [wmText, setWmText] = useState("");
  const [wmOpacity, setWmOpacity] = useState(20);
  const [wmAngle, setWmAngle] = useState(45);
  const [wmSize, setWmSize] = useState(48);
  const [wmTile, setWmTile] = useState(true);
  // 文本提取面板。
  const [textPages, setTextPages] = useState<string[] | null>(null);
  const [textBusy, setTextBusy] = useState(false);

  // 源文档:0 = 主文档(云端 path),1.. = 合并进来的本地 PDF 字节。
  const docs = useRef<Uint8Array[]>([]);
  // pdf.js 文档句柄,与 docs 索引一一对应(渲染缩略图用)。
  const pdfDocs = useRef<pdfjs.PDFDocumentProxy[]>([]);
  const dragFrom = useRef<number | null>(null);
  const readerCanvasRef = useRef<HTMLCanvasElement>(null);

  const close = useCallback(() => void getCurrentWindow().close(), []);

  useEffect(() => {
    document.title = name;
  }, [name]);

  // 跟随应用主题。
  useEffect(() => {
    api
      .getPref("theme")
      .then((th) => {
        const theme =
          th === "light"
            ? "light"
            : th === "dark"
              ? "dark"
              : window.matchMedia("(prefers-color-scheme: dark)").matches
                ? "dark"
                : "light";
        document.documentElement.setAttribute("data-theme", theme);
        void getCurrentWindow().setTheme(theme);
      })
      .catch(() => {});
  }, []);

  // 载入主文档:取字节 → pdf.js 打开 → 初始化页面工作集(带各页原有旋转)。
  useEffect(() => {
    let alive = true;
    (async () => {
      try {
        const [bytes, info] = await Promise.all([
          api.pdfBytes(account, path),
          api.pdfInfo(account, path),
        ]);
        if (!alive) return;
        const u8 = new Uint8Array(bytes);
        docs.current = [u8];
        const pdf = await pdfjs.getDocument({ data: u8.slice() }).promise;
        if (!alive) return;
        pdfDocs.current = [pdf];
        setPages(
          info.pages.map((p, i) => ({
            id: `0:${i}`,
            doc: 0,
            page: i,
            rotate: ((p.rotate % 360) + 360) % 360,
          })),
        );
        setLoading(false);
      } catch (e) {
        if (alive) {
          setError(String(e));
          setLoading(false);
        }
      }
    })();
    return () => {
      alive = false;
    };
  }, [account, path]);

  // 缺哪页缩略图就渲染哪页(无旋转底图,旋转用 CSS 施加,和后端绝对角一致)。
  useEffect(() => {
    let alive = true;
    (async () => {
      for (const it of pages) {
        const key = `${it.doc}:${it.page}`;
        if (thumbs.has(key)) continue;
        const pdf = pdfDocs.current[it.doc];
        if (!pdf) continue;
        try {
          const page = await pdf.getPage(it.page + 1);
          const viewport = page.getViewport({ scale: 0.4, rotation: 0 });
          const canvas = document.createElement("canvas");
          canvas.width = Math.ceil(viewport.width);
          canvas.height = Math.ceil(viewport.height);
          const ctx = canvas.getContext("2d");
          if (!ctx) continue;
          await page.render({ canvas, canvasContext: ctx, viewport }).promise;
          if (!alive) return;
          const url = canvas.toDataURL();
          setThumbs((m) => {
            const n = new Map(m);
            n.set(key, url);
            return n;
          });
        } catch {
          /* 单页渲染失败不阻断其它页 */
        }
      }
    })();
    return () => {
      alive = false;
    };
  }, [pages, thumbs]);

  useEffect(() => {
    if (!toast) return;
    const id = setTimeout(() => setToast(null), 2600);
    return () => clearTimeout(id);
  }, [toast]);

  // 阅读视图:把当前页按 zoom 高清渲染到画布(旋转用 pdf.js viewport,文字清晰)。
  useEffect(() => {
    if (reader === null) return;
    const item = pages[reader];
    if (!item) return;
    let alive = true;
    setReaderBusy(true);
    (async () => {
      try {
        const pdf = pdfDocs.current[item.doc];
        if (!pdf) return;
        const page = await pdf.getPage(item.page + 1);
        const base = page.getViewport({ scale: 1, rotation: item.rotate });
        // 适配窗口宽度(减去边距)后再乘用户 zoom;限个上限防超大页。
        const avail = Math.min(window.innerWidth - 80, 1400);
        const fit = Math.min(2.5, Math.max(0.4, avail / base.width));
        const dpr = window.devicePixelRatio || 1;
        const viewport = page.getViewport({
          scale: fit * zoom * dpr,
          rotation: item.rotate,
        });
        const canvas = readerCanvasRef.current;
        if (!canvas || !alive) return;
        canvas.width = Math.ceil(viewport.width);
        canvas.height = Math.ceil(viewport.height);
        canvas.style.width = `${Math.ceil(viewport.width / dpr)}px`;
        canvas.style.height = `${Math.ceil(viewport.height / dpr)}px`;
        const ctx = canvas.getContext("2d");
        if (!ctx) return;
        await page.render({ canvas, canvasContext: ctx, viewport }).promise;
      } catch {
        /* 渲染失败忽略 */
      } finally {
        if (alive) setReaderBusy(false);
      }
    })();
    return () => {
      alive = false;
    };
  }, [reader, zoom, pages]);

  const openReader = (index: number) => {
    setZoom(1);
    setReader(index);
  };
  const readerNav = useCallback(
    (delta: number) => {
      setReader((r) => {
        if (r === null) return r;
        const next = r + delta;
        if (next < 0 || next >= pages.length) return r;
        setZoom(1);
        return next;
      });
    },
    [pages.length],
  );

  // 阅读视图键盘:方向键翻页、+/- 缩放、Esc 退出。
  useEffect(() => {
    if (reader === null) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "ArrowRight" || e.key === "ArrowDown" || e.key === "PageDown")
        readerNav(1);
      else if (e.key === "ArrowLeft" || e.key === "ArrowUp" || e.key === "PageUp")
        readerNav(-1);
      else if (e.key === "+" || e.key === "=") setZoom((z) => Math.min(4, z + 0.2));
      else if (e.key === "-") setZoom((z) => Math.max(0.4, z - 0.2));
      else if (e.key === "Escape") setReader(null);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [reader, readerNav]);

  const toggleSelect = (id: string) => {
    setSelected((s) => {
      const n = new Set(s);
      if (n.has(id)) n.delete(id);
      else n.add(id);
      return n;
    });
  };
  const selectAll = () => {
    setSelected((s) =>
      s.size === pages.length ? new Set() : new Set(pages.map((p) => p.id)),
    );
  };

  // 旋转选中页(没选则全部)±90°,绝对角。
  const rotateSelected = (delta: number) => {
    const targets = selected.size ? selected : new Set(pages.map((p) => p.id));
    setPages((ps) =>
      ps.map((p) =>
        targets.has(p.id)
          ? { ...p, rotate: (((p.rotate + delta) % 360) + 360) % 360 }
          : p,
      ),
    );
    setDirty(true);
  };

  const deleteSelected = () => {
    if (!selected.size) return;
    if (selected.size >= pages.length) {
      setToast(t("不能删除全部页面"));
      return;
    }
    setPages((ps) => ps.filter((p) => !selected.has(p.id)));
    setSelected(new Set());
    setDirty(true);
  };

  // 拖拽重排。
  const onDragStart = (i: number) => (dragFrom.current = i);
  const onDrop = (to: number) => {
    const from = dragFrom.current;
    dragFrom.current = null;
    if (from === null || from === to) return;
    setPages((ps) => {
      const n = ps.slice();
      const [moved] = n.splice(from, 1);
      n.splice(to, 0, moved);
      return n;
    });
    setDirty(true);
  };

  // 合并本地 PDF:选文件 → 读字节 → pdf.js 打开 → 把它的页追加到工作集末尾。
  const mergeLocal = async () => {
    const picked = await openDialog({
      multiple: false,
      filters: [{ name: "PDF", extensions: ["pdf"] }],
    });
    if (typeof picked !== "string") return;
    try {
      const bytes = new Uint8Array(await api.readFileBytes(picked));
      const pdf = await pdfjs.getDocument({ data: bytes.slice() }).promise;
      const docIndex = docs.current.length;
      docs.current.push(bytes);
      pdfDocs.current.push(pdf);
      const added: PageItem[] = [];
      for (let i = 0; i < pdf.numPages; i++) {
        const p = await pdf.getPage(i + 1);
        added.push({
          id: `${docIndex}:${i}`,
          doc: docIndex,
          page: i,
          rotate: (((p.rotate ?? 0) % 360) + 360) % 360,
        });
      }
      setPages((ps) => [...ps, ...added]);
      setDirty(true);
      setToast(t("已合并 {n} 页", { n: String(added.length) }));
    } catch (e) {
      setToast(t("合并失败:{msg}", { msg: String(e) }));
    }
  };

  const buildAssembly = (): PdfAssembly => ({
    pages: pages.map((p) => ({ doc: p.doc, page: p.page, rotate: p.rotate })),
  });
  // 合并源(doc 1..)的字节,转成普通数组给 IPC。
  const sourceBytes = () =>
    docs.current.slice(1).map((u8) => Array.from(u8));

  // 提取选中页为一个新对象(按当前顺序;不改动原文档)。
  const extractSelected = async () => {
    if (!selected.size) {
      setToast(t("请先选择要提取的页"));
      return;
    }
    const asm: PdfAssembly = {
      pages: pages
        .filter((p) => selected.has(p.id))
        .map((p) => ({ doc: p.doc, page: p.page, rotate: p.rotate })),
    };
    const dot = path.lastIndexOf(".");
    const dest = `${dot > 0 ? path.slice(0, dot) : path}-extract.pdf`;
    setSaving(true);
    try {
      await api.pdfSave(account, path, sourceBytes(), asm, dest);
      setToast(t("✓ 已提取 {n} 页为 {name}", {
        n: String(selected.size),
        name: baseName(dest),
      }));
    } catch (e) {
      setToast(t("保存失败:{msg}", { msg: String(e) }));
    }
    setSaving(false);
  };

  // 加页码:先组装当前工作集,再叠页码,另存为新对象。
  const applyNumbers = async () => {
    if (!pages.length) return;
    setNumOpen(false);
    setSaving(true);
    const opts: PdfPageNumbers = {
      start: numStart,
      position: numPos,
      size: numSize,
      margin: 24,
      format: numFormat || "{n}",
    };
    const dot = path.lastIndexOf(".");
    const dest = `${dot > 0 ? path.slice(0, dot) : path}-numbered.pdf`;
    try {
      await api.pdfNumber(account, path, sourceBytes(), buildAssembly(), opts, dest);
      setToast(t("✓ 已加页码,另存为 {name}", { name: baseName(dest) }));
    } catch (e) {
      setToast(t("保存失败:{msg}", { msg: String(e) }));
    }
    setSaving(false);
  };

  // 加水印:组装当前工作集 → 叠文字水印 → 另存为新对象。
  const applyWatermark = async () => {
    if (!pages.length || !wmText.trim()) {
      setToast(t("请输入水印文字"));
      return;
    }
    setWmOpen(false);
    setSaving(true);
    const wm: PdfWatermark = {
      text: wmText,
      size: wmSize,
      opacity: wmOpacity / 100,
      angle: wmAngle,
      gray: 128,
      tile: wmTile,
    };
    const dot = path.lastIndexOf(".");
    const dest = `${dot > 0 ? path.slice(0, dot) : path}-watermark.pdf`;
    try {
      await api.pdfWatermark(account, path, sourceBytes(), buildAssembly(), wm, dest);
      setToast(t("✓ 已加水印,另存为 {name}", { name: baseName(dest) }));
    } catch (e) {
      setToast(t("保存失败:{msg}", { msg: String(e) }));
    }
    setSaving(false);
  };

  // 压缩瘦身(作用于云端原文档,另存为新对象)。
  const compress = async () => {
    setSaving(true);
    const dot = path.lastIndexOf(".");
    const dest = `${dot > 0 ? path.slice(0, dot) : path}-compressed.pdf`;
    try {
      const [orig, next] = await api.pdfCompress(account, path, dest);
      const saved = orig - next;
      const pct = orig > 0 ? Math.round((saved / orig) * 100) : 0;
      setToast(
        saved > 0
          ? t("✓ 已压缩:{a} → {b}(省 {p}%)", {
              a: formatBytes(orig),
              b: formatBytes(next),
              p: String(pct),
            })
          : t("已是最优,无可压缩空间"),
      );
    } catch (e) {
      setToast(t("保存失败:{msg}", { msg: String(e) }));
    }
    setSaving(false);
  };

  // 提取文本(作用于云端原文档)。
  const extractText = async () => {
    setTextBusy(true);
    setTextPages([]);
    try {
      const t2 = await api.pdfText(account, path);
      setTextPages(t2);
    } catch (e) {
      setTextPages(null);
      setToast(t("提取文本失败:{msg}", { msg: String(e) }));
    }
    setTextBusy(false);
  };
  const joinedText = () =>
    (textPages ?? [])
      .map((tx, i) => `— ${t("第 {n} 页", { n: String(i + 1) })} —\n${tx}`)
      .join("\n\n");
  const copyText = async () => {
    try {
      await navigator.clipboard.writeText(joinedText());
      setToast(t("✓ 已复制全部文本"));
    } catch {
      setToast(t("复制失败"));
    }
  };
  const exportText = async () => {
    const dot = path.lastIndexOf(".");
    const suggested = `${baseName(dot > 0 ? path.slice(0, dot) : path)}.txt`;
    const dest = await saveDialog({
      defaultPath: suggested,
      filters: [{ name: "Text", extensions: ["txt"] }],
    });
    if (typeof dest !== "string") return;
    try {
      const bytes = Array.from(new TextEncoder().encode(joinedText()));
      await api.saveImageBytesLocal(dest, bytes);
      setToast(t("✓ 已导出到本地"));
    } catch (e) {
      setToast(t("保存失败:{msg}", { msg: String(e) }));
    }
  };

  const save = async (overwrite: boolean) => {
    setSaveMenu(false);
    if (!pages.length) {
      setToast(t("没有页面"));
      return;
    }
    setSaving(true);
    const dest = overwrite ? path : editedPath(path);
    try {
      await api.pdfSave(account, path, sourceBytes(), buildAssembly(), dest);
      setToast(
        overwrite
          ? t("✓ 已覆盖保存")
          : t("✓ 已另存为 {name}", { name: baseName(dest) }),
      );
      setDirty(false);
    } catch (e) {
      setToast(t("保存失败:{msg}", { msg: String(e) }));
    }
    setSaving(false);
  };

  const downloadLocal = async () => {
    setSaveMenu(false);
    if (!pages.length) return;
    const dest = await saveDialog({
      defaultPath: baseName(editedPath(path)),
      filters: [{ name: "PDF", extensions: ["pdf"] }],
    });
    if (typeof dest !== "string") return;
    setSaving(true);
    try {
      await api.pdfDownload(account, path, sourceBytes(), buildAssembly(), dest);
      setToast(t("✓ 已下载到本地"));
    } catch (e) {
      setToast(t("保存失败:{msg}", { msg: String(e) }));
    }
    setSaving(false);
  };

  const allSelected = selected.size === pages.length && pages.length > 0;

  return (
    <div className="pv">
      <div className="pv__bar" data-tauri-drag-region>
        <span className="pv__name" title={name}>
          {name}
          {dirty ? " •" : ""}
        </span>
        <span className="pv__count">{t("{n} 页", { n: String(pages.length) })}</span>
        <div className="pv__spacer" />
        <div className="pv__savewrap">
          <Tooltip label={saving ? t("保存中…") : t("保存")} side="bottom">
            <button
              className="pv__btn pv__btn--primary"
              disabled={saving || !!error}
              onClick={() => setSaveMenu((v) => !v)}
            >
              <FontAwesomeIcon icon={saving ? faSpinner : faSave} spin={saving} />
            </button>
          </Tooltip>
          {saveMenu && (
            <div className="pv__savemenu">
              <button onClick={() => save(false)}>{t("另存为新对象")}</button>
              <button onClick={() => save(true)}>{t("覆盖原文件")}</button>
              <button onClick={downloadLocal}>{t("下载到本地")}</button>
            </div>
          )}
        </div>
        <Tooltip label={t("关闭")} side="bottom">
          <button className="pv__btn" onClick={close}>
            <FontAwesomeIcon icon={faXmark} />
          </button>
        </Tooltip>
      </div>

      {!error && (
        <div className="pv__toolbar">
          <Tooltip label={t("全选")}>
            <button
              className={`pv__tbtn ${allSelected ? "pv__tbtn--on" : ""}`}
              onClick={selectAll}
            >
              <FontAwesomeIcon icon={faCheckDouble} />
            </button>
          </Tooltip>
          <Tooltip label={t("向左旋转")}>
            <button className="pv__tbtn" onClick={() => rotateSelected(-90)}>
              <FontAwesomeIcon icon={faRotateLeft} />
            </button>
          </Tooltip>
          <Tooltip label={t("向右旋转")}>
            <button className="pv__tbtn" onClick={() => rotateSelected(90)}>
              <FontAwesomeIcon icon={faRotateRight} />
            </button>
          </Tooltip>
          <Tooltip label={t("删除选中页")}>
            <button
              className="pv__tbtn"
              disabled={!selected.size}
              onClick={deleteSelected}
            >
              <FontAwesomeIcon icon={faTrash} />
            </button>
          </Tooltip>
          <Tooltip label={t("提取选中页为新 PDF")}>
            <button
              className="pv__tbtn"
              disabled={!selected.size}
              onClick={extractSelected}
            >
              <FontAwesomeIcon icon={faFileExport} />
            </button>
          </Tooltip>
          <Tooltip label={t("合并 PDF")}>
            <button className="pv__tbtn" onClick={mergeLocal}>
              <FontAwesomeIcon icon={faObjectGroup} />
            </button>
          </Tooltip>
          <Tooltip label={t("加页码")}>
            <button className="pv__tbtn" onClick={() => setNumOpen(true)}>
              <FontAwesomeIcon icon={faListOl} />
            </button>
          </Tooltip>
          <Tooltip label={t("加水印")}>
            <button className="pv__tbtn" onClick={() => setWmOpen(true)}>
              <FontAwesomeIcon icon={faStamp} />
            </button>
          </Tooltip>
          <Tooltip label={t("提取文本")}>
            <button className="pv__tbtn" onClick={extractText}>
              <FontAwesomeIcon icon={faFileLines} />
            </button>
          </Tooltip>
          <Tooltip label={t("压缩瘦身")}>
            <button className="pv__tbtn" disabled={saving} onClick={compress}>
              <FontAwesomeIcon icon={faCompress} />
            </button>
          </Tooltip>
          <div className="pv__spacer" />
          <span className="pv__hint">
            {selected.size
              ? t("已选 {n} 页 · 拖拽可重排", { n: String(selected.size) })
              : t("双击阅读 · 单击选择 · 拖拽重排")}
          </span>
        </div>
      )}

      <div className="pv__body">
        {loading && <div className="pv__status">{t("加载中…")}</div>}
        {error && <div className="pv__status">{t("无法打开该 PDF")}</div>}
        {!loading && !error && (
          <div className="pv__grid">
            {pages.map((p, i) => {
              const key = `${p.doc}:${p.page}`;
              const url = thumbs.get(key);
              const isSel = selected.has(p.id);
              return (
                <div
                  key={p.id}
                  className={`pv__page ${isSel ? "pv__page--sel" : ""}`}
                  draggable
                  onDragStart={() => onDragStart(i)}
                  onDragOver={(e) => e.preventDefault()}
                  onDrop={() => onDrop(i)}
                  onClick={() => toggleSelect(p.id)}
                  onDoubleClick={() => openReader(i)}
                >
                  <div className="pv__thumb">
                    {url ? (
                      <img
                        src={url}
                        alt={`page ${i + 1}`}
                        draggable={false}
                        style={{ transform: `rotate(${p.rotate}deg)` }}
                      />
                    ) : (
                      <div className="pv__thumbload">
                        <FontAwesomeIcon icon={faSpinner} spin />
                      </div>
                    )}
                  </div>
                  <div className="pv__pageno">{i + 1}</div>
                  {isSel && <div className="pv__check">✓</div>}
                </div>
              );
            })}
          </div>
        )}

        {reader !== null && (
          <div className="pv__reader" onClick={() => setReader(null)}>
            <div className="pv__reader-toolbar" onClick={(e) => e.stopPropagation()}>
              <Tooltip label={t("上一页")}>
                <button
                  className="pv__btn"
                  disabled={reader <= 0}
                  onClick={() => readerNav(-1)}
                >
                  <FontAwesomeIcon icon={faChevronLeft} />
                </button>
              </Tooltip>
              <span className="pv__reader-pos">
                {reader + 1} / {pages.length}
              </span>
              <Tooltip label={t("下一页")}>
                <button
                  className="pv__btn"
                  disabled={reader >= pages.length - 1}
                  onClick={() => readerNav(1)}
                >
                  <FontAwesomeIcon icon={faChevronRight} />
                </button>
              </Tooltip>
              <span className="pv__reader-gap" />
              <Tooltip label={t("缩小")}>
                <button
                  className="pv__btn"
                  onClick={() => setZoom((z) => Math.max(0.4, z - 0.2))}
                >
                  <FontAwesomeIcon icon={faMagnifyingGlassMinus} />
                </button>
              </Tooltip>
              <span className="pv__reader-zoom">{Math.round(zoom * 100)}%</span>
              <Tooltip label={t("放大")}>
                <button
                  className="pv__btn"
                  onClick={() => setZoom((z) => Math.min(4, z + 0.2))}
                >
                  <FontAwesomeIcon icon={faMagnifyingGlassPlus} />
                </button>
              </Tooltip>
              <span className="pv__reader-gap" />
              <Tooltip label={t("关闭")}>
                <button className="pv__btn" onClick={() => setReader(null)}>
                  <FontAwesomeIcon icon={faXmark} />
                </button>
              </Tooltip>
            </div>
            <div className="pv__reader-stage" onClick={(e) => e.stopPropagation()}>
              {readerBusy && (
                <div className="pv__reader-busy">
                  <FontAwesomeIcon icon={faSpinner} spin />
                </div>
              )}
              <canvas ref={readerCanvasRef} className="pv__reader-canvas" />
            </div>
          </div>
        )}

        {numOpen && (
          <div className="pv__modal-back" onClick={() => setNumOpen(false)}>
            <div className="pv__modal" onClick={(e) => e.stopPropagation()}>
              <div className="pv__modal-title">{t("加页码")}</div>
              <label className="pv__field">
                <span>{t("位置")}</span>
                <div className="pv__select">
                  <Select
                    value={numPos}
                    options={[
                      { value: "bottom_center", label: t("底部居中") },
                      { value: "bottom_right", label: t("底部右") },
                      { value: "bottom_left", label: t("底部左") },
                      { value: "top_center", label: t("顶部居中") },
                      { value: "top_right", label: t("顶部右") },
                      { value: "top_left", label: t("顶部左") },
                    ]}
                    onChange={(v) => setNumPos(v as PdfNumberPos)}
                  />
                </div>
              </label>
              <label className="pv__field">
                <span>{t("起始编号")}</span>
                <input
                  type="number"
                  value={numStart}
                  onChange={(e) => setNumStart(Number(e.target.value))}
                />
              </label>
              <label className="pv__field">
                <span>{t("字号")}</span>
                <input
                  type="number"
                  min={6}
                  max={72}
                  value={numSize}
                  onChange={(e) => setNumSize(Number(e.target.value))}
                />
              </label>
              <label className="pv__field">
                <span>{t("格式")}</span>
                <input
                  type="text"
                  value={numFormat}
                  placeholder="{n}"
                  onChange={(e) => setNumFormat(e.target.value)}
                />
              </label>
              <div className="pv__field-hint">{t("{n} 代表页码,如「第 {n} 页」")}</div>
              <div className="pv__modal-actions">
                <button className="pv__btn" onClick={() => setNumOpen(false)}>
                  {t("取消")}
                </button>
                <button
                  className="pv__btn pv__btn--primary"
                  onClick={applyNumbers}
                >
                  {t("生成新文件")}
                </button>
              </div>
            </div>
          </div>
        )}

        {wmOpen && (
          <div className="pv__modal-back" onClick={() => setWmOpen(false)}>
            <div className="pv__modal" onClick={(e) => e.stopPropagation()}>
              <div className="pv__modal-title">{t("加水印")}</div>
              <label className="pv__field">
                <span>{t("文字")}</span>
                <input
                  type="text"
                  value={wmText}
                  placeholder={t("如:机密 / 草稿")}
                  onChange={(e) => setWmText(e.target.value)}
                />
              </label>
              <label className="pv__field">
                <span>{t("透明度")} {wmOpacity}%</span>
                <input
                  type="range"
                  min={5}
                  max={100}
                  value={wmOpacity}
                  onChange={(e) => setWmOpacity(Number(e.target.value))}
                />
              </label>
              <label className="pv__field">
                <span>{t("角度")} {wmAngle}°</span>
                <input
                  type="range"
                  min={-90}
                  max={90}
                  value={wmAngle}
                  onChange={(e) => setWmAngle(Number(e.target.value))}
                />
              </label>
              <label className="pv__field">
                <span>{t("字号")}</span>
                <input
                  type="number"
                  min={12}
                  max={144}
                  value={wmSize}
                  onChange={(e) => setWmSize(Number(e.target.value))}
                />
              </label>
              <label className="pv__field">
                <span>{t("平铺铺满")}</span>
                <Checkbox checked={wmTile} onChange={() => setWmTile((v) => !v)} />
              </label>
              <div className="pv__modal-actions">
                <button className="pv__btn" onClick={() => setWmOpen(false)}>
                  {t("取消")}
                </button>
                <button
                  className="pv__btn pv__btn--primary"
                  onClick={applyWatermark}
                >
                  {t("生成新文件")}
                </button>
              </div>
            </div>
          </div>
        )}

        {textPages !== null && (
          <div className="pv__textpanel">
            <div className="pv__textbar">
              <span className="pv__texttitle">{t("提取文本")}</span>
              <div className="pv__spacer" />
              <Tooltip label={t("复制全部")}>
                <button className="pv__btn" onClick={copyText} disabled={textBusy}>
                  <FontAwesomeIcon icon={faCopy} />
                </button>
              </Tooltip>
              <Tooltip label={t("导出 txt")}>
                <button className="pv__btn" onClick={exportText} disabled={textBusy}>
                  <FontAwesomeIcon icon={faDownload} />
                </button>
              </Tooltip>
              <Tooltip label={t("关闭")}>
                <button className="pv__btn" onClick={() => setTextPages(null)}>
                  <FontAwesomeIcon icon={faXmark} />
                </button>
              </Tooltip>
            </div>
            <div className="pv__textbody">
              {textBusy && (
                <div className="pv__textbusy">
                  <FontAwesomeIcon icon={faSpinner} spin /> {t("提取中…")}
                </div>
              )}
              {!textBusy &&
                textPages.map((tx, i) => (
                  <div key={i} className="pv__textpage">
                    <div className="pv__textpageno">
                      {t("第 {n} 页", { n: String(i + 1) })}
                    </div>
                    <pre className="pv__textcontent">
                      {tx || t("(本页无文字层)")}
                    </pre>
                  </div>
                ))}
            </div>
          </div>
        )}

        {toast && <div className="pv__toast">{toast}</div>}
      </div>
    </div>
  );
}
