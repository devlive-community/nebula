import type { Entry } from "../types";
import { formatBytes } from "../util";

interface Props {
  entries: Entry[];
  loading: boolean;
  onOpenDir: (entry: Entry) => void;
  onDownload: (entry: Entry) => void;
  onDelete: (entry: Entry) => void;
}

export function FileList({ entries, loading, onOpenDir, onDownload, onDelete }: Props) {
  if (loading) {
    return <div className="filelist__state">加载中…</div>;
  }
  if (entries.length === 0) {
    return <div className="filelist__state">这里空空如也</div>;
  }

  return (
    <div className="filelist">
      <div className="filelist__head">
        <span className="col-name">名称</span>
        <span className="col-size">大小</span>
        <span className="col-modified">修改时间</span>
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
                <span className="row__icon">{isDir ? "📁" : "📄"}</span>
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
                      ⬇
                    </button>
                    <button
                      className="icon-btn icon-btn--danger"
                      title="删除"
                      onClick={() => onDelete(entry)}
                    >
                      🗑
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
