import { useEffect, useState } from "react";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import { faPlus, faXmark } from "@fortawesome/free-solid-svg-icons";
import * as api from "../api";
import { useI18n } from "../i18n";
import { STORAGE_CLASSES } from "./StorageClassDialog";
import { Select } from "./Select";
import type { LifecycleRule } from "../types";

interface Props {
  account: string;
  vendor: string;
  bucket: string;
  name: string;
  onSaved: (rules: LifecycleRule[]) => void;
  onCancel: () => void;
  onError: (msg: string) => void;
}

let nextId = 0;
/** 新规则的默认 ID:时间戳 + 自增序号,避免同一秒内新增多条规则时重复。 */
function freshRuleId(): string {
  nextId += 1;
  return `rule-${Date.now()}-${nextId}`;
}

/** Bucket 生命周期规则编辑器:加载现有规则 → 增删改规则与转换项 → 整套覆盖保存。 */
export function LifecycleDialog({
  account,
  vendor,
  bucket,
  name,
  onSaved,
  onCancel,
  onError,
}: Props) {
  const { t } = useI18n();
  const [rules, setRules] = useState<LifecycleRule[]>([]);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const classOptions = STORAGE_CLASSES[vendor] ?? [{ value: "STANDARD", label: "标准" }];

  useEffect(() => {
    let alive = true;
    api
      .bucketLifecycle(account, bucket)
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

  const updateRule = (i: number, patch: Partial<LifecycleRule>) =>
    setRules((rs) => rs.map((r, j) => (j === i ? { ...r, ...patch } : r)));
  const addRule = () =>
    setRules((rs) => [
      ...rs,
      {
        id: freshRuleId(),
        prefix: "",
        enabled: true,
        expiration_days: null,
        transitions: [],
      },
    ]);
  const removeRule = (i: number) => setRules((rs) => rs.filter((_, j) => j !== i));

  const addTransition = (i: number) =>
    updateRule(i, {
      transitions: [...rules[i].transitions, [30, classOptions[0].value]],
    });
  const updateTransition = (i: number, ti: number, patch: Partial<{ days: number; cls: string }>) =>
    updateRule(i, {
      transitions: rules[i].transitions.map((tr, j) =>
        j === ti ? [patch.days ?? tr[0], patch.cls ?? tr[1]] : tr,
      ),
    });
  const removeTransition = (i: number, ti: number) =>
    updateRule(i, { transitions: rules[i].transitions.filter((_, j) => j !== ti) });

  const canSave = !loading && !saving && rules.every((r) => r.id.trim().length > 0);

  const save = async () => {
    setSaving(true);
    try {
      await api.setBucketLifecycle(account, bucket, rules);
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
            {t("生命周期规则")} · {name}
          </h3>
        </div>
        <div className="modal__body">
          {loading ? (
            <p className="tags__hint">{t("加载中…")}</p>
          ) : rules.length === 0 ? (
            <p className="tags__hint">{t("没有生命周期规则,点下方添加。")}</p>
          ) : (
            <div className="lifecycle__rules">
              {rules.map((rule, i) => (
                <div className="lifecycle__rule" key={i}>
                  <div className="lifecycle__rule-head">
                    <input
                      className="prompt__input lifecycle__id"
                      value={rule.id}
                      placeholder={t("规则名称")}
                      onChange={(e) => updateRule(i, { id: e.target.value })}
                    />
                    <input
                      className="prompt__input lifecycle__prefix"
                      value={rule.prefix}
                      placeholder={t("前缀")}
                      onChange={(e) => updateRule(i, { prefix: e.target.value })}
                    />
                    <label className="lifecycle__enabled">
                      <input
                        type="checkbox"
                        checked={rule.enabled}
                        onChange={(e) => updateRule(i, { enabled: e.target.checked })}
                      />
                      {t("启用")}
                    </label>
                    <button
                      className="tags__remove"
                      title={t("移除")}
                      onClick={() => removeRule(i)}
                    >
                      <FontAwesomeIcon icon={faXmark} />
                    </button>
                  </div>

                  <div className="lifecycle__transitions">
                    {rule.transitions.map(([days, cls], ti) => (
                      <div className="lifecycle__transition" key={ti}>
                        <input
                          type="number"
                          min={1}
                          className="prompt__input lifecycle__days"
                          value={days}
                          onChange={(e) =>
                            updateTransition(i, ti, { days: Number(e.target.value) })
                          }
                        />
                        <span className="lifecycle__days-suffix">{t("天数")}</span>
                        <div className="lifecycle__class">
                          <Select
                            value={cls}
                            options={classOptions.map((o) => ({
                              value: o.value,
                              label: `${t(o.label)}(${o.value})`,
                            }))}
                            onChange={(v) => updateTransition(i, ti, { cls: v })}
                          />
                        </div>
                        <button
                          className="tags__remove"
                          title={t("移除")}
                          onClick={() => removeTransition(i, ti)}
                        >
                          <FontAwesomeIcon icon={faXmark} />
                        </button>
                      </div>
                    ))}
                    <button className="tags__add" onClick={() => addTransition(i)}>
                      <FontAwesomeIcon icon={faPlus} /> {t("添加转换规则")}
                    </button>
                  </div>

                  <label className="lifecycle__expiration">
                    <span>{t("过期天数(留空则不过期)")}</span>
                    <input
                      type="number"
                      min={1}
                      className="prompt__input"
                      value={rule.expiration_days ?? ""}
                      onChange={(e) =>
                        updateRule(i, {
                          expiration_days: e.target.value ? Number(e.target.value) : null,
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
