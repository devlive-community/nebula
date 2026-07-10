import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import { faRotateRight, faXmark } from "@fortawesome/free-solid-svg-icons";
import type { TransferItem } from "../types";

interface Props {
  items: TransferItem[];
  onClear: () => void;
  onRetry: (id: string) => void;
  onCancel: (id: string) => void;
}

/** 底部传输任务面板:每个上传 / 下载独立显示进度与状态。 */
export function TransferPanel({ items, onClear, onRetry, onCancel }: Props) {
  const active = items.filter((i) => i.status === "active").length;
  const hasFinished = items.some((i) => i.status !== "active");

  return (
    <div className="transfers">
      <div className="transfers__head">
        <span className="transfers__title">
          传输{active > 0 ? ` · 进行中 ${active}` : ""}
        </span>
        <button
          className="transfers__clear"
          onClick={onClear}
          disabled={!hasFinished}
        >
          清除已完成
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
          const label =
            i.status === "error"
              ? "失败"
              : i.status === "cancelled"
                ? "已取消"
                : i.status === "done"
                  ? "完成"
                  : `${pct}%`;
          return (
            <div className="transfers__item" key={i.id}>
              <div className="transfers__row">
                <span className="transfers__kind">{i.kind}</span>
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
                    title="取消"
                    onClick={() => onCancel(i.id)}
                  >
                    <FontAwesomeIcon icon={faXmark} />
                  </button>
                )}
                {(i.status === "error" || i.status === "cancelled") &&
                  i.kind !== "迁移" &&
                  i.kind !== "迁移文件夹" && (
                    <button
                      className="transfers__retry"
                      title={i.status === "cancelled" ? "继续" : "重试"}
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
