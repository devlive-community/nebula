import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import { faFile, faFolder } from "@fortawesome/free-solid-svg-icons";
import type { Entry } from "../types";
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
}: Props) {
  if (loading) {
    return <div className="filelist__state">加载中…</div>;
  }
  if (entries.length === 0) {
    return <div className="filelist__state">这里空空如也</div>;
  }

  return (
    <div className="grid">
      {entries.map((entry) => {
        const isDir = entry.kind === "directory";
        const thumb = thumbs[entry.path];
        return (
          <div
            key={entry.path}
            className={`card ${selected.has(entry.path) ? "card--selected" : ""}`}
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
                <FontAwesomeIcon icon={faFolder} className="card__icon card__icon--dir" />
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
    </div>
  );
}
