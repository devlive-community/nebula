import { useEffect, useMemo, useState } from "react";
import { Select } from "./Select";
import { useI18n } from "../i18n";
import * as api from "../api";
import type { RenamePlan } from "../types";

interface Props {
  /** 选中的对象完整路径。 */
  paths: string[];
  onApply: (plan: RenamePlan[]) => void;
  onCancel: () => void;
}

/** 取路径最后一段(基名)用于预览展示。 */
function baseName(path: string): string {
  const i = path.lastIndexOf("/");
  return i >= 0 ? path.slice(i + 1) : path;
}

/**
 * 批量重命名:给选中对象统一加前缀 / 加后缀(扩展名前)/ 查找替换。
 * 改名计划由后端纯函数计算(与执行同一逻辑),这里实时预览、确认后再执行。
 */
export function BatchRenameDialog({ paths, onApply, onCancel }: Props) {
  const { t } = useI18n();
  const [mode, setMode] = useState("prefix");
  const [a, setA] = useState("");
  const [b, setB] = useState("");
  const [plan, setPlan] = useState<RenamePlan[]>([]);

  // 规则或选择变化时,向后端要新的改名计划(本地 IPC,基本无延迟)。
  useEffect(() => {
    let alive = true;
    api
      .planBatchRename(paths, { mode, a, b })
      .then((p) => {
        if (alive) setPlan(p);
      })
      .catch(() => {
        if (alive) setPlan([]);
      });
    return () => {
      alive = false;
    };
  }, [paths, mode, a, b]);

  const modeOptions = useMemo(
    () => [
      { value: "prefix", label: t("添加前缀") },
      { value: "suffix", label: t("添加后缀(扩展名前)") },
      { value: "replace", label: t("查找替换") },
    ],
    [t],
  );

  return (
    <div className="modal-backdrop" onClick={onCancel}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <div className="modal__header">
          <h3>{t("批量重命名")}</h3>
        </div>
        <div className="modal__body">
          <label className="field">
            <span>{t("已选 {n} 项", { n: paths.length })}</span>
            <Select value={mode} options={modeOptions} onChange={setMode} />
          </label>

          {mode === "replace" ? (
            <div className="rename-fields">
              <label className="field">
                <span>{t("查找")}</span>
                <input
                  value={a}
                  autoFocus
                  onChange={(e) => setA(e.target.value)}
                  placeholder={t("要替换的文本")}
                />
              </label>
              <label className="field">
                <span>{t("替换为")}</span>
                <input
                  value={b}
                  onChange={(e) => setB(e.target.value)}
                  placeholder={t("留空即删除该文本")}
                />
              </label>
            </div>
          ) : (
            <label className="field">
              <span>{mode === "prefix" ? t("前缀") : t("后缀")}</span>
              <input
                value={a}
                autoFocus
                onChange={(e) => setA(e.target.value)}
                placeholder={mode === "prefix" ? "2026_" : "-v2"}
              />
            </label>
          )}

          <div className="rename-preview">
            <div className="rename-preview__head">
              {plan.length === 0
                ? t("没有会改动的对象")
                : t("将重命名 {n} 项:", { n: plan.length })}
            </div>
            {plan.slice(0, 20).map((p) => (
              <div className="rename-preview__row" key={p.from}>
                <span className="rename-preview__old">{baseName(p.from)}</span>
                <span className="rename-preview__arrow">→</span>
                <span className="rename-preview__new">{baseName(p.to)}</span>
              </div>
            ))}
            {plan.length > 20 && (
              <div className="rename-preview__more">
                {t("…还有 {n} 项", { n: plan.length - 20 })}
              </div>
            )}
          </div>
        </div>
        <div className="modal__footer">
          <button className="btn" onClick={onCancel}>
            {t("取消")}
          </button>
          <button
            className="btn btn--primary"
            disabled={plan.length === 0}
            onClick={() => onApply(plan)}
          >
            {t("重命名")}
          </button>
        </div>
      </div>
    </div>
  );
}
