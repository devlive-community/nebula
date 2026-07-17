import { useCallback, useEffect, useRef, useState } from "react";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import {
  faXmark,
  faMagnifyingGlassPlus,
  faMagnifyingGlassMinus,
  faRotate,
  faCircleInfo,
  faArrowsRotate,
  faPen,
  faSpinner,
  faCrop,
  faCheck,
  faExpand,
  faRotateLeft,
  faRotateRight,
  faArrowsLeftRight,
  faArrowsUpDown,
  faDroplet,
  faCircleHalfStroke,
  faArrowRotateLeft,
  faRightFromBracket,
  faSave,
} from "@fortawesome/free-solid-svg-icons";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import * as api from "../api";
import { useI18n } from "../i18n";
import { formatBytes } from "../util";
import { Tooltip } from "./Tooltip";
import type { ExifInfo, ImageData, ImageOps } from "../types";

interface Props {
  account: string;
  path: string;
  name: string;
  etag: string | null;
  size: number;
}

const MIN_SCALE = 0.2;
const MAX_SCALE = 8;

function viewportEdge(): number {
  const dpr = window.devicePixelRatio || 1;
  const edge = Math.max(window.innerWidth, window.innerHeight) * dpr;
  return Math.min(2560, Math.round(edge));
}

function baseName(p: string): string {
  const i = p.lastIndexOf("/");
  return i >= 0 ? p.slice(i + 1) : p;
}

/** 保存格式对应的扩展名。 */
function formatExt(format: string): string {
  return format === "png" ? "png" : format === "webp" ? "webp" : "jpg";
}

/** 另存为新对象的路径:在扩展名前加 -edited,并按保存格式改扩展名。 */
function editedPath(p: string, format: string): string {
  const ext = formatExt(format);
  const slash = p.lastIndexOf("/");
  const dir = slash >= 0 ? p.slice(0, slash + 1) : "";
  const base = slash >= 0 ? p.slice(slash + 1) : p;
  const dot = base.lastIndexOf(".");
  const stem = dot > 0 ? base.slice(0, dot) : base;
  return `${dir}${stem}-edited.${ext}`;
}

/** 由原文件名推断默认保存格式。 */
function defaultFormat(name: string): string {
  if (/\.png$/i.test(name)) return "png";
  if (/\.webp$/i.test(name)) return "webp";
  return "jpeg";
}

/**
 * 独立窗口里的图片浏览器 + 编辑器:只处理打开的这一张。
 * 浏览:Rust 解码/缩放,前端缩放/平移/旋转/EXIF。
 * 编辑:前端调参 → Rust 在缓存原图上出预览 → 保存回云端(覆盖 / 另存为新对象)。
 */
export function ImageWindow({ account, path, name, etag, size }: Props) {
  const { t } = useI18n();
  const [data, setData] = useState<ImageData | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState(false);
  const [scale, setScale] = useState(1);
  const [offset, setOffset] = useState({ x: 0, y: 0 });
  const [rotation, setRotation] = useState(0);
  const [showInfo, setShowInfo] = useState(false);
  const [exif, setExif] = useState<ExifInfo | null>(null);

  const [editing, setEditing] = useState(false);
  // ops 历史栈:撤销 / 重做。同类连续微调(滑块)用 coalesce 合并成一步。
  const [hist, setHist] = useState<{ stack: ImageOps[]; idx: number }>({
    stack: [{}],
    idx: 0,
  });
  const lastCoalesce = useRef<string | null>(null);
  const ops = hist.stack[hist.idx];
  const canUndo = hist.idx > 0;
  const canRedo = hist.idx < hist.stack.length - 1;
  const pushOps = (next: ImageOps, coalesce?: string) => {
    setHist(({ stack, idx }) => {
      const base = stack.slice(0, idx + 1);
      if (coalesce && coalesce === lastCoalesce.current) {
        const copy = base.slice();
        copy[idx] = next;
        return { stack: copy, idx };
      }
      lastCoalesce.current = coalesce ?? null;
      return { stack: [...base, next], idx: idx + 1 };
    });
  };
  const resetOps = () => {
    lastCoalesce.current = null;
    setHist({ stack: [{}], idx: 0 });
  };
  const undo = () => {
    lastCoalesce.current = null;
    setHist((h) => ({ ...h, idx: Math.max(0, h.idx - 1) }));
  };
  const redo = () => {
    lastCoalesce.current = null;
    setHist((h) => ({ ...h, idx: Math.min(h.stack.length - 1, h.idx + 1) }));
  };
  const [editData, setEditData] = useState<ImageData | null>(null);
  const [editBusy, setEditBusy] = useState(false);
  const [saving, setSaving] = useState(false);
  const [saveMenu, setSaveMenu] = useState(false);
  const [toast, setToast] = useState<string | null>(null);
  // 保存格式与 JPEG 画质。
  const [format, setFormat] = useState(() => defaultFormat(name));
  const [quality, setQuality] = useState(90);
  // 尺寸调整对话框。
  const [resizeOpen, setResizeOpen] = useState(false);
  const [resizeW, setResizeW] = useState(0);
  const [resizeH, setResizeH] = useState(0);
  const [resizeLock, setResizeLock] = useState(true);
  const aspect = useRef(1);

  // 裁剪子模式:cropRect 为相对图片的比例(0..1);imgBox 是图片在舞台里的实际像素框。
  const [cropping, setCropping] = useState(false);
  const [cropRect, setCropRect] = useState({ x: 0.1, y: 0.1, w: 0.8, h: 0.8 });
  const [imgBox, setImgBox] = useState({ left: 0, top: 0, width: 0, height: 0 });
  const imgRef = useRef<HTMLImageElement>(null);
  const cropDrag = useRef<{
    mode: string;
    x: number;
    y: number;
    rect: { x: number; y: number; w: number; h: number };
  } | null>(null);

  // 测量图片在舞台内的真实渲染框(裁剪选框据此定位,永远对齐)。
  const measureImg = useCallback(() => {
    const img = imgRef.current;
    const stage = stageRef.current;
    if (!img || !stage) return;
    const ir = img.getBoundingClientRect();
    const sr = stage.getBoundingClientRect();
    setImgBox({
      left: ir.left - sr.left,
      top: ir.top - sr.top,
      width: ir.width,
      height: ir.height,
    });
  }, []);

  const stageRef = useRef<HTMLDivElement>(null);
  const drag = useRef<{ x: number; y: number; ox: number; oy: number } | null>(null);

  const shown = editing ? editData : data;

  const resetView = () => {
    setScale(1);
    setOffset({ x: 0, y: 0 });
    setRotation(0);
  };

  useEffect(() => {
    document.title = name;
  }, [name]);

  // 独立窗口跟随应用主题:设 data-theme(webview 内容)+ 窗口原生主题(标题栏)。
  useEffect(() => {
    api
      .getPref("theme")
      .then((th) => {
        const theme = th === "light" ? "light" : "dark";
        document.documentElement.setAttribute("data-theme", theme);
        void getCurrentWindow().setTheme(theme);
      })
      .catch(() => {});
  }, []);

  useEffect(() => {
    let alive = true;
    setLoading(true);
    setError(false);
    api
      .imageView(account, path, etag, viewportEdge())
      .then((d) => {
        if (!alive) return;
        setData(d);
        setLoading(false);
      })
      .catch(() => {
        if (!alive) return;
        setError(true);
        setLoading(false);
      });
    return () => {
      alive = false;
    };
  }, [account, path, etag]);

  useEffect(() => {
    if (!showInfo) return;
    setExif(null);
    let alive = true;
    api
      .imageExif(account, path, etag)
      .then((e) => alive && setExif(e))
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, [showInfo, account, path, etag]);

  // 编辑中:操作变化后(防抖)向 Rust 要预览。渲染期间显示「处理中…」。
  useEffect(() => {
    if (!editing) return;
    let alive = true;
    const id = setTimeout(() => {
      setEditBusy(true);
      api
        .imageEditPreview(account, path, etag, ops, viewportEdge())
        .then((d) => alive && setEditData(d))
        .catch((e) => alive && setToast(t("预览失败:{msg}", { msg: String(e) })))
        .finally(() => alive && setEditBusy(false));
    }, 120);
    return () => {
      alive = false;
      clearTimeout(id);
    };
  }, [editing, ops, account, path, etag]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    if (!toast) return;
    const id = setTimeout(() => setToast(null), 2600);
    return () => clearTimeout(id);
  }, [toast]);

  // 裁剪时:图片加载 / 窗口尺寸变化后重新测量图片框。
  useEffect(() => {
    if (!cropping) return;
    measureImg();
    window.addEventListener("resize", measureImg);
    return () => window.removeEventListener("resize", measureImg);
  }, [cropping, editData, measureImg]);

  const zoomAt = (factor: number, cx: number, cy: number) => {
    setScale((s) => {
      const ns = Math.min(MAX_SCALE, Math.max(MIN_SCALE, s * factor));
      const ratio = ns / s;
      setOffset((o) => ({ x: cx - (cx - o.x) * ratio, y: cy - (cy - o.y) * ratio }));
      return ns;
    });
  };

  const onWheel = (e: React.WheelEvent) => {
    e.preventDefault();
    const rect = stageRef.current?.getBoundingClientRect();
    if (!rect) return;
    const cx = e.clientX - rect.left - rect.width / 2;
    const cy = e.clientY - rect.top - rect.height / 2;
    zoomAt(e.deltaY < 0 ? 1.15 : 1 / 1.15, cx, cy);
  };

  const onMouseDown = (e: React.MouseEvent) => {
    drag.current = { x: e.clientX, y: e.clientY, ox: offset.x, oy: offset.y };
  };
  const onMouseMove = (e: React.MouseEvent) => {
    if (!drag.current) return;
    setOffset({
      x: drag.current.ox + (e.clientX - drag.current.x),
      y: drag.current.oy + (e.clientY - drag.current.y),
    });
  };
  const endDrag = () => {
    drag.current = null;
  };

  const close = useCallback(() => {
    void getCurrentWindow().close();
  }, []);

  const enterEdit = () => {
    resetOps();
    setEditData(data);
    resetView();
    setEditing(true);
  };
  const exitEdit = () => {
    setEditing(false);
    setSaveMenu(false);
    setCropping(false);
    resetView();
  };

  // 进入裁剪:清掉已有裁剪(显示完整变换图)、复位视图、给个居中初始框。
  const startCrop = () => {
    setCropRect({ x: 0.1, y: 0.1, w: 0.8, h: 0.8 });
    resetView();
    setCropping(true);
  };
  const applyCrop = () => {
    if (editData) {
      const ow = editData.orig_width;
      const oh = editData.orig_height;
      pushOps({
        ...ops,
        crop: {
          x: Math.round(cropRect.x * ow),
          y: Math.round(cropRect.y * oh),
          width: Math.max(1, Math.round(cropRect.w * ow)),
          height: Math.max(1, Math.round(cropRect.h * oh)),
        },
      });
    }
    setCropping(false);
  };

  // 打开尺寸对话框:以当前(已应用其它编辑的)全分辨率尺寸为基准。
  const openResize = () => {
    const w = ops.resize?.width ?? editData?.orig_width ?? 0;
    const h = ops.resize?.height ?? editData?.orig_height ?? 0;
    aspect.current = h > 0 ? w / h : 1;
    setResizeW(w);
    setResizeH(h);
    setResizeOpen(true);
  };
  const onResizeW = (v: number) => {
    setResizeW(v);
    if (resizeLock) setResizeH(Math.max(1, Math.round(v / aspect.current)));
  };
  const onResizeH = (v: number) => {
    setResizeH(v);
    if (resizeLock) setResizeW(Math.max(1, Math.round(v * aspect.current)));
  };
  const applyResize = () => {
    pushOps({
      ...ops,
      resize: { width: Math.max(1, resizeW), height: Math.max(1, resizeH) },
    });
    setResizeOpen(false);
  };

  const onCropDown = (e: React.MouseEvent, mode: string) => {
    e.stopPropagation();
    cropDrag.current = { mode, x: e.clientX, y: e.clientY, rect: cropRect };
  };
  const onCropMove = (e: React.MouseEvent) => {
    const d = cropDrag.current;
    if (!d || imgBox.width === 0) return;
    const dx = (e.clientX - d.x) / imgBox.width;
    const dy = (e.clientY - d.y) / imgBox.height;
    const cl = (v: number) => Math.min(1, Math.max(0, v));
    const r = d.rect;
    if (d.mode === "move") {
      setCropRect({
        ...r,
        x: Math.min(Math.max(0, r.x + dx), 1 - r.w),
        y: Math.min(Math.max(0, r.y + dy), 1 - r.h),
      });
      return;
    }
    let { x, y, w, h } = r;
    const right = r.x + r.w;
    const bottom = r.y + r.h;
    if (d.mode.includes("w")) {
      x = Math.min(cl(r.x + dx), right - 0.05);
      w = right - x;
    }
    if (d.mode.includes("e")) {
      w = Math.max(0.05, cl(right + dx) - r.x);
    }
    if (d.mode.includes("n")) {
      y = Math.min(cl(r.y + dy), bottom - 0.05);
      h = bottom - y;
    }
    if (d.mode.includes("s")) {
      h = Math.max(0.05, cl(bottom + dy) - r.y);
    }
    setCropRect({ x, y, w, h });
  };
  const onCropUp = () => {
    cropDrag.current = null;
  };

  const opsIdentity =
    !ops.crop &&
    (ops.rotate ?? 0) % 360 === 0 &&
    !ops.flip_h &&
    !ops.flip_v &&
    !ops.brightness &&
    !ops.contrast &&
    !ops.grayscale &&
    !ops.invert &&
    !ops.resize;

  const save = async (overwrite: boolean) => {
    setSaveMenu(false);
    if (opsIdentity && overwrite && format === defaultFormat(name)) {
      // 覆盖原图且无任何改动、格式也没变 → 没意义。
      setToast(t("没有改动"));
      return;
    }
    setSaving(true);
    const dest = overwrite ? path : editedPath(path, format);
    try {
      await api.imageEditSave(account, path, etag, ops, dest, format, quality);
      setToast(
        overwrite
          ? t("✓ 已覆盖保存")
          : t("✓ 已另存为 {name}", { name: baseName(dest) }),
      );
      if (editData) setData(editData);
      exitEdit();
    } catch {
      setToast(t("保存失败"));
    }
    setSaving(false);
  };

  // 下载到本地:弹保存对话框,Rust 编码后写到所选路径。
  const downloadLocal = async () => {
    setSaveMenu(false);
    const ext = formatExt(format);
    const suggested = baseName(editedPath(path, format));
    const dest = await saveDialog({
      defaultPath: suggested,
      filters: [{ name: ext.toUpperCase(), extensions: [ext] }],
    });
    if (typeof dest !== "string") return;
    setSaving(true);
    try {
      await api.imageEditDownload(account, path, etag, ops, dest, format, quality);
      setToast(t("✓ 已下载到本地"));
    } catch {
      setToast(t("保存失败"));
    }
    setSaving(false);
  };

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      // 编辑时:Cmd/Ctrl+Z 撤销,Cmd/Ctrl+Shift+Z 重做。
      if (editing && (e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "z") {
        e.preventDefault();
        if (e.shiftKey) redo();
        else undo();
        return;
      }
      switch (e.key) {
        case "+":
        case "=":
          zoomAt(1.2, 0, 0);
          break;
        case "-":
          zoomAt(1 / 1.2, 0, 0);
          break;
        case "0":
          resetView();
          break;
        case "i":
        case "I":
          setShowInfo((v) => !v);
          break;
        case "Escape":
          if (resizeOpen) setResizeOpen(false);
          else if (editing) exitEdit();
          else close();
          break;
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [close, editing, resizeOpen]); // eslint-disable-line react-hooks/exhaustive-deps

  const exifRows = exif
    ? ([
        [t("相机"), [exif.make, exif.model].filter(Boolean).join(" ")],
        [t("镜头"), exif.lens],
        [t("拍摄时间"), exif.taken_at],
        [t("光圈"), exif.aperture],
        [t("快门"), exif.exposure],
        ["ISO", exif.iso],
        [t("焦距"), exif.focal_length],
      ].filter(([, v]) => v) as [string, string][])
    : [];

  return (
    <div className="iv">
      <div className="iv__bar" data-tauri-drag-region>
        <span className="iv__name" title={name}>
          {name}
        </span>
        <div className="iv__spacer" />
        <Tooltip label={t("缩小")} side="bottom">
          <button className="iv__btn" onClick={() => zoomAt(1 / 1.2, 0, 0)}>
            <FontAwesomeIcon icon={faMagnifyingGlassMinus} />
          </button>
        </Tooltip>
        <span className="iv__zoom">{Math.round(scale * 100)}%</span>
        <Tooltip label={t("放大")} side="bottom">
          <button className="iv__btn" onClick={() => zoomAt(1.2, 0, 0)}>
            <FontAwesomeIcon icon={faMagnifyingGlassPlus} />
          </button>
        </Tooltip>
        <Tooltip label={t("复位")} side="bottom">
          <button className="iv__btn" onClick={resetView}>
            <FontAwesomeIcon icon={faArrowsRotate} />
          </button>
        </Tooltip>
        {!editing && (
          <Tooltip label={t("信息")} side="bottom">
            <button
              className={`iv__btn ${showInfo ? "iv__btn--on" : ""}`}
              onClick={() => setShowInfo((v) => !v)}
            >
              <FontAwesomeIcon icon={faCircleInfo} />
            </button>
          </Tooltip>
        )}
        {!editing && (
          <Tooltip label={t("编辑")} side="bottom">
            <button className="iv__btn" onClick={enterEdit}>
              <FontAwesomeIcon icon={faPen} />
            </button>
          </Tooltip>
        )}
        <Tooltip label={t("关闭")} side="bottom">
          <button className="iv__btn" onClick={close}>
            <FontAwesomeIcon icon={faXmark} />
          </button>
        </Tooltip>
      </div>

      {editing && !cropping && (
        <div className="iv__editbar">
          <Tooltip label={t("撤销")}>
            <button className="iv__ebtn" disabled={!canUndo} onClick={undo}>
              <FontAwesomeIcon icon={faRotateLeft} />
            </button>
          </Tooltip>
          <Tooltip label={t("重做")}>
            <button className="iv__ebtn" disabled={!canRedo} onClick={redo}>
              <FontAwesomeIcon icon={faRotateRight} />
            </button>
          </Tooltip>
          <Tooltip label={t("旋转")}>
            <button
              className="iv__ebtn"
              onClick={() => pushOps({ ...ops, rotate: ((ops.rotate ?? 0) + 90) % 360 })}
            >
              <FontAwesomeIcon icon={faRotate} />
            </button>
          </Tooltip>
          <Tooltip label={t("水平翻转")}>
            <button
              className={`iv__ebtn ${ops.flip_h ? "iv__ebtn--on" : ""}`}
              onClick={() => pushOps({ ...ops, flip_h: !ops.flip_h })}
            >
              <FontAwesomeIcon icon={faArrowsLeftRight} />
            </button>
          </Tooltip>
          <Tooltip label={t("垂直翻转")}>
            <button
              className={`iv__ebtn ${ops.flip_v ? "iv__ebtn--on" : ""}`}
              onClick={() => pushOps({ ...ops, flip_v: !ops.flip_v })}
            >
              <FontAwesomeIcon icon={faArrowsUpDown} />
            </button>
          </Tooltip>
          <Tooltip label={t("灰度")}>
            <button
              className={`iv__ebtn ${ops.grayscale ? "iv__ebtn--on" : ""}`}
              onClick={() => pushOps({ ...ops, grayscale: !ops.grayscale })}
            >
              <FontAwesomeIcon icon={faDroplet} />
            </button>
          </Tooltip>
          <Tooltip label={t("反相")}>
            <button
              className={`iv__ebtn ${ops.invert ? "iv__ebtn--on" : ""}`}
              onClick={() => pushOps({ ...ops, invert: !ops.invert })}
            >
              <FontAwesomeIcon icon={faCircleHalfStroke} />
            </button>
          </Tooltip>
          <Tooltip label={t("裁剪")}>
            <button
              className={`iv__ebtn ${ops.crop ? "iv__ebtn--on" : ""}`}
              onClick={startCrop}
            >
              <FontAwesomeIcon icon={faCrop} />
            </button>
          </Tooltip>
          <Tooltip label={t("调整尺寸")}>
            <button
              className={`iv__ebtn ${ops.resize ? "iv__ebtn--on" : ""}`}
              onClick={openResize}
            >
              <FontAwesomeIcon icon={faExpand} />
            </button>
          </Tooltip>
          <label className="iv__slider">
            {t("亮度")}
            <input
              type="range"
              min={-100}
              max={100}
              value={ops.brightness ?? 0}
              onChange={(e) =>
                pushOps({ ...ops, brightness: Number(e.target.value) }, "brightness")
              }
            />
          </label>
          <label className="iv__slider">
            {t("对比度")}
            <input
              type="range"
              min={-100}
              max={100}
              value={ops.contrast ?? 0}
              onChange={(e) =>
                pushOps({ ...ops, contrast: Number(e.target.value) }, "contrast")
              }
            />
          </label>
          <Tooltip label={t("重置")}>
            <button className="iv__ebtn" onClick={resetOps}>
              <FontAwesomeIcon icon={faArrowRotateLeft} />
            </button>
          </Tooltip>
          <div className="iv__spacer" />
          <div className="iv__savewrap">
            <Tooltip label={saving ? t("保存中…") : t("保存")}>
              <button
                className="iv__ebtn iv__ebtn--primary"
                disabled={saving}
                onClick={() => setSaveMenu((v) => !v)}
              >
                <FontAwesomeIcon icon={saving ? faSpinner : faSave} spin={saving} />
              </button>
            </Tooltip>
            {saveMenu && (
              <div className="iv__savemenu iv__savemenu--wide">
                <div className="iv__saveopt">
                  <span>{t("格式")}</span>
                  <div className="iv__fmts">
                    {["jpeg", "png", "webp"].map((f) => (
                      <button
                        key={f}
                        className={`iv__fmt ${format === f ? "iv__fmt--on" : ""}`}
                        onClick={() => setFormat(f)}
                      >
                        {f.toUpperCase()}
                      </button>
                    ))}
                  </div>
                </div>
                {format === "jpeg" && (
                  <div className="iv__saveopt">
                    <span>
                      {t("画质")} {quality}
                    </span>
                    <input
                      type="range"
                      min={40}
                      max={100}
                      value={quality}
                      onChange={(e) => setQuality(Number(e.target.value))}
                    />
                  </div>
                )}
                <div className="iv__savemenu-sep" />
                <button onClick={() => save(false)}>{t("另存为新对象")}</button>
                <button onClick={() => save(true)}>{t("覆盖原图")}</button>
                <button onClick={downloadLocal}>{t("下载到本地")}</button>
              </div>
            )}
          </div>
          <Tooltip label={t("退出编辑")}>
            <button className="iv__ebtn" onClick={exitEdit}>
              <FontAwesomeIcon icon={faRightFromBracket} />
            </button>
          </Tooltip>
        </div>
      )}

      {editing && cropping && (
        <div className="iv__editbar">
          <span className="iv__crop-hint">{t("拖动选框选择裁剪区域")}</span>
          <div className="iv__spacer" />
          <button className="iv__ebtn iv__ebtn--primary" onClick={applyCrop}>
            <FontAwesomeIcon icon={faCheck} /> {t("应用裁剪")}
          </button>
          <button className="iv__ebtn" onClick={() => setCropping(false)}>
            {t("取消")}
          </button>
        </div>
      )}

      <div
        className="iv__stage"
        ref={stageRef}
        onWheel={onWheel}
        onMouseDown={onMouseDown}
        onMouseMove={onMouseMove}
        onMouseUp={endDrag}
        onMouseLeave={endDrag}
        onDoubleClick={() => (scale === 1 ? zoomAt(2, 0, 0) : resetView())}
      >
        {loading && <div className="iv__status">{t("加载中…")}</div>}
        {error && <div className="iv__status">{t("无法加载该图片")}</div>}
        {shown && !error && !cropping && (
          <img
            className="iv__img"
            src={shown.data_url}
            alt={name}
            draggable={false}
            style={{
              transform: `translate(${offset.x}px, ${offset.y}px) scale(${scale}) rotate(${rotation}deg)`,
              cursor: scale > 1 ? "grab" : "default",
            }}
          />
        )}

        {shown && !error && cropping && (
          <div
            className="iv__croplayer"
            onMouseMove={onCropMove}
            onMouseUp={onCropUp}
            onMouseLeave={onCropUp}
          >
            <img
              ref={imgRef}
              className="iv__img"
              src={shown.data_url}
              alt={name}
              draggable={false}
              onLoad={measureImg}
            />
            {imgBox.width > 0 && (
              <div
                className="iv__crop-box"
                style={{
                  left: imgBox.left + cropRect.x * imgBox.width,
                  top: imgBox.top + cropRect.y * imgBox.height,
                  width: cropRect.w * imgBox.width,
                  height: cropRect.h * imgBox.height,
                }}
                onMouseDown={(e) => onCropDown(e, "move")}
              >
                <span className="iv__crop-h iv__crop-h--nw" onMouseDown={(e) => onCropDown(e, "nw")} />
                <span className="iv__crop-h iv__crop-h--ne" onMouseDown={(e) => onCropDown(e, "ne")} />
                <span className="iv__crop-h iv__crop-h--sw" onMouseDown={(e) => onCropDown(e, "sw")} />
                <span className="iv__crop-h iv__crop-h--se" onMouseDown={(e) => onCropDown(e, "se")} />
              </div>
            )}
          </div>
        )}

        {(editBusy || saving) && (
          <div className="iv__busy">
            <FontAwesomeIcon icon={faSpinner} spin />{" "}
            {saving ? t("保存中…") : t("处理中…")}
          </div>
        )}

        {toast && <div className="iv__toast">{toast}</div>}

        {resizeOpen && (
          <div
            className="iv__resize-backdrop"
            onMouseDown={() => setResizeOpen(false)}
          >
            <div
              className="iv__resize"
              onMouseDown={(e) => e.stopPropagation()}
            >
              <div className="iv__resize-title">{t("调整尺寸")}</div>
              <div className="iv__resize-row">
                <label>
                  {t("宽")}
                  <input
                    type="number"
                    min={1}
                    value={resizeW}
                    onChange={(e) => onResizeW(Number(e.target.value))}
                  />
                </label>
                <span className="iv__resize-x">×</span>
                <label>
                  {t("高")}
                  <input
                    type="number"
                    min={1}
                    value={resizeH}
                    onChange={(e) => onResizeH(Number(e.target.value))}
                  />
                </label>
              </div>
              <label className="iv__resize-lock">
                <input
                  type="checkbox"
                  checked={resizeLock}
                  onChange={(e) => setResizeLock(e.target.checked)}
                />
                {t("锁定宽高比")}
              </label>
              <div className="iv__resize-actions">
                <button className="iv__ebtn" onClick={() => setResizeOpen(false)}>
                  {t("取消")}
                </button>
                <button className="iv__ebtn iv__ebtn--primary" onClick={applyResize}>
                  {t("确定")}
                </button>
              </div>
            </div>
          </div>
        )}

        {showInfo && !editing && (
          <div className="iv__info" onMouseDown={(e) => e.stopPropagation()}>
            <div className="iv__info-title">{name}</div>
            <div className="iv__info-row">
              <span>{t("尺寸")}</span>
              <span>{data ? `${data.orig_width} × ${data.orig_height}` : "—"}</span>
            </div>
            <div className="iv__info-row">
              <span>{t("大小")}</span>
              <span>{formatBytes(size)}</span>
            </div>
            {exifRows.map(([k, v]) => (
              <div className="iv__info-row" key={k}>
                <span>{k}</span>
                <span>{v}</span>
              </div>
            ))}
            {exif?.gps_lat != null && exif?.gps_lon != null && (
              <a
                className="iv__info-gps"
                href={`https://www.openstreetmap.org/?mlat=${exif.gps_lat}&mlon=${exif.gps_lon}#map=15/${exif.gps_lat}/${exif.gps_lon}`}
                target="_blank"
                rel="noopener noreferrer"
              >
                {t("在地图上查看")}
              </a>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
