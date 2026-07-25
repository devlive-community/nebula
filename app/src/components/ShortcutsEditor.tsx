import { useEffect, useState } from "react";
import { useI18n } from "../i18n";
import {
  SHORTCUT_ACTIONS,
  eventToBinding,
  formatBinding,
  resolveBindings,
  type Bindings,
} from "../shortcuts";

interface Props {
  bindings: Bindings;
  onChange: (next: Bindings) => void;
}

/** 自定义快捷键编辑器:列出每个动作,点「录制」后按下组合键即可重绑,可恢复默认。 */
export function ShortcutsEditor({ bindings, onChange }: Props) {
  const { t } = useI18n();
  const resolved = resolveBindings(bindings);
  const [recording, setRecording] = useState<string | null>(null);

  // 录制中:捕获下一个组合键并写入该动作。
  useEffect(() => {
    if (!recording) return;
    const onKey = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.key === "Escape") {
        setRecording(null);
        return;
      }
      const b = eventToBinding(e);
      if (!b) return; // 纯修饰键,继续等
      onChange({ ...bindings, [recording]: b });
      setRecording(null);
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [recording, bindings, onChange]);

  const reset = (id: string) => {
    const next = { ...bindings };
    delete next[id];
    onChange(next);
  };

  return (
    <div className="shortcuts">
      {SHORTCUT_ACTIONS.map((a) => {
        const isDefault = !bindings[a.id] || bindings[a.id] === a.def;
        return (
          <div className="shortcuts__row" key={a.id}>
            <span className="shortcuts__label">{t(a.label)}</span>
            <button
              className={`shortcuts__key ${recording === a.id ? "shortcuts__key--rec" : ""}`}
              onClick={() => setRecording(recording === a.id ? null : a.id)}
              title={t("点击后按下新的组合键")}
            >
              {recording === a.id ? t("按下按键…") : formatBinding(resolved[a.id])}
            </button>
            <button
              className="shortcuts__reset"
              disabled={isDefault}
              onClick={() => reset(a.id)}
              title={t("恢复默认")}
            >
              ↺
            </button>
          </div>
        );
      })}
    </div>
  );
}
