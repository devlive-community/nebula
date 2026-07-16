import { useCallback, useEffect, useRef, useState } from "react";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import {
  faXmark,
  faMagnifyingGlassPlus,
  faMagnifyingGlassMinus,
  faRotate,
  faCircleInfo,
  faArrowsRotate,
} from "@fortawesome/free-solid-svg-icons";
import { getCurrentWindow } from "@tauri-apps/api/window";
import * as api from "../api";
import { useI18n } from "../i18n";
import { formatBytes } from "../util";
import type { ExifInfo, ImageData } from "../types";

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

/**
 * 独立窗口里的图片浏览器:只显示打开的这一张(不加载同目录其它图)。
 * Rust 侧已解码 / 缩放到视口大小,这里做缩放 / 平移 / 旋转 / 信息 / EXIF。
 * 编辑器(裁剪 / 调整 / 滤镜)后续也落在这个窗口。
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

  const stageRef = useRef<HTMLDivElement>(null);
  const drag = useRef<{ x: number; y: number; ox: number; oy: number } | null>(null);

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

  const close = useCallback(() => {
    void getCurrentWindow().close();
  }, []);

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
        case "r":
        case "R":
          setRotation((r) => (r + 90) % 360);
          break;
        case "i":
        case "I":
          setShowInfo((v) => !v);
          break;
        case "Escape":
          close();
          break;
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [close]);

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
        <button
          className="iv__btn"
          title={t("旋转")}
          onClick={() => setRotation((r) => (r + 90) % 360)}
        >
          <FontAwesomeIcon icon={faRotate} />
        </button>
        <button className="iv__btn" title={t("复位")} onClick={resetView}>
          <FontAwesomeIcon icon={faArrowsRotate} />
        </button>
        <button
          className={`iv__btn ${showInfo ? "iv__btn--on" : ""}`}
          title={t("信息")}
          onClick={() => setShowInfo((v) => !v)}
        >
          <FontAwesomeIcon icon={faCircleInfo} />
        </button>
        <button className="iv__btn" title={t("关闭")} onClick={close}>
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
        {loading && <div className="iv__status">{t("加载中…")}</div>}
        {error && <div className="iv__status">{t("无法加载该图片")}</div>}
        {data && !error && (
          <img
            className="iv__img"
            src={data.data_url}
            alt={name}
            draggable={false}
            style={{
              transform: `translate(${offset.x}px, ${offset.y}px) scale(${scale}) rotate(${rotation}deg)`,
              cursor: scale > 1 ? "grab" : "default",
            }}
          />
        )}

        {showInfo && (
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
