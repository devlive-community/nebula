import { useState } from "react";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import {
  faDatabase,
  faFile,
  faFolder,
  faSortDown,
  faSortUp,
} from "@fortawesome/free-solid-svg-icons";
import type { Entry } from "../types";
import { formatBytes, formatDate, isBucket } from "../util";
import { useI18n } from "../i18n";
import { useIncremental } from "../hooks";
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
  onToggleSelect: (path: string, shift?: boolean) => void;
  onToggleSelectAll: () => void;
  onOpenDir: (entry: Entry) => void;
  onOpenFile: (entry: Entry) => void;
  onOpenDetails: (entry: Entry) => void;
  onContext: (entry: Entry, x: number, y: number) => void;
  onDragStartFile: (entry: Entry) => void;
  onDropDir: (dir: Entry) => void;
  onReachEnd?: () => void;
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
  onReachEnd,
}: Props) {
  const [dropTarget, setDropTarget] = useState<string | null>(null);
  const { shown, shownCount, total, hasMore, onScroll } = useIncremental(
    entries,
    120,
    onReachEnd,
  );
  const { t } = useI18n();
  const someSelected = entries.some(
    (e) => e.kind === "file" && selected.has(e.path),
  );

  if (loading) {
    return <div className="filelist__state">{t("加载中…")}</div>;
  }
  if (entries.length === 0) {
    return <div className="filelist__state">{t("这里空空如也")}</div>;
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
            title={t("全选 / 取消全选")}
            onChange={onToggleSelectAll}
          />
        </span>
        {header("name", t("名称"), "col-name")}
        {header("size", t("大小"), "col-size")}
        {header("modified", t("修改时间"), "col-modified")}
      </div>
      <div className="filelist__body" onScroll={onScroll}>
        {shown.map((entry) => {
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
                    onChange={(shift) => onToggleSelect(entry.path, shift)}
                  />
                )}
              </span>
              <span className="col-name">
                <span className="row__icon">
                  <FontAwesomeIcon
                    icon={isBucket(entry) ? faDatabase : isDir ? faFolder : faFile}
                  />
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
        {hasMore && (
          <div className="list-more">
            {t("下滑加载更多 · 已显示 {shown} / {total}", {
              shown: shownCount,
              total,
            })}
          </div>
        )}
      </div>
    </div>
  );
}
