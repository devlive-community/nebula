import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import {
  faCopy,
  faDownload,
  faFile,
  faFolder,
  faPen,
  faSortDown,
  faSortUp,
  faTrash,
} from "@fortawesome/free-solid-svg-icons";
import type { Entry } from "../types";
import { formatBytes, formatDate } from "../util";
import { Checkbox } from "./Checkbox";

type SortKey = "name" | "size" | "modified";

interface Props {
  entries: Entry[];
  loading: boolean;
  sortKey: SortKey;
  sortDir: "asc" | "desc";
  selected: Set<string>;
  allSelected: boolean;
  onSort: (key: SortKey) => void;
  onToggleSelect: (path: string) => void;
  onToggleSelectAll: () => void;
  onOpenDir: (entry: Entry) => void;
  onDownload: (entry: Entry) => void;
  onRename: (entry: Entry) => void;
  onMoveCopy: (entry: Entry) => void;
  onDelete: (entry: Entry) => void;
}

export function FileList({
  entries,
  loading,
  sortKey,
  sortDir,
  selected,
  allSelected,
  onSort,
  onToggleSelect,
  onToggleSelectAll,
  onOpenDir,
  onDownload,
  onRename,
  onMoveCopy,
  onDelete,
}: Props) {
  const someSelected = entries.some(
    (e) => e.kind === "file" && selected.has(e.path),
  );

  if (loading) {
    return <div className="filelist__state">加载中…</div>;
  }
  if (entries.length === 0) {
    return <div className="filelist__state">这里空空如也</div>;
  }

  const header = (key: SortKey, label: string, className: string) => (
    <button className={`${className} col-sort`} onClick={() => onSort(key)}>
      {label}
      {sortKey === key && (
        <FontAwesomeIcon
          icon={sortDir === "asc" ? faSortUp : faSortDown}
          className="col-sort__icon"
        />
      )}
    </button>
  );

  return (
    <div className="filelist">
      <div className="filelist__head">
        <span className="col-check">
          <Checkbox
            checked={allSelected}
            indeterminate={someSelected && !allSelected}
            title="全选 / 取消全选"
            onChange={onToggleSelectAll}
          />
        </span>
        {header("name", "名称", "col-name")}
        {header("size", "大小", "col-size")}
        {header("modified", "修改时间", "col-modified")}
        <span className="col-actions" />
      </div>
      <div className="filelist__body">
        {entries.map((entry) => {
          const isDir = entry.kind === "directory";
          return (
            <div
              key={entry.path}
              className={`row ${isDir ? "row--dir" : ""}`}
              onDoubleClick={() => isDir && onOpenDir(entry)}
            >
              <span className="col-check">
                {!isDir && (
                  <Checkbox
                    checked={selected.has(entry.path)}
                    onChange={() => onToggleSelect(entry.path)}
                  />
                )}
              </span>
              <span className="col-name">
                <span className="row__icon">
                  <FontAwesomeIcon icon={isDir ? faFolder : faFile} />
                </span>
                {isDir ? (
                  <button className="row__name-link" onClick={() => onOpenDir(entry)}>
                    {entry.name}
                  </button>
                ) : (
                  <span className="row__name">{entry.name}</span>
                )}
              </span>
              <span className="col-size">{isDir ? "—" : formatBytes(entry.size)}</span>
              <span className="col-modified">
                {isDir ? "—" : formatDate(entry.last_modified)}
              </span>
              <span className="col-actions">
                {!isDir && (
                  <>
                    <button
                      className="icon-btn"
                      title="下载"
                      onClick={() => onDownload(entry)}
                    >
                      <FontAwesomeIcon icon={faDownload} />
                    </button>
                    <button
                      className="icon-btn"
                      title="重命名"
                      onClick={() => onRename(entry)}
                    >
                      <FontAwesomeIcon icon={faPen} />
                    </button>
                    <button
                      className="icon-btn"
                      title="复制 / 移动到"
                      onClick={() => onMoveCopy(entry)}
                    >
                      <FontAwesomeIcon icon={faCopy} />
                    </button>
                    <button
                      className="icon-btn icon-btn--danger"
                      title="删除"
                      onClick={() => onDelete(entry)}
                    >
                      <FontAwesomeIcon icon={faTrash} />
                    </button>
                  </>
                )}
              </span>
            </div>
          );
        })}
      </div>
    </div>
  );
}
