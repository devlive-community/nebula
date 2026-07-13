import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import {
  faDownload,
  faFile,
  faLink,
  faPen,
  faTags,
  faXmark,
} from "@fortawesome/free-solid-svg-icons";
import type { Entry } from "../types";
import { formatBytes, formatDate, guessType } from "../util";

interface Props {
  entry: Entry;
  onClose: () => void;
  onDownload: (entry: Entry) => void;
  onShare: (entry: Entry) => void;
  onEditType: (entry: Entry) => void;
  onEditTags: (entry: Entry) => void;
}

/** 右侧对象详情抽屉。 */
/** 各家云常见存储类型 → 友好中文标签;未收录的原样显示。 */
const STORAGE_LABELS: Record<string, string> = {
  STANDARD: "标准",
  IA: "低频访问",
  STANDARD_IA: "低频访问",
  WARM: "低频 (WARM)",
  ARCHIVE: "归档",
  COLD: "归档 (COLD)",
  GLACIER: "归档 (Glacier)",
  DEEP_ARCHIVE: "深度归档",
  COLD_ARCHIVE: "冷归档",
  INTELLIGENT_TIERING: "智能分层",
  REDUCED_REDUNDANCY: "低冗余",
};

const storageLabel = (sc: string | null): string =>
  sc ? (STORAGE_LABELS[sc.toUpperCase()] ?? sc) : "标准";

export function FileDetails({
  entry,
  onClose,
  onDownload,
  onShare,
  onEditType,
  onEditTags,
}: Props) {
  // 有真实 Content-Type 就显示它,否则按文件名推测。
  const contentType = entry.content_type ?? guessType(entry.name);
  const rows: [string, string][] = [
    ["名称", entry.name],
    ["路径", entry.path],
    ["大小", formatBytes(entry.size)],
    ["存储类型", storageLabel(entry.storage_class)],
    ["修改时间", formatDate(entry.last_modified)],
    ["ETag", entry.etag ?? "—"],
  ];

  return (
    <div className="details">
      <div className="details__header">
        <span className="details__title">详情</span>
        <button className="details__close" onClick={onClose} title="关闭">
          <FontAwesomeIcon icon={faXmark} />
        </button>
      </div>

      <div className="details__hero">
        <FontAwesomeIcon icon={faFile} className="details__hero-icon" />
        <span className="details__hero-name">{entry.name}</span>
      </div>

      <div className="details__rows">
        {rows.slice(0, 2).map(([label, value]) => (
          <div className="details__row" key={label}>
            <span className="details__label">{label}</span>
            <span className="details__value">{value}</span>
          </div>
        ))}
        <div className="details__row">
          <span className="details__label">类型</span>
          <span className="details__value">
            {contentType}
            <button
              className="details__edit"
              title="修改内容类型"
              onClick={() => onEditType(entry)}
            >
              <FontAwesomeIcon icon={faPen} />
            </button>
          </span>
        </div>
        {rows.slice(2).map(([label, value]) => (
          <div className="details__row" key={label}>
            <span className="details__label">{label}</span>
            <span className="details__value">{value}</span>
          </div>
        ))}
      </div>

      <div className="details__actions">
        <button className="btn btn--primary" onClick={() => onDownload(entry)}>
          <FontAwesomeIcon icon={faDownload} /> 下载
        </button>
        <button className="btn" onClick={() => onShare(entry)}>
          <FontAwesomeIcon icon={faLink} /> 分享链接
        </button>
        <button className="btn" onClick={() => onEditTags(entry)}>
          <FontAwesomeIcon icon={faTags} /> 标签
        </button>
      </div>
    </div>
  );
}
