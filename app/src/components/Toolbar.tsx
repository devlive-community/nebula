import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import {
  faArrowUp,
  faDatabase,
  faFolderOpen,
  faFolderPlus,
  faList,
  faMagnifyingGlass,
  faRotateRight,
  faTableCells,
  faUpload,
} from "@fortawesome/free-solid-svg-icons";
import { useI18n } from "../i18n";

interface Props {
  canGoUp: boolean;
  canUpload: boolean;
  busy: boolean;
  filter: string;
  view: "list" | "grid";
  canSearch: boolean;
  /** 是否在根层级(全部 Bucket)。true 时显示"新建 Bucket"。 */
  atRoot: boolean;
  onNewBucket: () => void;
  onFilter: (value: string) => void;
  onSearch: (query: string) => void;
  onUp: () => void;
  onRefresh: () => void;
  onToggleView: () => void;
  onUpload: () => void;
  onUploadFolder: () => void;
  onNewFolder: () => void;
}

export function Toolbar({
  canGoUp,
  canUpload,
  busy,
  filter,
  view,
  canSearch,
  atRoot,
  onNewBucket,
  onFilter,
  onSearch,
  onUp,
  onRefresh,
  onToggleView,
  onUpload,
  onUploadFolder,
  onNewFolder,
}: Props) {
  const { t } = useI18n();
  return (
    <div className="toolbar">
      <button
        className="btn"
        disabled={!canGoUp}
        onClick={onUp}
        title={t("上一层")}
      >
        <FontAwesomeIcon icon={faArrowUp} /> {t("上一层")}
      </button>
      <button className="btn" onClick={onRefresh} title={t("刷新")}>
        <FontAwesomeIcon icon={faRotateRight} /> {t("刷新")}
      </button>
      <div className="toolbar__search">
        <FontAwesomeIcon icon={faMagnifyingGlass} className="toolbar__search-icon" />
        <input
          className="toolbar__search-input"
          placeholder={
            canSearch ? t("过滤当前目录 / 回车递归搜索…") : t("过滤当前目录…")
          }
          value={filter}
          onChange={(e) => onFilter(e.target.value)}
          onKeyDown={(e) => {
            const q = filter.trim();
            if (e.key === "Enter" && canSearch && q) onSearch(q);
          }}
        />
      </div>
      <div className="toolbar__spacer" />
      {busy && <span className="toolbar__busy">{t("处理中…")}</span>}
      <button
        className="btn"
        onClick={onToggleView}
        title={view === "list" ? t("网格视图") : t("列表视图")}
      >
        <FontAwesomeIcon icon={view === "list" ? faTableCells : faList} />
      </button>
      {atRoot ? (
        <button
          className="btn btn--primary"
          onClick={onNewBucket}
          title={t("新建一个 Bucket")}
        >
          <FontAwesomeIcon icon={faDatabase} /> {t("新建 Bucket")}
        </button>
      ) : (
        <>
          <button
            className="btn"
            disabled={!canUpload}
            onClick={onNewFolder}
            title={t("在当前目录新建文件夹")}
          >
            <FontAwesomeIcon icon={faFolderPlus} /> {t("新建文件夹")}
          </button>
          <button
            className="btn"
            disabled={!canUpload}
            onClick={onUploadFolder}
            title={t("上传文件夹到当前目录")}
          >
            <FontAwesomeIcon icon={faFolderOpen} /> {t("上传文件夹")}
          </button>
          <button
            className="btn btn--primary"
            disabled={!canUpload}
            onClick={onUpload}
            title={t("上传文件到当前目录")}
          >
            <FontAwesomeIcon icon={faUpload} /> {t("上传")}
          </button>
        </>
      )}
    </div>
  );
}
