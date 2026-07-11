import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import { faRotateRight, faXmark } from "@fortawesome/free-solid-svg-icons";
import type { TransferItem } from "../types";
import { formatBytes, formatDuration } from "../util";
import { useI18n } from "../i18n";

/** 进度以字节计的传输种类(可显示速度 / ETA);文件夹类以文件数计,不显示。 */
const BYTE_KINDS = new Set(["上传", "下载", "迁移"]);

interface Props {
  items: TransferItem[];
  onClear: () => void;
  onRetry: (id: string) => void;
  onCancel: (id: string) => void;
}

/** 底部传输任务面板:每个上传 / 下载独立显示进度与状态。 */
export function TransferPanel({ items, onClear, onRetry, onCancel }: Props) {
  const { t } = useI18n();
  const active = items.filter((i) => i.status === "active").length;
  const hasFinished = items.some((i) => i.status !== "active");

  return (
    <div className="transfers">
      <div className="transfers__head">
        <span className="transfers__title">
          {t("传输")}
          {active > 0 ? t(" · 进行中 {n}", { n: active }) : ""}
        </span>
        <button
          className="transfers__clear"
          onClick={onClear}
          disabled={!hasFinished}
        >
          {t("清除已完成")}
        </button>
      </div>
      <div className="transfers__list">
        {items.map((i) => {
          const pct =
            i.status === "done"
              ? 100
              : i.total > 0
                ? Math.round((i.done / i.total) * 100)
                : 0;
          // 进行中的字节类传输:百分比后追加速度与 ETA(如 45% · 3.2 MB/s · ~12s)。
          let active = `${pct}%`;
          const speed = i.speed ?? 0;
          if (BYTE_KINDS.has(i.kind) && speed > 0) {
            active += ` · ${formatBytes(speed)}/s`;
            if (i.total > i.done) {
              const eta = formatDuration((i.total - i.done) / speed);
              if (eta) active += ` · ~${eta}`;
            }
          }
          // 可从面板重跑的种类;迁移需带源端信息(重启后丢失 → 不可重试)。
          const retryable =
            i.kind === "上传" ||
            i.kind === "下载" ||
            i.kind === "下载文件夹" ||
            ((i.kind === "迁移" || i.kind === "迁移文件夹") && !!i.srcAccount);
          const label =
            i.status === "error"
              ? t("失败")
              : i.status === "cancelled"
                ? t("已取消")
                : i.status === "interrupted"
                  ? t("已中断")
                  : i.status === "done"
                    ? t("完成")
                    : active;
          return (
            <div className="transfers__item" key={i.id}>
              <div className="transfers__row">
                <span className="transfers__kind">{t(i.kind)}</span>
                <span className="transfers__name">{i.name}</span>
                <span
                  className={`transfers__status ${
                    i.status === "error" ? "transfers__status--error" : ""
                  }`}
                >
                  {label}
                </span>
                {i.status === "active" && (
                  <button
                    className="transfers__retry"
                    title={t("取消")}
                    onClick={() => onCancel(i.id)}
                  >
                    <FontAwesomeIcon icon={faXmark} />
                  </button>
                )}
                {(i.status === "error" ||
                  i.status === "cancelled" ||
                  i.status === "interrupted") &&
                  retryable && (
                    <button
                      className="transfers__retry"
                      title={i.status === "error" ? t("重试") : t("继续")}
                      onClick={() => onRetry(i.id)}
                    >
                      <FontAwesomeIcon icon={faRotateRight} />
                    </button>
                  )}
              </div>
              <div className="transfers__track">
                <div
                  className={`transfers__fill ${
                    i.status === "error" ? "transfers__fill--error" : ""
                  }`}
                  style={{ width: `${i.status === "error" ? 100 : pct}%` }}
                />
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}
