import { useEffect, useRef, useState } from "react";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import { faBookmark, faStar, faXmark } from "@fortawesome/free-solid-svg-icons";
import { useI18n } from "../i18n";

export interface Bookmark {
  account: string;
  path: string;
}

interface Props {
  bookmarks: Bookmark[];
  isBookmarked: boolean;
  canBookmark: boolean;
  onToggle: () => void;
  onJump: (account: string, path: string) => void;
  onRemove: (account: string, path: string) => void;
}

/** 收藏夹:收藏常去的桶 / 前缀,一键跳回。星标切换当前位置,下拉列出所有收藏。 */
export function Bookmarks({
  bookmarks,
  isBookmarked,
  canBookmark,
  onToggle,
  onJump,
  onRemove,
}: Props) {
  const { t } = useI18n();
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const onDoc = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && setOpen(false);
    document.addEventListener("mousedown", onDoc);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDoc);
      document.removeEventListener("keydown", onKey);
    };
  }, [open]);

  return (
    <div className="bookmarks" ref={ref}>
      <button
        className="btn"
        title={t("收藏夹")}
        onClick={() => setOpen((o) => !o)}
      >
        <FontAwesomeIcon icon={faBookmark} />
      </button>
      {open && (
        <div className="bookmarks__menu">
          <button
            className={`bookmarks__toggle ${
              isBookmarked ? "bookmarks__toggle--on" : ""
            }`}
            disabled={!canBookmark}
            onClick={onToggle}
          >
            <FontAwesomeIcon icon={faStar} />
            {isBookmarked ? t("取消收藏当前位置") : t("收藏当前位置")}
          </button>
          <div className="bookmarks__list">
            {bookmarks.length === 0 ? (
              <div className="bookmarks__empty">{t("还没有收藏")}</div>
            ) : (
              bookmarks.map((b) => (
                <div className="bookmarks__item" key={`${b.account}\n${b.path}`}>
                  <button
                    className="bookmarks__jump"
                    onClick={() => {
                      onJump(b.account, b.path);
                      setOpen(false);
                    }}
                    title={`${b.account}: ${b.path || "/"}`}
                  >
                    <span className="bookmarks__acct">{b.account}</span>
                    <span className="bookmarks__path">{b.path || "/"}</span>
                  </button>
                  <button
                    className="bookmarks__remove"
                    title={t("移除")}
                    onClick={() => onRemove(b.account, b.path)}
                  >
                    <FontAwesomeIcon icon={faXmark} />
                  </button>
                </div>
              ))
            )}
          </div>
        </div>
      )}
    </div>
  );
}
