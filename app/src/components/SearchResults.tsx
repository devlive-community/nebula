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
  onOpen: (entry: Entry) => void;
  onClear: () => void;
}

/** 递归搜索的结果视图:显示命中文件的完整路径,点击跳到其所在目录。 */
export function SearchResults({
  query,
  root,
  results,
  loading,
  truncated,
  onOpen,
  onClear,
}: Props) {
  const { shown, onScroll } = useIncremental(results);

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
