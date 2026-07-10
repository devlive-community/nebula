import { useState } from "react";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import { faDatabase, faFile, faFolder } from "@fortawesome/free-solid-svg-icons";
import type { Entry } from "../types";
import { useIncremental } from "../hooks";
import { isBucket } from "../util";
import { Checkbox } from "./Checkbox";

interface Props {
  entries: Entry[];
  loading: boolean;
  thumbs: Record<string, string>;
  selected: Set<string>;
  onToggleSelect: (path: string) => void;
  onOpenDir: (entry: Entry) => void;
  onOpenFile: (entry: Entry) => void;
  onContext: (entry: Entry, x: number, y: number) => void;
  onDragStartFile: (entry: Entry) => void;
  onDropDir: (dir: Entry) => void;
  onReachEnd?: () => void;
}

export function FileGrid({
  entries,
  loading,
  thumbs,
  selected,
  onToggleSelect,
  onOpenDir,
  onOpenFile,
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
  if (loading) {
    return <div className="filelist__state">加载中…</div>;
  }
  if (entries.length === 0) {
    return <div className="filelist__state">这里空空如也</div>;
  }

  return (
    <div className="grid" onScroll={onScroll}>
      {shown.map((entry) => {
        const isDir = entry.kind === "directory";
        const thumb = thumbs[entry.path];
        return (
          <div
            key={entry.path}
            className={`card ${selected.has(entry.path) ? "card--selected" : ""} ${
              dropTarget === entry.path ? "card--drop" : ""
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
            {!isDir && (
              <span className="card__check">
                <Checkbox
                  checked={selected.has(entry.path)}
                  onChange={() => onToggleSelect(entry.path)}
                />
              </span>
            )}
            <div className="card__thumb">
              {isDir ? (
                <FontAwesomeIcon
                  icon={isBucket(entry) ? faDatabase : faFolder}
                  className="card__icon card__icon--dir"
                />
              ) : thumb ? (
                <img className="card__img" src={thumb} alt={entry.name} loading="lazy" />
              ) : (
                <FontAwesomeIcon icon={faFile} className="card__icon" />
              )}
            </div>
            <div className="card__name" title={entry.name}>
              {entry.name}
            </div>
          </div>
        );
      })}
      {hasMore && (
        <div className="list-more">下滑加载更多 · 已显示 {shownCount} / {total}</div>
      )}
    </div>
  );
}
