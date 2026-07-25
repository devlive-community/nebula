import { useEffect, useRef, useState } from "react";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import { faPlus, faXmark } from "@fortawesome/free-solid-svg-icons";
import { listen } from "@tauri-apps/api/event";
import * as api from "../api";
import { useI18n } from "../i18n";

interface Props {
  account: string;
  paths: string[];
  onClose: () => void;
  onDone: () => void;
}

interface FolderProgress {
  op: string;
  path: string;
  done: number;
  total: number;
}

/** 批量标签对话框:给多选对象一次性打标签,可合并(保留其它键)或整体替换。 */
export function BatchTagsDialog({ account, paths, onClose, onDone }: Props) {
  const { t } = useI18n();
  const [rows, setRows] = useState<[string, string][]>([["", ""]]);
  const [merge, setMerge] = useState(true);
  const [busy, setBusy] = useState(false);
  const [progress, setProgress] = useState<{ done: number; total: number } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const runId = useRef("");
  const unlisten = useRef<(() => void) | null>(null);

  useEffect(() => () => unlisten.current?.(), []);

  const setKey = (i: number, v: string) =>
    setRows((r) => r.map((row, j) => (j === i ? [v, row[1]] : row)));
  const setVal = (i: number, v: string) =>
    setRows((r) => r.map((row, j) => (j === i ? [row[0], v] : row)));
  const addRow = () => setRows((r) => [...r, ["", ""]]);
  const removeRow = (i: number) =>
    setRows((r) => (r.length > 1 ? r.filter((_, j) => j !== i) : r));

  const run = async () => {
    const tags = rows
      .map(([k, v]) => [k.trim(), v.trim()] as [string, string])
      .filter(([k]) => k);
    if (tags.length === 0) {
      setError(t("请至少填一个标签键"));
      return;
    }
    setError(null);
    setBusy(true);
    setProgress({ done: 0, total: paths.length });
    runId.current = `tags-${Date.now()}`;
    unlisten.current = await listen<FolderProgress>("folder-progress", (e) => {
      if (e.payload.op === "tags" && e.payload.path === runId.current)
        setProgress({ done: e.payload.done, total: e.payload.total });
    });
    try {
      const ok = await api.setTagsBatch(runId.current, account, paths, tags, merge);
      onDone();
      onClose();
      void ok;
    } catch (e) {
      setError(String(e));
      setBusy(false);
      setProgress(null);
    }
    unlisten.current?.();
    unlisten.current = null;
  };

  const cancel = () => {
    if (runId.current) void api.cancelTransfer(runId.current);
  };

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <div className="modal__header">
          <h3>{t("批量标签({n} 项)", { n: String(paths.length) })}</h3>
        </div>
        <div className="modal__body">
          <div className="tags__rows">
            {rows.map((row, i) => (
              <div className="tags__row" key={i}>
                <input
                  className="prompt__input tags__key"
                  value={row[0]}
                  placeholder={t("键")}
                  onChange={(e) => setKey(i, e.target.value)}
                />
                <input
                  className="prompt__input tags__val"
                  value={row[1]}
                  placeholder={t("值")}
                  onChange={(e) => setVal(i, e.target.value)}
                />
                <button
                  className="tags__remove"
                  title={t("移除")}
                  onClick={() => removeRow(i)}
                >
                  <FontAwesomeIcon icon={faXmark} />
                </button>
              </div>
            ))}
          </div>
          <button className="tags__add" onClick={addRow}>
            <FontAwesomeIcon icon={faPlus} /> {t("添加标签")}
          </button>

          <label className="field field--check" style={{ marginTop: 12 }}>
            <input
              type="checkbox"
              checked={merge}
              onChange={(e) => setMerge(e.target.checked)}
            />
            <span>{t("保留各对象已有的其它标签(合并)")}</span>
          </label>

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
          {error && <div className="sync__error">{error}</div>}
        </div>
        <div className="modal__footer">
          <button className="btn" onClick={onClose}>
            {t("关闭")}
          </button>
          {busy ? (
            <button className="btn btn--danger" onClick={cancel}>
              {t("取消")}
            </button>
          ) : (
            <button className="btn btn--primary" onClick={run}>
              {merge ? t("应用标签") : t("替换标签")}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
