import { useEffect, useState } from "react";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import { faFile, faXmark } from "@fortawesome/free-solid-svg-icons";
import type { Entry } from "../types";
import { formatBytes } from "../util";
import { useIncremental } from "../hooks";
import { Select } from "./Select";
import { useI18n } from "../i18n";

interface Props {
  query: string;
  root: string;
  results: Entry[];
  loading: boolean;
  truncated: boolean;
  minSize: number;
  ext: string;
  selected: Set<string>;
  onFilter: (minSize: number, ext: string) => void;
  onOpen: (entry: Entry) => void;
  onToggleSelect: (path: string, shift?: boolean) => void;
  onToggleSelectAll: () => void;
  /** 内容搜索时:路径 → 命中片段。 */
  snippets?: Record<string, string>;
  /** 是否内容搜索模式。 */
  contentMode: boolean;
  onToggleContent: () => void;
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
  selected,
  onFilter,
  onOpen,
  onToggleSelect,
  onToggleSelectAll,
  snippets,
  contentMode,
  onToggleContent,
  onClear,
}: Props) {
  const { t } = useI18n();
  const { shown, onScroll } = useIncremental(results);
  const [extInput, setExtInput] = useState(ext);
  useEffect(() => setExtInput(ext), [ext]);
  const allSelected =
    results.length > 0 && results.every((r) => selected.has(r.path));

  return (
    <div className="search-results">
      <div className="search-results__head">
        <span className="search-results__title">
          {t("在 {root} 下搜索", { root: root || "/" })} “{query}”
          {!loading && (
            <>
              {" "}
              · {t("{n} 条", { n: results.length })}
              {truncated ? t("(已达上限)") : ""}
            </>
          )}
        </span>
        <button className="btn" onClick={onClear} title={t("退出搜索")}>
          <FontAwesomeIcon icon={faXmark} /> {t("退出搜索")}
        </button>
      </div>

      <div className="search-results__filters">
        <button
          className={`btn ${contentMode ? "btn--primary" : ""}`}
          onClick={onToggleContent}
          title={t("在文本 / PDF 文件内容里搜索")}
        >
          {contentMode ? t("按内容搜索") : t("按名称搜索")}
        </button>
        {!contentMode && (
          <>
            <Select
              value={String(minSize)}
              options={SIZE_PRESETS.map(([label, v]) => ({
                value: String(v),
                label: t(label),
              }))}
              onChange={(v) => onFilter(Number(v), extInput)}
            />
            <input
              className="search-results__ext"
              placeholder={t("扩展名,如 jpg(回车应用)")}
              value={extInput}
              onChange={(e) => setExtInput(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") onFilter(minSize, extInput);
              }}
              onBlur={() => {
                if (extInput.trim() !== ext.trim()) onFilter(minSize, extInput);
              }}
            />
          </>
        )}
        {!loading && results.length > 0 && (
          <label className="search-results__selall">
            <input
              type="checkbox"
              checked={allSelected}
              onChange={onToggleSelectAll}
            />
            {t("全选")}
          </label>
        )}
      </div>

      <div className="search-results__body" onScroll={onScroll}>
        {loading ? (
          <div className="filelist__state">{t("搜索中…")}</div>
        ) : results.length === 0 ? (
          <div className="filelist__state">{t("没有匹配的文件")}</div>
        ) : (
          shown.map((entry) => (
            <div
              key={entry.path}
              className={`search-results__row ${
                selected.has(entry.path) ? "search-results__row--selected" : ""
              }`}
            >
              <input
                type="checkbox"
                className="search-results__check"
                checked={selected.has(entry.path)}
                onChange={(e) =>
                  onToggleSelect(
                    entry.path,
                    (e.nativeEvent as MouseEvent).shiftKey,
                  )
                }
                onClick={(e) => e.stopPropagation()}
              />
              <button
                className="search-results__open"
                onClick={() => onOpen(entry)}
                title={t("打开所在目录")}
              >
                <FontAwesomeIcon
                  icon={faFile}
                  className="search-results__icon"
                />
                <span className="search-results__path">{entry.path}</span>
                <span className="search-results__size">
                  {formatBytes(entry.size)}
                </span>
              </button>
              {snippets?.[entry.path] && (
                <div className="search-results__snippet">
                  {snippets[entry.path]}
                </div>
              )}
            </div>
          ))
        )}
      </div>
    </div>
  );
}
