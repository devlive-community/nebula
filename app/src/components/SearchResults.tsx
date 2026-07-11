import { useEffect, useState } from "react";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import { faFile, faXmark } from "@fortawesome/free-solid-svg-icons";
import type { Entry } from "../types";
import { formatBytes } from "../util";
import { useIncremental } from "../hooks";

interface Props {
  query: string;
  root: string;
  results: Entry[];
  loading: boolean;
  truncated: boolean;
  minSize: number;
  ext: string;
  onFilter: (minSize: number, ext: string) => void;
  onOpen: (entry: Entry) => void;
  onClear: () => void;
}

/** 最小大小预设(字节)。 */
const SIZE_PRESETS: [string, number][] = [
  ["不限大小", 0],
  ["> 1 MB", 1024 * 1024],
  ["> 10 MB", 10 * 1024 * 1024],
  ["> 100 MB", 100 * 1024 * 1024],
  ["> 1 GB", 1024 * 1024 * 1024],
];

/** 递归搜索的结果视图:显示命中文件的完整路径,点击跳到其所在目录。 */
export function SearchResults({
  query,
  root,
  results,
  loading,
  truncated,
  minSize,
  ext,
  onFilter,
  onOpen,
  onClear,
}: Props) {
  const { shown, onScroll } = useIncremental(results);
  const [extInput, setExtInput] = useState(ext);
  useEffect(() => setExtInput(ext), [ext]);

  return (
    <div className="search-results">
      <div className="search-results__head">
        <span className="search-results__title">
          在 <b>{root || "/"}</b> 下搜索 “{query}”
          {!loading && <> · {results.length} 条{truncated ? "(已达上限)" : ""}</>}
        </span>
        <button className="btn" onClick={onClear} title="退出搜索">
          <FontAwesomeIcon icon={faXmark} /> 退出搜索
        </button>
      </div>

      <div className="search-results__filters">
        <select
          value={minSize}
          onChange={(e) => onFilter(Number(e.target.value), extInput)}
        >
          {SIZE_PRESETS.map(([label, v]) => (
            <option key={v} value={v}>
              {label}
            </option>
          ))}
        </select>
        <input
          className="search-results__ext"
          placeholder="扩展名,如 jpg(回车应用)"
          value={extInput}
          onChange={(e) => setExtInput(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") onFilter(minSize, extInput);
          }}
          onBlur={() => {
            if (extInput.trim() !== ext.trim()) onFilter(minSize, extInput);
          }}
        />
      </div>

      <div className="search-results__body" onScroll={onScroll}>
        {loading ? (
          <div className="filelist__state">搜索中…</div>
        ) : results.length === 0 ? (
          <div className="filelist__state">没有匹配的文件</div>
        ) : (
          shown.map((entry) => (
            <button
              key={entry.path}
              className="search-results__row"
              onClick={() => onOpen(entry)}
              title="打开所在目录"
            >
              <FontAwesomeIcon icon={faFile} className="search-results__icon" />
              <span className="search-results__path">{entry.path}</span>
              <span className="search-results__size">{formatBytes(entry.size)}</span>
            </button>
          ))
        )}
      </div>
    </div>
  );
}
