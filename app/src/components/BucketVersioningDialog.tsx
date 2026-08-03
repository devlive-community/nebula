import { useEffect, useState } from "react";
import * as api from "../api";
import { useI18n } from "../i18n";

interface Props {
  account: string;
  bucket: string;
  name: string;
  onSaved: (enabled: boolean) => void;
  onCancel: () => void;
  onError: (msg: string) => void;
}

/** Bucket 版本控制开关:加载当前状态 → 启用 / 暂停。 */
export function BucketVersioningDialog({
  account,
  bucket,
  name,
  onSaved,
  onCancel,
  onError,
}: Props) {
  const { t } = useI18n();
  const [enabled, setEnabled] = useState(false);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    let alive = true;
    api
      .bucketVersioning(account, bucket)
      .then((v) => {
        if (alive) {
          setEnabled(v);
          setLoading(false);
        }
      })
      .catch((e) => {
        if (alive) {
          onError(String(e));
          onCancel();
        }
      });
    return () => {
      alive = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [account, bucket]);

  const toggle = async () => {
    setSaving(true);
    const next = !enabled;
    try {
      await api.setBucketVersioning(account, bucket, next);
      onSaved(next);
    } catch (e) {
      onError(String(e));
      setSaving(false);
    }
  };

  return (
    <div className="modal-backdrop" onClick={onCancel}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <div className="modal__header">
          <h3>
            {t("版本控制")} · {name}
          </h3>
        </div>
        <div className="modal__body">
          {loading ? (
            <p className="tags__hint">{t("加载中…")}</p>
          ) : (
            <label className="field">
              <span>{enabled ? t("已启用") : t("未启用")}</span>
            </label>
          )}
        </div>
        <div className="modal__footer">
          <button className="btn" onClick={onCancel}>
            {t("取消")}
          </button>
          <button
            className="btn btn--primary"
            disabled={loading || saving}
            onClick={toggle}
          >
            {enabled ? t("暂停版本控制") : t("启用版本控制")}
          </button>
        </div>
      </div>
    </div>
  );
}
