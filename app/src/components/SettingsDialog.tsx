import { useState } from "react";
import type { Settings } from "../types";
import { Select } from "./Select";
import { useI18n } from "../i18n";

interface Props {
  settings: Settings;
  onSave: (settings: Settings) => void;
  onClose: () => void;
}

export function SettingsDialog({ settings, onSave, onClose }: Props) {
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

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <div className="modal__header">
          <h3>{t("设置")}</h3>
        </div>
        <div className="modal__body">
          <label className="field">
            <span>{t("语言")}</span>
            <Select
              value={locale}
              options={[
                { value: "zh", label: "中文" },
                { value: "en", label: "English" },
              ]}
              onChange={(v) => setLocale(v as "zh" | "en")}
            />
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
