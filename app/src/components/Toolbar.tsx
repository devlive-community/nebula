import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import {
  faArrowUp,
  faFolderOpen,
  faFolderPlus,
  faMagnifyingGlass,
  faRotateRight,
  faUpload,
} from "@fortawesome/free-solid-svg-icons";

interface Props {
  canGoUp: boolean;
  canUpload: boolean;
  busy: boolean;
  filter: string;
  onFilter: (value: string) => void;
  onUp: () => void;
  onRefresh: () => void;
  onUpload: () => void;
  onUploadFolder: () => void;
  onNewFolder: () => void;
}

export function Toolbar({
  canGoUp,
  canUpload,
  busy,
  filter,
  onFilter,
  onUp,
  onRefresh,
  onUpload,
  onUploadFolder,
  onNewFolder,
}: Props) {
  return (
    <div className="toolbar">
      <button className="btn" disabled={!canGoUp} onClick={onUp} title="上一层">
        <FontAwesomeIcon icon={faArrowUp} /> 上一层
      </button>
      <button className="btn" onClick={onRefresh} title="刷新">
        <FontAwesomeIcon icon={faRotateRight} /> 刷新
      </button>
      <div className="toolbar__search">
        <FontAwesomeIcon icon={faMagnifyingGlass} className="toolbar__search-icon" />
        <input
          className="toolbar__search-input"
          placeholder="过滤当前目录…"
          value={filter}
          onChange={(e) => onFilter(e.target.value)}
        />
      </div>
      <div className="toolbar__spacer" />
      {busy && <span className="toolbar__busy">处理中…</span>}
      <button
        className="btn"
        disabled={!canUpload}
        onClick={onNewFolder}
        title="在当前目录新建文件夹"
      >
        <FontAwesomeIcon icon={faFolderPlus} /> 新建文件夹
      </button>
      <button
        className="btn"
        disabled={!canUpload}
        onClick={onUploadFolder}
        title="上传文件夹到当前目录"
      >
        <FontAwesomeIcon icon={faFolderOpen} /> 上传文件夹
      </button>
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
