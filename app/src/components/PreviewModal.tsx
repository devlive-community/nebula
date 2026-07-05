import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import { faXmark } from "@fortawesome/free-solid-svg-icons";

interface Props {
  url: string;
  name: string;
  kind: "image" | "video";
  onClose: () => void;
}

/** 图片 / 视频预览弹窗(用预签名链接加载)。 */
export function PreviewModal({ url, name, kind, onClose }: Props) {
  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="preview" onClick={(e) => e.stopPropagation()}>
        <div className="preview__bar">
          <span className="preview__name">{name}</span>
          <button className="preview__close" onClick={onClose} title="关闭">
            <FontAwesomeIcon icon={faXmark} />
          </button>
        </div>
        {kind === "image" ? (
          <img className="preview__media" src={url} alt={name} />
        ) : (
          <video className="preview__media" src={url} controls autoPlay />
        )}
      </div>
    </div>
  );
}
