import { useState } from "react";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import { faCheck, faCopy } from "@fortawesome/free-solid-svg-icons";
import { useI18n } from "../i18n";

interface Props {
  url: string;
  /** 有效期(分钟),仅用于提示文案。 */
  minutes: number;
  /** true = 预签名上传(PUT)链接;false = 下载分享链接。 */
  upload?: boolean;
  onClose: () => void;
}

/** 展示预签名分享 / 上传链接并支持一键复制。 */
export function ShareDialog({ url, minutes, upload, onClose }: Props) {
  const { t } = useI18n();
  const [copied, setCopied] = useState(false);

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(url);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      // 剪贴板不可用时忽略,用户可手动选中复制。
    }
  };

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <div className="modal__header">
          <h3>{upload ? t("上传链接") : t("分享链接")}</h3>
        </div>
        <div className="modal__body">
          <div className="share__url">{url}</div>
          <p className="share__note">
            {t("此链接 {minutes} 分钟后失效。", { minutes })}
          </p>
          {upload && (
            <p className="share__note">
              {t("持链接者用 PUT 直接上传即可,例如:")}
              <br />
              <code>curl -X PUT --upload-file 文件 "链接"</code>
            </p>
          )}
        </div>
        <div className="modal__footer">
          <button className="btn" onClick={onClose}>
            {t("关闭")}
          </button>
          <button className="btn btn--primary" onClick={copy}>
            <FontAwesomeIcon icon={copied ? faCheck : faCopy} />
            {copied ? ` ${t("已复制")}` : ` ${t("复制链接")}`}
          </button>
        </div>
      </div>
    </div>
  );
}
