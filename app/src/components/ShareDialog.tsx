import { useState } from "react";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import { faCheck, faCopy } from "@fortawesome/free-solid-svg-icons";

interface Props {
  url: string;
  /** 有效期(分钟),仅用于提示文案。 */
  minutes: number;
  onClose: () => void;
}

/** 展示预签名分享链接并支持一键复制。 */
export function ShareDialog({ url, minutes, onClose }: Props) {
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
          <h3>分享链接</h3>
        </div>
        <div className="modal__body">
          <div className="share__url">{url}</div>
          <p className="share__note">此链接 {minutes} 分钟后失效。</p>
        </div>
        <div className="modal__footer">
          <button className="btn" onClick={onClose}>
            关闭
          </button>
          <button className="btn btn--primary" onClick={copy}>
            <FontAwesomeIcon icon={copied ? faCheck : faCopy} />
            {copied ? " 已复制" : " 复制链接"}
          </button>
        </div>
      </div>
    </div>
  );
}
