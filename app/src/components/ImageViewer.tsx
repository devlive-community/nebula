import { useCallback, useEffect, useRef, useState } from "react";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import {
  faXmark,
  faChevronLeft,
  faChevronRight,
  faMagnifyingGlassPlus,
  faMagnifyingGlassMinus,
  faRotate,
  faExpand,
  faCompress,
  faCircleInfo,
} from "@fortawesome/free-solid-svg-icons";
import * as api from "../api";
import { useI18n } from "../i18n";
import { formatBytes } from "../util";
import type { ExifInfo, ImageData } from "../types";

export interface ImageItem {
  path: string;
  name: string;
  etag: string | null;
  size: number;
}

interface Props {
  account: string;
  images: ImageItem[];
  index: number;
  onClose: () => void;
}

const MIN_SCALE = 0.2;
const MAX_SCALE = 8;

/** 视口边长(乘 DPR、封顶),决定 Rust 渲染的展示图分辨率。 */
function viewportEdge(): number {
  const dpr = window.devicePixelRatio || 1;
  const edge = Math.max(window.innerWidth, window.innerHeight) * dpr;
  return Math.min(2560, Math.round(edge));
}

/** 缩略图条里的一格,挂载时按需向 Rust 要缩略图(Rust 会缓存)。 */
function StripThumb({
  account,
  item,
  active,
  onClick,
}: {
  account: string;
  item: ImageItem;
  active: boolean;
  onClick: () => void;
}) {
  const [url, setUrl] = useState<string | null>(null);
  useEffect(() => {
    let alive = true;
    api
      .imageThumb(account, item.path, item.etag, 96)
      .then((d) => alive && setUrl(d.data_url))
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, [account, item.path, item.etag]);
  return (
    <button
      className={`iv__thumb ${active ? "iv__thumb--active" : ""}`}
      title={item.name}
      onClick={onClick}
    >
      {url ? <img src={url} alt={item.name} /> : <div className="iv__thumb-ph" />}
    </button>
  );
}

/**
 * 图片浏览器:Rust 侧已把图解码 / 缩放到视口大小,这里只做缩放 / 平移 / 旋转 /
 * 前后翻页 / 全屏 / 信息与 EXIF。滚轮按光标缩放、拖拽平移、双击 适应↔2x。
 */
export function ImageViewer({ account, images, index, onClose }: Props) {
  const { t } = useI18n();
  const [idx, setIdx] = useState(index);
  const [data, setData] = useState<ImageData | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState(false);
  const [scale, setScale] = useState(1);
  const [offset, setOffset] = useState({ x: 0, y: 0 });
  const [rotation, setRotation] = useState(0);
  const [fullscreen, setFullscreen] = useState(false);
  const [showInfo, setShowInfo] = useState(false);
  const [exif, setExif] = useState<ExifInfo | null>(null);

  const rootRef = useRef<HTMLDivElement>(null);
  const stageRef = useRef<HTMLDivElement>(null);
  const drag = useRef<{ x: number; y: number; ox: number; oy: number } | null>(null);

  const cur = images[idx];

  const resetView = () => {
    setScale(1);
    setOffset({ x: 0, y: 0 });
    setRotation(0);
  };

  // 加载当前图,并预取相邻两张(Rust 会缓存,翻页即时)。
  useEffect(() => {
    if (!cur) return;
    let alive = true;
    setLoading(true);
    setError(false);
    resetView();
    const edge = viewportEdge();
    api
      .imageView(account, cur.path, cur.etag, edge)
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
    for (const n of [idx - 1, idx + 1]) {
      const nb = images[n];
      if (nb) void api.imageView(account, nb.path, nb.etag, edge).catch(() => {});
    }
    return () => {
      alive = false;
    };
  }, [account, idx]); // eslint-disable-line react-hooks/exhaustive-deps

  // 信息面板打开时懒加载 EXIF。
  useEffect(() => {
    if (!showInfo || !cur) return;
    setExif(null);
    let alive = true;
    api
      .imageExif(account, cur.path, cur.etag)
      .then((e) => alive && setExif(e))
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, [showInfo, account, idx]); // eslint-disable-line react-hooks/exhaustive-deps

  const go = useCallback(
    (delta: number) => {
      setIdx((i) => {
        const n = i + delta;
        return n < 0 || n >= images.length ? i : n;
      });
    },
    [images.length],
  );

  const zoomAt = (factor: number, cx: number, cy: number) => {
    setScale((s) => {
      const ns = Math.min(MAX_SCALE, Math.max(MIN_SCALE, s * factor));
      const ratio = ns / s;
      setOffset((o) => ({
        x: cx - (cx - o.x) * ratio,
        y: cy - (cy - o.y) * ratio,
      }));
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

  const toggleFullscreen = useCallback(() => {
    const el = rootRef.current;
    if (!el) return;
    if (document.fullscreenElement) void document.exitFullscreen();
    else void el.requestFullscreen?.();
  }, []);

  useEffect(() => {
    const onFs = () => setFullscreen(!!document.fullscreenElement);
    document.addEventListener("fullscreenchange", onFs);
    return () => document.removeEventListener("fullscreenchange", onFs);
  }, []);

  // 键盘:←/→ 翻页 · +/− 缩放 · 0 复位 · R 旋转 · F 全屏 · I 信息 · Esc 关闭。
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      switch (e.key) {
        case "ArrowLeft":
          go(-1);
          break;
        case "ArrowRight":
          go(1);
          break;
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
        case "r":
        case "R":
          setRotation((r) => (r + 90) % 360);
          break;
        case "f":
        case "F":
          toggleFullscreen();
          break;
        case "i":
        case "I":
          setShowInfo((v) => !v);
          break;
        case "Escape":
          if (document.fullscreenElement) void document.exitFullscreen();
          else onClose();
          break;
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [go, toggleFullscreen, onClose]);

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
    <div className="iv" ref={rootRef}>
      <div className="iv__bar">
        <span className="iv__name" title={cur?.name}>
          {cur?.name}
        </span>
        <span className="iv__count">
          {idx + 1} / {images.length}
        </span>
        <div className="iv__spacer" />
        <button className="iv__btn" title={t("缩小")} onClick={() => zoomAt(1 / 1.2, 0, 0)}>
          <FontAwesomeIcon icon={faMagnifyingGlassMinus} />
        </button>
        <span className="iv__zoom">{Math.round(scale * 100)}%</span>
        <button className="iv__btn" title={t("放大")} onClick={() => zoomAt(1.2, 0, 0)}>
          <FontAwesomeIcon icon={faMagnifyingGlassPlus} />
        </button>
        <button
          className="iv__btn"
          title={t("旋转")}
          onClick={() => setRotation((r) => (r + 90) % 360)}
        >
          <FontAwesomeIcon icon={faRotate} />
        </button>
        <button
          className={`iv__btn ${showInfo ? "iv__btn--on" : ""}`}
          title={t("信息")}
          onClick={() => setShowInfo((v) => !v)}
        >
          <FontAwesomeIcon icon={faCircleInfo} />
        </button>
        <button className="iv__btn" title={t("全屏")} onClick={toggleFullscreen}>
          <FontAwesomeIcon icon={fullscreen ? faCompress : faExpand} />
        </button>
        <button className="iv__btn" title={t("关闭")} onClick={onClose}>
          <FontAwesomeIcon icon={faXmark} />
        </button>
      </div>

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
        {idx > 0 && (
          <button className="iv__nav iv__nav--prev" onClick={() => go(-1)} title={t("上一张")}>
            <FontAwesomeIcon icon={faChevronLeft} />
          </button>
        )}
        {loading && <div className="iv__status">{t("加载中…")}</div>}
        {error && <div className="iv__status">{t("无法加载该图片")}</div>}
        {data && !error && (
          <img
            className="iv__img"
            src={data.data_url}
            alt={cur?.name}
            draggable={false}
            style={{
              transform: `translate(${offset.x}px, ${offset.y}px) scale(${scale}) rotate(${rotation}deg)`,
              cursor: scale > 1 ? "grab" : "default",
            }}
          />
        )}
        {idx < images.length - 1 && (
          <button className="iv__nav iv__nav--next" onClick={() => go(1)} title={t("下一张")}>
            <FontAwesomeIcon icon={faChevronRight} />
          </button>
        )}

        {showInfo && (
          <div className="iv__info" onMouseDown={(e) => e.stopPropagation()}>
            <div className="iv__info-title">{cur?.name}</div>
            <div className="iv__info-row">
              <span>{t("尺寸")}</span>
              <span>
                {data ? `${data.orig_width} × ${data.orig_height}` : "—"}
              </span>
            </div>
            <div className="iv__info-row">
              <span>{t("大小")}</span>
              <span>{cur ? formatBytes(cur.size) : "—"}</span>
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

      {images.length > 1 && (
        <div className="iv__strip">
          {images.map((it, i) => (
            <StripThumb
              key={it.path}
              account={account}
              item={it}
              active={i === idx}
              onClick={() => setIdx(i)}
            />
          ))}
        </div>
      )}
    </div>
  );
}
