import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { relaunch } from "@tauri-apps/plugin-process";
import * as api from "../api";
import { useI18n } from "../i18n";

/**
 * 设置里的「AI 抠图」插件卡片:显示状态,启用时下载模型 + 运行时库(进度条),
 * 完成后提示重启生效;已安装可卸载。
 */
export function MattingPluginCard() {
  const { t } = useI18n();
  const [supported, setSupported] = useState(true);
  const [installed, setInstalled] = useState<boolean | null>(null);
  const [busy, setBusy] = useState(false);
  const [progress, setProgress] = useState<{ done: number; total: number } | null>(
    null,
  );
  const [needRestart, setNeedRestart] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const unlisten = useRef<(() => void) | null>(null);

  useEffect(() => {
    api.mattingSupported().then(setSupported).catch(() => setSupported(true));
    api.mattingInstalled().then(setInstalled).catch(() => setInstalled(false));
    return () => unlisten.current?.();
  }, []);

  const install = async () => {
    setBusy(true);
    setError(null);
    setProgress({ done: 0, total: 0 });
    unlisten.current = await listen<[number, number]>("matting-progress", (e) => {
      setProgress({ done: e.payload[0], total: e.payload[1] });
    });
    try {
      await api.installMattingPlugin();
      setNeedRestart(true);
      setInstalled(true);
    } catch (e) {
      setError(String(e));
    } finally {
      unlisten.current?.();
      unlisten.current = null;
      setBusy(false);
      setProgress(null);
    }
  };

  const uninstall = async () => {
    setBusy(true);
    try {
      await api.uninstallMattingPlugin();
      setInstalled(false);
      setNeedRestart(true);
    } catch (e) {
      setError(String(e));
    }
    setBusy(false);
  };

  const pct =
    progress && progress.total > 0
      ? Math.round((progress.done / progress.total) * 100)
      : 0;
  const mb = (b: number) => (b / 1024 / 1024).toFixed(1);

  return (
    <div className="plugin-card">
      <div className="plugin-card__head">
        <div>
          <div className="plugin-card__title">{t("AI 抠图(去背景)")}</div>
          <div className="plugin-card__desc">
            {t("下载本地模型,一键去掉图片背景。约 20 MB,首次需下载。")}
          </div>
        </div>
        {!supported && (
          <span className="plugin-card__state">{t("本平台暂不支持")}</span>
        )}
        {supported && installed === false && !busy && (
          <button className="btn btn--primary" onClick={install}>
            {t("启用")}
          </button>
        )}
        {installed === true && !busy && (
          <button className="btn" onClick={uninstall}>
            {t("卸载")}
          </button>
        )}
        {busy && <span className="plugin-card__state">{t("处理中…")}</span>}
      </div>

      {progress && (
        <div className="plugin-card__progress">
          <div className="plugin-card__bar">
            <div className="plugin-card__fill" style={{ width: `${pct}%` }} />
          </div>
          <span className="plugin-card__pct">
            {pct}%{progress.total > 0 ? ` · ${mb(progress.done)}/${mb(progress.total)} MB` : ""}
          </span>
        </div>
      )}

      {needRestart && (
        <div className="plugin-card__restart">
          {t("安装完成,重启应用后生效。")}
          <button className="btn btn--primary" onClick={() => void relaunch()}>
            {t("立即重启")}
          </button>
        </div>
      )}

      {error && <div className="plugin-card__error">{error}</div>}
    </div>
  );
}
