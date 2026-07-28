import { useState } from "react";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import { faSliders, faKeyboard, faPuzzlePiece } from "@fortawesome/free-solid-svg-icons";
import type { Settings } from "../types";
import { Select } from "./Select";
import { MattingPluginCard } from "./MattingPluginCard";
import { ShortcutsEditor } from "./ShortcutsEditor";
import { useI18n } from "../i18n";
import { LOCALES } from "../locales";
import type { Bindings } from "../shortcuts";
import { ACCENTS, type Accent } from "../accents";

type Tab = "general" | "shortcuts" | "plugins";

type ThemePref = "dark" | "light" | "system";

interface Props {
  settings: Settings;
  shortcuts: Bindings;
  onShortcutsChange: (next: Bindings) => void;
  themePref: ThemePref;
  onThemeChange: (t: ThemePref) => void;
  accent: Accent;
  onAccentChange: (a: Accent) => void;
  onSave: (settings: Settings) => void;
  onClose: () => void;
}

export function SettingsDialog({
  settings,
  shortcuts,
  onShortcutsChange,
  themePref,
  onThemeChange,
  accent,
  onAccentChange,
  onSave,
  onClose,
}: Props) {
  const { t, locale, setLocale } = useI18n();
  const [minutes, setMinutes] = useState(
    Math.round(settings.share_expiry_secs / 60),
  );
  const [concurrency, setConcurrency] = useState(settings.concurrency);
  const [rateLimit, setRateLimit] = useState(
    settings.rate_limit_kib_per_sec ?? 0,
  );
  const valid =
    minutes >= 1 && concurrency >= 1 && concurrency <= 10 && rateLimit >= 0;
  const [tab, setTab] = useState<Tab>("general");

  const tabs: { id: Tab; label: string; icon: typeof faSliders }[] = [
    { id: "general", label: t("基础"), icon: faSliders },
    { id: "shortcuts", label: t("快捷键"), icon: faKeyboard },
    { id: "plugins", label: t("插件"), icon: faPuzzlePiece },
  ];

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal modal--tabs" onClick={(e) => e.stopPropagation()}>
        <div className="modal__header">
          <h3>{t("设置")}</h3>
        </div>
        <div className="settings-tabs">
          {tabs.map((tb) => (
            <button
              key={tb.id}
              className={`settings-tab ${tab === tb.id ? "settings-tab--on" : ""}`}
              onClick={() => setTab(tb.id)}
            >
              <FontAwesomeIcon icon={tb.icon} />
              <span>{tb.label}</span>
            </button>
          ))}
        </div>
        <div className="modal__body">
          {tab === "general" && (
            <>
              <label className="field">
                <span>{t("语言")}</span>
                <Select
                  value={locale}
                  options={LOCALES.map((l) => ({ value: l.code, label: l.label }))}
                  onChange={setLocale}
                />
              </label>
              <label className="field">
                <span>{t("主题")}</span>
                <Select
                  value={themePref}
                  options={[
                    { value: "system", label: t("跟随系统") },
                    { value: "dark", label: t("深色") },
                    { value: "light", label: t("浅色") },
                  ]}
                  onChange={(v) => onThemeChange(v as ThemePref)}
                />
              </label>
              <label className="field">
                <span>{t("强调色")}</span>
                <div className="accent-swatches">
                  {ACCENTS.map((a) => (
                    <button
                      key={a.id}
                      type="button"
                      className={`accent-swatch ${accent === a.id ? "accent-swatch--on" : ""}`}
                      style={{ background: a.color }}
                      title={a.id}
                      onClick={() => onAccentChange(a.id)}
                    />
                  ))}
                </div>
              </label>
              <label className="field">
                <span>{t("分享链接有效期(分钟)")}</span>
                <input
                  type="number"
                  min={1}
                  value={minutes}
                  onChange={(e) => setMinutes(Number(e.target.value))}
                />
              </label>
              <label className="field">
                <span>{t("批量传输并发数(1–10)")}</span>
                <input
                  type="number"
                  min={1}
                  max={10}
                  value={concurrency}
                  onChange={(e) => setConcurrency(Number(e.target.value))}
                />
              </label>
              <label className="field">
                <span>{t("传输限速(KiB/秒,0 = 不限速)")}</span>
                <input
                  type="number"
                  min={0}
                  value={rateLimit}
                  onChange={(e) => setRateLimit(Number(e.target.value))}
                />
              </label>
            </>
          )}

          {tab === "shortcuts" && (
            <ShortcutsEditor bindings={shortcuts} onChange={onShortcutsChange} />
          )}

          {tab === "plugins" && <MattingPluginCard />}
        </div>
        <div className="modal__footer">
          <button className="btn" onClick={onClose}>
            {t("取消")}
          </button>
          <button
            className="btn btn--primary"
            disabled={!valid}
            onClick={() =>
              onSave({
                share_expiry_secs: minutes * 60,
                concurrency,
                rate_limit_kib_per_sec: rateLimit,
              })
            }
          >
            {t("保存")}
          </button>
        </div>
      </div>
    </div>
  );
}
