import { useCallback, useEffect, useState } from "react";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import { faArrowUp, faFolder } from "@fortawesome/free-solid-svg-icons";
import type { Entry } from "../types";
import * as api from "../api";
import { baseName, breadcrumbs, joinRemote, parentPath } from "../util";
import { useI18n } from "../i18n";

interface Props {
  account: string;
  /** 源对象完整路径,如 bucket/a/b.txt。 */
  from: string;
  onCopy: (to: string) => void;
  onMove: (to: string) => void;
  onCancel: () => void;
}

/** 级联浏览选择目标目录,再复制 / 移动过去(支持跨目录、跨桶)。 */
export function MoveCopyDialog({ account, from, onCopy, onMove, onCancel }: Props) {
  const { t } = useI18n();
  const [pickPath, setPickPath] = useState("");
  const [dirs, setDirs] = useState<Entry[]>([]);
  const [loading, setLoading] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    setErr(null);
    try {
      const entries = await api.browse(account, pickPath);
      setDirs(entries.filter((e) => e.kind === "directory"));
    } catch (e) {
      setErr(String(e));
      setDirs([]);
    } finally {
      setLoading(false);
    }
  }, [account, pickPath]);

  useEffect(() => {
    load();
  }, [load]);

  // 源可能是文件夹(带结尾斜杠),取末段作为落点名前先去掉结尾斜杠。
  const leaf = baseName(from.replace(/\/+$/, ""));
  const target = pickPath ? joinRemote(pickPath, leaf) : "";
  const canConfirm = target !== "" && target !== from;
  const crumbs = breadcrumbs(pickPath);

  return (
    <div className="modal-backdrop" onClick={onCancel}>
      <div className="modal modal--picker" onClick={(e) => e.stopPropagation()}>
        <div className="modal__header">
          <h3>{t("复制 / 移动到")}</h3>
        </div>

        <div className="modal__body">
          <div className="picker__bar">
            <button
              className="btn"
              disabled={pickPath === ""}
              onClick={() => setPickPath(parentPath(pickPath))}
              title={t("上一层")}
            >
              <FontAwesomeIcon icon={faArrowUp} />
            </button>
            <div className="picker__crumbs">
              {crumbs.map((c, i) => (
                <span key={c.path}>
                  <button
                    className="breadcrumb__link"
                    onClick={() => setPickPath(c.path)}
                  >
                    {c.label}
                  </button>
                  {i < crumbs.length - 1 && (
                    <span className="breadcrumb__sep">/</span>
                  )}
                </span>
              ))}
            </div>
          </div>

          <div className="picker__list">
            {loading ? (
              <div className="picker__state">{t("加载中…")}</div>
            ) : err ? (
              <div className="picker__state">{err}</div>
            ) : dirs.length === 0 ? (
              <div className="picker__state">{t("没有子目录")}</div>
            ) : (
              dirs.map((d) => (
                <button
                  key={d.path}
                  className="picker__item"
                  onClick={() =>
                    setPickPath(d.path.endsWith("/") ? d.path : d.path + "/")
                  }
                >
                  <FontAwesomeIcon icon={faFolder} className="picker__item-icon" />
                  <span>{d.name}</span>
                </button>
              ))
            )}
          </div>

          <div className="picker__target">
            {t("目标")}:{target || t("请进入一个 bucket / 目录")}
          </div>
        </div>

        <div className="modal__footer">
          <button className="btn" onClick={onCancel}>
            {t("取消")}
          </button>
          <button
            className="btn"
            disabled={!canConfirm}
            onClick={() => onMove(target)}
          >
            {t("移动到此")}
          </button>
          <button
            className="btn btn--primary"
            disabled={!canConfirm}
            onClick={() => onCopy(target)}
          >
            {t("复制到此")}
          </button>
        </div>
      </div>
    </div>
  );
}
