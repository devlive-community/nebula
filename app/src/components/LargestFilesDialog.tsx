import { useEffect, useState } from "react";
import * as api from "../api";
import { useI18n } from "../i18n";
import { formatBytes } from "../util";
import type { LargestFiles } from "../types";

interface Props {
  account: string;
  root: string;
  onClose: () => void;
  /** 点击某文件时跳到其所在目录。 */
  onOpen: (path: string) => void;
}

/** 大文件排行:列出目录下最占空间的对象,显示大小与占总量的比例条。 */
export function LargestFilesDialog({ account, root, onClose, onOpen }: Props) {
  const { t } = useI18n();
  const [data, setData] = useState<LargestFiles | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    setLoading(true);
    api
      .largestFiles(account, root, 50)
      .then((r) => alive && (setData(r), setLoading(false)))
      .catch((e) => alive && (setError(String(e)), setLoading(false)));
    return () => {
      alive = false;
    };
  }, [account, root]);

  const maxSize = data?.files[0]?.size ?? 1;

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal modal--wide" onClick={(e) => e.stopPropagation()}>
        <div className="modal__header">
          <h3>{t("大文件排行")}</h3>
        </div>
        <div className="modal__body">
          {loading && <div className="dup__state">{t("扫描中…")}</div>}
          {!loading && data && (
            <>
              <div className="dup__summary">
                {t("共 {n} 个文件 · {b}", {
                  n: String(data.total_count),
                  b: formatBytes(data.total_bytes),
                })}
              </div>
              <div className="large__list">
                {data.files.map((f) => {
                  const pct =
                    data.total_bytes > 0
                      ? (f.size / data.total_bytes) * 100
                      : 0;
                  return (
                    <button
                      className="large__row"
                      key={f.path}
                      onClick={() => onOpen(f.path)}
                      title={f.path}
                    >
                      <span className="large__path">{f.path}</span>
                      <span className="large__size">{formatBytes(f.size)}</span>
                      <span className="large__pct">{pct.toFixed(1)}%</span>
                      <span className="large__bar">
                        <span
                          className="large__fill"
                          style={{ width: `${(f.size / maxSize) * 100}%` }}
                        />
                      </span>
                    </button>
                  );
                })}
                {data.files.length === 0 && (
                  <div className="dup__state">{t("该目录没有文件")}</div>
                )}
              </div>
            </>
          )}
          {error && <div className="dup__error">{error}</div>}
        </div>
        <div className="modal__footer">
          <button className="btn" onClick={onClose}>
            {t("关闭")}
          </button>
        </div>
      </div>
    </div>
  );
}
