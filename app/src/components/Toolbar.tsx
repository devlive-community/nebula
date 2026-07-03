import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import {
  faArrowUp,
  faRotateRight,
  faUpload,
} from "@fortawesome/free-solid-svg-icons";

interface Props {
  canGoUp: boolean;
  canUpload: boolean;
  busy: boolean;
  onUp: () => void;
  onRefresh: () => void;
  onUpload: () => void;
}

export function Toolbar({ canGoUp, canUpload, busy, onUp, onRefresh, onUpload }: Props) {
  return (
    <div className="toolbar">
      <button className="btn" disabled={!canGoUp} onClick={onUp} title="上一层">
        <FontAwesomeIcon icon={faArrowUp} /> 上一层
      </button>
      <button className="btn" onClick={onRefresh} title="刷新">
        <FontAwesomeIcon icon={faRotateRight} /> 刷新
      </button>
      <div className="toolbar__spacer" />
      {busy && <span className="toolbar__busy">处理中…</span>}
      <button
        className="btn btn--primary"
        disabled={!canUpload}
        onClick={onUpload}
        title="上传文件到当前目录"
      >
        <FontAwesomeIcon icon={faUpload} /> 上传
      </button>
    </div>
  );
}
