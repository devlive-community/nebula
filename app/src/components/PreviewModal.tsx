import { useState } from "react";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import { faXmark, faCopy, faTextWidth } from "@fortawesome/free-solid-svg-icons";
import { useI18n } from "../i18n";

interface Props {
  name: string;
  kind: "image" | "video" | "audio" | "pdf" | "text";
  url?: string;
  text?: string;
  truncated?: boolean;
  onClose: () => void;
}

/** 对象预览弹窗:图片 / 视频 / 音频 / PDF 用预签名链接加载,文本类展示后端读回的内容。 */
export function PreviewModal({
  name,
  kind,
  url,
  text,
  truncated,
  onClose,
}: Props) {
  const { t } = useI18n();
  const [wrap, setWrap] = useState(false);
  const [copied, setCopied] = useState(false);
  const lines = kind === "text" ? (text ?? "").split("\n") : [];
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(text ?? "");
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      /* 忽略剪贴板失败 */
    }
  };
  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div
        className={`preview ${
          kind === "text"
            ? "preview--text"
            : kind === "pdf"
              ? "preview--pdf"
              : kind === "audio"
                ? "preview--audio"
                : ""
        }`}
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
        ) : kind === "audio" ? (
          <div className="preview__audio">
            <audio src={url} controls autoPlay />
          </div>
        ) : kind === "pdf" ? (
          <iframe className="preview__pdf" src={url} title={name} />
        ) : (
          <div className="preview__textwrap">
            <div className="preview__texttools">
              <span className="preview__lines">{t("{n} 行", { n: String(lines.length) })}</span>
              <div className="preview__spacer" />
              <button
                className={`preview__tbtn ${wrap ? "preview__tbtn--on" : ""}`}
                onClick={() => setWrap((w) => !w)}
                title={t("自动换行")}
              >
                <FontAwesomeIcon icon={faTextWidth} />
              </button>
              <button className="preview__tbtn" onClick={copy} title={t("复制全部")}>
                <FontAwesomeIcon icon={faCopy} /> {copied ? t("已复制") : ""}
              </button>
            </div>
            <div className={`preview__code ${wrap ? "preview__code--wrap" : ""}`}>
              {lines.map((line, i) => (
                <div className="preview__codeline" key={i}>
                  <span className="preview__ln">{i + 1}</span>
                  <span className="preview__lc">{line || " "}</span>
                </div>
              ))}
            </div>
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
