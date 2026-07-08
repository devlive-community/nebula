import { useState } from "react";
import { relaunch } from "@tauri-apps/plugin-process";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import { faXmark, faCloudArrowDown } from "@fortawesome/free-solid-svg-icons";
import type { Update } from "../update";

interface Props {
  update: Update;
  onClose: () => void;
}

/** 发现新版本时的弹窗:展示版本与更新说明,下载并安装后自动重启。 */
export function UpdateDialog({ update, onClose }: Props) {
  const [phase, setPhase] = useState<"idle" | "downloading" | "error">("idle");
  const [pct, setPct] = useState(0);
  const [err, setErr] = useState("");

  const install = async () => {
    setPhase("downloading");
    setErr("");
    setPct(0);
    try {
      let total = 0;
      let got = 0;
      await update.downloadAndInstall((e) => {
        if (e.event === "Started") {
          total = e.data.contentLength ?? 0;
        } else if (e.event === "Progress") {
          got += e.data.chunkLength;
          if (total > 0) setPct(Math.round((got / total) * 100));
        } else if (e.event === "Finished") {
          setPct(100);
        }
      });
      // 安装完成后重启应用以生效。
      await relaunch();
    } catch (e) {
      setErr(String(e));
      setPhase("error");
    }
  };

  const downloading = phase === "downloading";

  return (
    <div className="modal-backdrop" onClick={downloading ? undefined : onClose}>
      <div className="modal modal--update" onClick={(e) => e.stopPropagation()}>
        <div className="modal__header">
          <h3>发现新版本</h3>
          {!downloading && (
            <button className="modal__close" onClick={onClose}>
              <FontAwesomeIcon icon={faXmark} />
            </button>
          )}
        </div>

        <div className="modal__body update">
          <div className="update__ver">
            <span className="update__badge">v{update.currentVersion}</span>
            <span className="update__arrow">→</span>
            <span className="update__badge update__badge--new">
              v{update.version}
            </span>
          </div>

          {update.body && (
            <div className="update__notes">{update.body}</div>
          )}

          {downloading && (
            <div className="update__progress">
              <div className="update__bar">
                <div className="update__bar-fill" style={{ width: `${pct}%` }} />
              </div>
              <span className="update__pct">下载中 {pct}%</span>
            </div>
          )}

          {phase === "error" && <div className="update__error">{err}</div>}
        </div>

        <div className="modal__footer">
          {!downloading && (
            <button className="btn" onClick={onClose}>
              稍后
            </button>
          )}
          <button
            className="btn btn--primary"
            disabled={downloading}
            onClick={install}
          >
            <FontAwesomeIcon icon={faCloudArrowDown} />
            {phase === "error" ? " 重试" : downloading ? " 更新中…" : " 立即更新"}
          </button>
        </div>
      </div>
    </div>
  );
}
