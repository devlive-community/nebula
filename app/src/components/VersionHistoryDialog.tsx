import { useEffect, useState } from "react";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import { faXmark } from "@fortawesome/free-solid-svg-icons";
import * as api from "../api";
import { useI18n } from "../i18n";
import { formatBytes, formatDate } from "../util";
import type { ObjectVersion } from "../types";
import { ConfirmDialog } from "./ConfirmDialog";

interface Props {
  account: string;
  path: string;
  name: string;
  onClose: () => void;
  onChanged: () => void;
  onError: (msg: string) => void;
}

/** 对象版本历史:加载全部版本 → 恢复旧版本 / 永久删除某个版本(含删除标记)。 */
export function VersionHistoryDialog({
  account,
  path,
  name,
  onClose,
  onChanged,
  onError,
}: Props) {
  const { t } = useI18n();
  const [versions, setVersions] = useState<ObjectVersion[]>([]);
  const [loading, setLoading] = useState(true);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [pendingDeleteId, setPendingDeleteId] = useState<string | null>(null);

  const load = () => {
    setLoading(true);
    api
      .listObjectVersions(account, path)
      .then((v) => {
        setVersions(v);
        setLoading(false);
      })
      .catch((e) => {
        onError(String(e));
        onClose();
      });
  };

  useEffect(load, [account, path]);

  const restore = async (versionId: string) => {
    setBusyId(versionId);
    try {
      await api.restoreObjectVersion(account, path, versionId);
      onChanged();
      load();
    } catch (e) {
      onError(String(e));
    } finally {
      setBusyId(null);
    }
  };

  const remove = async (versionId: string) => {
    setPendingDeleteId(null);
    setBusyId(versionId);
    try {
      await api.deleteObjectVersion(account, path, versionId);
      onChanged();
      load();
    } catch (e) {
      onError(String(e));
    } finally {
      setBusyId(null);
    }
  };

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal modal--wide" onClick={(e) => e.stopPropagation()}>
        <div className="modal__header">
          <h3>
            {t("版本历史")} · {name}
          </h3>
          <button className="modal__close" onClick={onClose}>
            <FontAwesomeIcon icon={faXmark} />
          </button>
        </div>
        <div className="modal__body">
          {loading ? (
            <p className="tags__hint">{t("加载中…")}</p>
          ) : versions.length === 0 ? (
            <p className="tags__hint">{t("还没有版本历史")}</p>
          ) : (
            <div className="version__rows">
              {versions.map((v) => (
                <div className="version__row" key={v.version_id}>
                  <div className="version__meta">
                    <span className="version__date">{formatDate(v.last_modified)}</span>
                    {v.is_delete_marker ? (
                      <span className="version__badge version__badge--deleted">
                        {t("已删除")}
                      </span>
                    ) : (
                      <span className="version__size">{formatBytes(v.size)}</span>
                    )}
                    {v.is_latest && (
                      <span className="version__badge version__badge--current">
                        {t("当前版本")}
                      </span>
                    )}
                  </div>
                  <div className="version__actions">
                    {!v.is_delete_marker && !v.is_latest && (
                      <button
                        className="btn btn--sm"
                        disabled={busyId === v.version_id}
                        onClick={() => restore(v.version_id)}
                      >
                        {t("恢复此版本")}
                      </button>
                    )}
                    {(v.is_delete_marker || !v.is_latest) && (
                      <button
                        className="btn btn--sm btn--danger"
                        disabled={busyId === v.version_id}
                        onClick={() => setPendingDeleteId(v.version_id)}
                      >
                        {t("永久删除此版本")}
                      </button>
                    )}
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>
        <div className="modal__footer">
          <button className="btn" onClick={onClose}>
            {t("关闭")}
          </button>
        </div>
      </div>
      {pendingDeleteId && (
        <ConfirmDialog
          title={t("永久删除此版本")}
          message={t("确定永久删除此版本?此操作不可恢复。")}
          danger
          confirmLabel={t("永久删除此版本")}
          onConfirm={() => remove(pendingDeleteId)}
          onCancel={() => setPendingDeleteId(null)}
        />
      )}
    </div>
  );
}
