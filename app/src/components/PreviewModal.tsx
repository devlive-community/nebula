import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import { faXmark } from "@fortawesome/free-solid-svg-icons";
import { useI18n } from "../i18n";

interface Props {
  name: string;
  kind: "image" | "video" | "text";
  url?: string;
  text?: string;
  truncated?: boolean;
  onClose: () => void;
}

/** 对象预览弹窗:图片 / 视频用预签名链接加载,文本类展示后端读回的内容。 */
export function PreviewModal({
  name,
  kind,
  url,
  text,
  truncated,
  onClose,
}: Props) {
  const { t } = useI18n();
  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div
        className={`preview ${kind === "text" ? "preview--text" : ""}`}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="preview__bar">
          <span className="preview__name">{name}</span>
          <button className="preview__close" onClick={onClose} title={t("关闭")}>
            <FontAwesomeIcon icon={faXmark} />
          </button>
        </div>
        {kind === "image" ? (
          <img className="preview__media" src={url} alt={name} />
        ) : kind === "video" ? (
          <video className="preview__media" src={url} controls autoPlay />
        ) : (
          <div className="preview__textwrap">
            <pre className="preview__text">{text}</pre>
            {truncated && (
              <div className="preview__note">
                {t("内容较大,仅预览前 256 KB。")}
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
