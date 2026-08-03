import { useEffect, useState } from "react";
import * as api from "../api";
import { useI18n } from "../i18n";
import type { WebsiteConfig } from "../types";

interface Props {
  account: string;
  bucket: string;
  name: string;
  onSaved: (config: WebsiteConfig | null) => void;
  onCancel: () => void;
  onError: (msg: string) => void;
}

/** 静态网站托管配置:加载当前状态 → 编辑首页 / 错误页 → 保存或禁用。 */
export function WebsiteDialog({
  account,
  bucket,
  name,
  onSaved,
  onCancel,
  onError,
}: Props) {
  const { t } = useI18n();
  const [config, setConfig] = useState<WebsiteConfig | null>(null);
  const [indexDoc, setIndexDoc] = useState("index.html");
  const [errorDoc, setErrorDoc] = useState("");
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    let alive = true;
    api
      .bucketWebsite(account, bucket)
      .then((loaded) => {
        if (alive) {
          setConfig(loaded);
          if (loaded) {
            setIndexDoc(loaded.index_document);
            setErrorDoc(loaded.error_document ?? "");
          }
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

  const save = async () => {
    setSaving(true);
    const next: WebsiteConfig = {
      index_document: indexDoc.trim(),
      error_document: errorDoc.trim() || null,
    };
    try {
      await api.setBucketWebsite(account, bucket, next);
      onSaved(next);
    } catch (e) {
      onError(String(e));
      setSaving(false);
    }
  };

  const disable = async () => {
    setSaving(true);
    try {
      await api.setBucketWebsite(account, bucket, null);
      onSaved(null);
    } catch (e) {
      onError(String(e));
      setSaving(false);
    }
  };

  const canSave = !loading && !saving && indexDoc.trim().length > 0;

  return (
    <div className="modal-backdrop" onClick={onCancel}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <div className="modal__header">
          <h3>
            {t("静态网站托管")} · {name}
          </h3>
        </div>
        <div className="modal__body">
          {loading ? (
            <p className="tags__hint">{t("加载中…")}</p>
          ) : (
            <>
              <p className="tags__hint">{config ? t("已启用") : t("未启用")}</p>
              <label className="field">
                <span>{t("首页文档")}</span>
                <input
                  className="prompt__input"
                  value={indexDoc}
                  placeholder="index.html"
                  onChange={(e) => setIndexDoc(e.target.value)}
                />
              </label>
              <label className="field">
                <span>{t("错误页文档(可选)")}</span>
                <input
                  className="prompt__input"
                  value={errorDoc}
                  placeholder="error.html"
                  onChange={(e) => setErrorDoc(e.target.value)}
                />
              </label>
            </>
          )}
        </div>
        <div className="modal__footer">
          <button className="btn" onClick={onCancel}>
            {t("取消")}
          </button>
          {config && (
            <button className="btn" disabled={loading || saving} onClick={disable}>
              {t("禁用静态网站托管")}
            </button>
          )}
          <button className="btn btn--primary" disabled={!canSave} onClick={save}>
            {t("保存")}
          </button>
        </div>
      </div>
    </div>
  );
}
