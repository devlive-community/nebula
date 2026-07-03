import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import {
  faDownload,
  faFile,
  faFolder,
  faSortDown,
  faSortUp,
  faTrash,
} from "@fortawesome/free-solid-svg-icons";
import type { Entry } from "../types";
import { formatBytes } from "../util";

type SortKey = "name" | "size" | "modified";

interface Props {
  entries: Entry[];
  loading: boolean;
  sortKey: SortKey;
  sortDir: "asc" | "desc";
  onSort: (key: SortKey) => void;
  onOpenDir: (entry: Entry) => void;
  onDownload: (entry: Entry) => void;
  onDelete: (entry: Entry) => void;
}

export function FileList({
  entries,
  loading,
  sortKey,
  sortDir,
  onSort,
  onOpenDir,
  onDownload,
  onDelete,
}: Props) {
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
              <span className="col-modified">{entry.last_modified ?? "—"}</span>
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
