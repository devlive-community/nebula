import type { StorageBreakdown } from "../types";
import { formatBytes, storageLabel } from "../util";
import { useI18n } from "../i18n";

interface Props {
  name: string;
  data: StorageBreakdown | null;
  onClose: () => void;
}

/** 文件夹 / Bucket 统计弹窗:总量 + 按存储类型的分布(数量 / 大小 / 占比)。 */
export function StatsDialog({ name, data, onClose }: Props) {
  const { t } = useI18n();
  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <div className="modal__header">
          <h3>
            {t("统计信息")} · {name}
          </h3>
        </div>
        <div className="modal__body">
          {!data ? (
            <p className="tags__hint">{t("统计中…")}</p>
          ) : (
            <>
              <p className="stats__total">
                {t("共 {files} 个文件 · {size}", {
                  files: data.files,
                  size: formatBytes(data.bytes),
                })}
                {data.truncated ? ` · ${t("(超大目录,统计可能偏小)")}` : ""}
              </p>
              {data.classes.length > 0 && (
                <table className="stats__table">
                  <thead>
                    <tr>
                      <th>{t("存储类型")}</th>
                      <th>{t("文件数")}</th>
                      <th>{t("大小")}</th>
                      <th>{t("占比")}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {data.classes.map((c) => {
                      const pct =
                        data.bytes > 0
                          ? Math.round((c.bytes / data.bytes) * 100)
                          : 0;
                      return (
                        <tr key={c.class}>
                          <td>{storageLabel(c.class)}</td>
                          <td>{c.files}</td>
                          <td>{formatBytes(c.bytes)}</td>
                          <td>
                            <div className="stats__bar">
                              <div
                                className="stats__barfill"
                                style={{ width: `${pct}%` }}
                              />
                              <span>{pct}%</span>
                            </div>
                          </td>
                        </tr>
                      );
                    })}
                  </tbody>
                </table>
              )}
            </>
          )}
        </div>
        <div className="modal__footer">
          <button className="btn btn--primary" onClick={onClose}>
            {t("关闭")}
          </button>
        </div>
      </div>
    </div>
  );
}
