import { useEffect, useState } from "react";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import { faPlus, faXmark } from "@fortawesome/free-solid-svg-icons";
import * as api from "../api";
import { useI18n } from "../i18n";
import { Checkbox } from "./Checkbox";
import type { CorsRule } from "../types";

interface Props {
  account: string;
  bucket: string;
  name: string;
  onSaved: (rules: CorsRule[]) => void;
  onCancel: () => void;
  onError: (msg: string) => void;
}

const METHODS = ["GET", "PUT", "POST", "DELETE", "HEAD"];

function freshRule(): CorsRule {
  return {
    id: null,
    allowed_origins: ["*"],
    allowed_methods: ["GET"],
    allowed_headers: [],
    expose_headers: [],
    max_age_seconds: null,
  };
}

function splitCsv(text: string): string[] {
  return text
    .split(",")
    .map((s) => s.trim())
    .filter((s) => s.length > 0);
}

/** Bucket CORS 规则编辑器:加载现有规则 → 增删改规则 → 整套覆盖保存。 */
export function CorsDialog({
  account,
  bucket,
  name,
  onSaved,
  onCancel,
  onError,
}: Props) {
  const { t } = useI18n();
  const [rules, setRules] = useState<CorsRule[]>([]);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    let alive = true;
    api
      .bucketCors(account, bucket)
      .then((loaded) => {
        if (alive) {
          setRules(loaded);
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

  const updateRule = (i: number, patch: Partial<CorsRule>) =>
    setRules((rs) => rs.map((r, j) => (j === i ? { ...r, ...patch } : r)));
  const addRule = () => setRules((rs) => [...rs, freshRule()]);
  const removeRule = (i: number) => setRules((rs) => rs.filter((_, j) => j !== i));
  const toggleMethod = (i: number, method: string) =>
    updateRule(i, {
      allowed_methods: rules[i].allowed_methods.includes(method)
        ? rules[i].allowed_methods.filter((m) => m !== method)
        : [...rules[i].allowed_methods, method],
    });

  const canSave = !loading && !saving;

  const save = async () => {
    setSaving(true);
    try {
      await api.setBucketCors(account, bucket, rules);
      onSaved(rules);
    } catch (e) {
      onError(String(e));
      setSaving(false);
    }
  };

  return (
    <div className="modal-backdrop" onClick={onCancel}>
      <div className="modal modal--wide" onClick={(e) => e.stopPropagation()}>
        <div className="modal__header">
          <h3>
            {t("CORS 规则")} · {name}
          </h3>
        </div>
        <div className="modal__body">
          {loading ? (
            <p className="tags__hint">{t("加载中…")}</p>
          ) : rules.length === 0 ? (
            <p className="tags__hint">{t("没有 CORS 规则,点下方添加。")}</p>
          ) : (
            <div className="cors__rules">
              {rules.map((rule, i) => (
                <div className="cors__rule" key={i}>
                  <div className="cors__rule-head">
                    <input
                      className="prompt__input cors__id"
                      value={rule.id ?? ""}
                      placeholder={t("规则名称(可选)")}
                      onChange={(e) => updateRule(i, { id: e.target.value || null })}
                    />
                    <button
                      className="tags__remove"
                      title={t("移除")}
                      onClick={() => removeRule(i)}
                    >
                      <FontAwesomeIcon icon={faXmark} />
                    </button>
                  </div>

                  <label className="cors__field">
                    <span>{t("允许来源(用逗号分隔)")}</span>
                    <input
                      className="prompt__input"
                      value={rule.allowed_origins.join(", ")}
                      onChange={(e) =>
                        updateRule(i, { allowed_origins: splitCsv(e.target.value) })
                      }
                    />
                  </label>

                  <div className="cors__field">
                    <span>{t("允许方法")}</span>
                    <div className="cors__methods">
                      {METHODS.map((method) => (
                        <label
                          className="cors__method"
                          key={method}
                          onClick={() => toggleMethod(i, method)}
                        >
                          <Checkbox
                            checked={rule.allowed_methods.includes(method)}
                            onChange={() => toggleMethod(i, method)}
                          />
                          {method}
                        </label>
                      ))}
                    </div>
                  </div>

                  <label className="cors__field">
                    <span>{t("允许请求头(用逗号分隔)")}</span>
                    <input
                      className="prompt__input"
                      value={rule.allowed_headers.join(", ")}
                      onChange={(e) =>
                        updateRule(i, { allowed_headers: splitCsv(e.target.value) })
                      }
                    />
                  </label>

                  <label className="cors__field">
                    <span>{t("暴露的响应头(用逗号分隔)")}</span>
                    <input
                      className="prompt__input"
                      value={rule.expose_headers.join(", ")}
                      onChange={(e) =>
                        updateRule(i, { expose_headers: splitCsv(e.target.value) })
                      }
                    />
                  </label>

                  <label className="cors__field">
                    <span>{t("缓存时间(秒,留空则不设置)")}</span>
                    <input
                      type="number"
                      min={0}
                      className="prompt__input"
                      value={rule.max_age_seconds ?? ""}
                      onChange={(e) =>
                        updateRule(i, {
                          max_age_seconds: e.target.value ? Number(e.target.value) : null,
                        })
                      }
                    />
                  </label>
                </div>
              ))}
            </div>
          )}
          {!loading && (
            <button className="tags__add" onClick={addRule}>
              <FontAwesomeIcon icon={faPlus} /> {t("新增规则")}
            </button>
          )}
        </div>
        <div className="modal__footer">
          <button className="btn" onClick={onCancel}>
            {t("取消")}
          </button>
          <button className="btn btn--primary" disabled={!canSave} onClick={save}>
            {t("保存")}
          </button>
        </div>
      </div>
    </div>
  );
}
