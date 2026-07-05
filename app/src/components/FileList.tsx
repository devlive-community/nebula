import { useState } from "react";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import {
  faFile,
  faFolder,
  faSortDown,
  faSortUp,
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
  onOpenFile: (entry: Entry) => void;
  onOpenDetails: (entry: Entry) => void;
  onContext: (entry: Entry, x: number, y: number) => void;
  onDragStartFile: (entry: Entry) => void;
  onDropDir: (dir: Entry) => void;
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
  onOpenFile,
  onOpenDetails,
  onContext,
  onDragStartFile,
  onDropDir,
}: Props) {
  const [dropTarget, setDropTarget] = useState<string | null>(null);
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
      </div>
      <div className="filelist__body">
        {entries.map((entry) => {
          const isDir = entry.kind === "directory";
          return (
            <div
              key={entry.path}
              className={`row ${isDir ? "row--dir" : ""} ${
                dropTarget === entry.path ? "row--drop" : ""
              }`}
              draggable={!isDir}
              onDragStart={() => !isDir && onDragStartFile(entry)}
              onDragOver={
                isDir
                  ? (e) => {
                      e.preventDefault();
                      setDropTarget(entry.path);
                    }
                  : undefined
              }
              onDragLeave={isDir ? () => setDropTarget(null) : undefined}
              onDrop={
                isDir
                  ? (e) => {
                      e.preventDefault();
                      setDropTarget(null);
                      onDropDir(entry);
                    }
                  : undefined
              }
              onDoubleClick={() => (isDir ? onOpenDir(entry) : onOpenFile(entry))}
              onContextMenu={(e) => {
                e.preventDefault();
                onContext(entry, e.clientX, e.clientY);
              }}
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
                <button
                  className="row__name-link"
                  onClick={() =>
                    isDir ? onOpenDir(entry) : onOpenDetails(entry)
                  }
                >
                  {entry.name}
                </button>
              </span>
              <span className="col-size">{isDir ? "—" : formatBytes(entry.size)}</span>
              <span className="col-modified">
                {isDir ? "—" : formatDate(entry.last_modified)}
              </span>
            </div>
          );
        })}
      </div>
    </div>
  );
}
