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
  faFloppyDisk,
} from "@fortawesome/free-solid-svg-icons";
import { getCurrentWindow } from "@tauri-apps/api/window";
import * as api from "../api";
import { useI18n } from "../i18n";
import { formatBytes } from "../util";
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

/** 另存为新对象的路径:在扩展名前加 -edited,并按保存格式改扩展名。 */
function editedPath(p: string, format: string): string {
  const ext = format === "png" ? "png" : "jpg";
  const slash = p.lastIndexOf("/");
  const dir = slash >= 0 ? p.slice(0, slash + 1) : "";
  const base = slash >= 0 ? p.slice(slash + 1) : p;
  const dot = base.lastIndexOf(".");
  const stem = dot > 0 ? base.slice(0, dot) : base;
  return `${dir}${stem}-edited.${ext}`;
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
  const [ops, setOps] = useState<ImageOps>({});
  const [editData, setEditData] = useState<ImageData | null>(null);
  const [saving, setSaving] = useState(false);
  const [saveMenu, setSaveMenu] = useState(false);
  const [toast, setToast] = useState<string | null>(null);

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

  // 编辑中:操作变化后(防抖)向 Rust 要预览(基于缓存原图,快)。
  useEffect(() => {
    if (!editing) return;
    let alive = true;
    const id = setTimeout(() => {
      api
        .imageEditPreview(account, path, etag, ops, viewportEdge())
        .then((d) => alive && setEditData(d))
        .catch(() => {});
    }, 120);
    return () => {
      alive = false;
      clearTimeout(id);
    };
  }, [editing, ops, account, path, etag]);

  useEffect(() => {
    if (!toast) return;
    const id = setTimeout(() => setToast(null), 2600);
    return () => clearTimeout(id);
  }, [toast]);

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
    setOps({});
    setEditData(data);
    resetView();
    setEditing(true);
  };
  const exitEdit = () => {
    setEditing(false);
    setSaveMenu(false);
    resetView();
  };

  const opsIdentity =
    !ops.crop &&
    (ops.rotate ?? 0) % 360 === 0 &&
    !ops.flip_h &&
    !ops.flip_v &&
    !ops.brightness &&
    !ops.contrast &&
    !ops.grayscale &&
    !ops.invert;

  const save = async (overwrite: boolean) => {
    setSaveMenu(false);
    if (opsIdentity) {
      setToast(t("没有改动"));
      return;
    }
    setSaving(true);
    const format = /\.png$/i.test(name) ? "png" : "jpeg";
    const dest = overwrite ? path : editedPath(path, format);
    try {
      await api.imageEditSave(account, path, etag, ops, dest, format, 90);
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

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
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
          if (editing) exitEdit();
          else close();
          break;
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [close, editing]);

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
        <button className="iv__btn" title={t("缩小")} onClick={() => zoomAt(1 / 1.2, 0, 0)}>
          <FontAwesomeIcon icon={faMagnifyingGlassMinus} />
        </button>
        <span className="iv__zoom">{Math.round(scale * 100)}%</span>
        <button className="iv__btn" title={t("放大")} onClick={() => zoomAt(1.2, 0, 0)}>
          <FontAwesomeIcon icon={faMagnifyingGlassPlus} />
        </button>
        <button className="iv__btn" title={t("复位")} onClick={resetView}>
          <FontAwesomeIcon icon={faArrowsRotate} />
        </button>
        {!editing && (
          <button
            className={`iv__btn ${showInfo ? "iv__btn--on" : ""}`}
            title={t("信息")}
            onClick={() => setShowInfo((v) => !v)}
          >
            <FontAwesomeIcon icon={faCircleInfo} />
          </button>
        )}
        {!editing && (
          <button className="iv__btn" title={t("编辑")} onClick={enterEdit}>
            <FontAwesomeIcon icon={faPen} />
          </button>
        )}
        <button className="iv__btn" title={t("关闭")} onClick={close}>
          <FontAwesomeIcon icon={faXmark} />
        </button>
      </div>

      {editing && (
        <div className="iv__editbar">
          <button className="iv__ebtn" onClick={() => setOps((o) => ({ ...o, rotate: (((o.rotate ?? 0) + 90) % 360) }))}>
            <FontAwesomeIcon icon={faRotate} /> {t("旋转")}
          </button>
          <button
            className={`iv__ebtn ${ops.flip_h ? "iv__ebtn--on" : ""}`}
            onClick={() => setOps((o) => ({ ...o, flip_h: !o.flip_h }))}
          >
            {t("水平翻转")}
          </button>
          <button
            className={`iv__ebtn ${ops.flip_v ? "iv__ebtn--on" : ""}`}
            onClick={() => setOps((o) => ({ ...o, flip_v: !o.flip_v }))}
          >
            {t("垂直翻转")}
          </button>
          <button
            className={`iv__ebtn ${ops.grayscale ? "iv__ebtn--on" : ""}`}
            onClick={() => setOps((o) => ({ ...o, grayscale: !o.grayscale }))}
          >
            {t("灰度")}
          </button>
          <button
            className={`iv__ebtn ${ops.invert ? "iv__ebtn--on" : ""}`}
            onClick={() => setOps((o) => ({ ...o, invert: !o.invert }))}
          >
            {t("反相")}
          </button>
          <label className="iv__slider">
            {t("亮度")}
            <input
              type="range"
              min={-100}
              max={100}
              value={ops.brightness ?? 0}
              onChange={(e) => setOps((o) => ({ ...o, brightness: Number(e.target.value) }))}
            />
          </label>
          <label className="iv__slider">
            {t("对比度")}
            <input
              type="range"
              min={-100}
              max={100}
              value={ops.contrast ?? 0}
              onChange={(e) => setOps((o) => ({ ...o, contrast: Number(e.target.value) }))}
            />
          </label>
          <button className="iv__ebtn" onClick={() => setOps({})}>
            {t("重置")}
          </button>
          <div className="iv__spacer" />
          <div className="iv__savewrap">
            <button
              className="iv__ebtn iv__ebtn--primary"
              disabled={saving}
              onClick={() => setSaveMenu((v) => !v)}
            >
              <FontAwesomeIcon icon={faFloppyDisk} /> {saving ? t("保存中…") : t("保存")}
            </button>
            {saveMenu && (
              <div className="iv__savemenu">
                <button onClick={() => save(false)}>{t("另存为新对象")}</button>
                <button onClick={() => save(true)}>{t("覆盖原图")}</button>
              </div>
            )}
          </div>
          <button className="iv__ebtn" onClick={exitEdit}>
            {t("退出编辑")}
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
        {shown && !error && (
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

        {toast && <div className="iv__toast">{toast}</div>}

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
