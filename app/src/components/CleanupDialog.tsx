import { useEffect, useState } from "react";
import * as api from "../api";
import type { IncompleteUpload } from "../types";
import { formatDate } from "../util";
import { useI18n } from "../i18n";

interface Props {
  account: string;
  bucket: string;
  onClose: () => void;
  onCleaned: (count: number) => void;
  onError: (msg: string) => void;
}

/** 残留分片上传清理弹窗:列出某桶下未完成的分片上传,一键全部清理。 */
export function CleanupDialog({
  account,
  bucket,
  onClose,
  onCleaned,
  onError,
}: Props) {
  const { t } = useI18n();
  const [uploads, setUploads] = useState<IncompleteUpload[] | null>(null);
  const [cleaning, setCleaning] = useState(false);

  useEffect(() => {
    let alive = true;
    api
      .incompleteUploads(account, bucket)
      .then((u) => alive && setUploads(u))
      .catch((e) => {
        if (alive) {
          onError(String(e));
          onClose();
        }
      });
    return () => {
      alive = false;
    };
  }, [account, bucket]);

  const cleanAll = async () => {
    setCleaning(true);
    try {
      const n = await api.cleanIncompleteUploads(account, bucket);
      onCleaned(n);
    } catch (e) {
      onError(String(e));
      setCleaning(false);
    }
  };

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <div className="modal__header">
          <h3>
            {t("清理未完成上传")} · {bucket}
          </h3>
        </div>
        <div className="modal__body">
          {uploads === null ? (
            <p className="tags__hint">{t("加载中…")}</p>
          ) : uploads.length === 0 ? (
            <p className="tags__hint">{t("没有残留的分片上传,一切干净。")}</p>
          ) : (
            <>
              <p className="stats__total">
                {t("发现 {n} 个未完成的分片上传(仍在计费):", {
                  n: uploads.length,
                })}
              </p>
              <div className="cleanup__list">
                {uploads.map((u) => (
                  <div className="cleanup__item" key={u.upload_id}>
                    <span className="cleanup__key" title={u.key}>
                      {u.key || "(空 key)"}
                    </span>
                    <span className="cleanup__time">
                      {u.initiated ? formatDate(u.initiated) : "—"}
                    </span>
                  </div>
                ))}
              </div>
            </>
          )}
        </div>
        <div className="modal__footer">
          <button className="btn" onClick={onClose}>
            {t("取消")}
          </button>
          {uploads !== null && uploads.length > 0 && (
            <button
              className="btn btn--danger"
              disabled={cleaning}
              onClick={cleanAll}
            >
              {t("全部清理")}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
