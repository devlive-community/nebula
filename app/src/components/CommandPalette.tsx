import { useEffect, useMemo, useRef, useState } from "react";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import { faCloud, faBookmark, faMagnifyingGlass } from "@fortawesome/free-solid-svg-icons";
import { useI18n } from "../i18n";
import type { AccountInfo, Bookmark } from "../types";

/** 面板里的一个可跳转项:账号根,或某个收藏。 */
export interface PaletteItem {
  kind: "account" | "bookmark";
  account: string;
  path: string;
  /** 展示用的主标题。 */
  label: string;
  /** 展示用的副标题(账号项为厂商,收藏项为账号名)。 */
  hint: string;
}

interface Props {
  accounts: AccountInfo[];
  bookmarks: Bookmark[];
  onJump: (account: string, path: string) => void;
  onClose: () => void;
}

/**
 * 命令面板:Cmd/Ctrl+K 打开,输入即过滤账号与收藏,回车跳转。
 * 上下键选择、Esc 关闭;纯前端,复用已加载的账号 / 收藏数据。
 */
export function CommandPalette({ accounts, bookmarks, onJump, onClose }: Props) {
  const { t } = useI18n();
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  const items = useMemo<PaletteItem[]>(() => {
    const all: PaletteItem[] = [
      ...accounts.map((a) => ({
        kind: "account" as const,
        account: a.id,
        path: "",
        label: a.id,
        hint: a.vendor,
      })),
      ...bookmarks.map((b) => ({
        kind: "bookmark" as const,
        account: b.account,
        path: b.path,
        label: b.path || "/",
        hint: b.account,
      })),
    ];
    const q = query.trim().toLowerCase();
    if (!q) return all;
    return all.filter(
      (it) =>
        it.label.toLowerCase().includes(q) ||
        it.hint.toLowerCase().includes(q) ||
        it.account.toLowerCase().includes(q),
    );
  }, [accounts, bookmarks, query]);

  // 过滤结果变化时,把选中项夹回有效范围。
  useEffect(() => {
    setActive((a) => Math.min(a, Math.max(0, items.length - 1)));
  }, [items.length]);

  const choose = (it: PaletteItem | undefined) => {
    if (!it) return;
    onJump(it.account, it.path);
    onClose();
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setActive((a) => Math.min(a + 1, items.length - 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setActive((a) => Math.max(a - 1, 0));
    } else if (e.key === "Enter") {
      e.preventDefault();
      choose(items[active]);
    } else if (e.key === "Escape") {
      e.preventDefault();
      onClose();
    }
  };

  // 选中项滚动进可视区。
  useEffect(() => {
    const el = listRef.current?.children[active] as HTMLElement | undefined;
    el?.scrollIntoView({ block: "nearest" });
  }, [active]);

  return (
    <div className="palette__backdrop" onMouseDown={onClose}>
      <div className="palette" onMouseDown={(e) => e.stopPropagation()}>
        <div className="palette__search">
          <FontAwesomeIcon icon={faMagnifyingGlass} className="palette__search-icon" />
          <input
            ref={inputRef}
            className="palette__input"
            value={query}
            placeholder={t("跳转到账号或收藏…")}
            onChange={(e) => {
              setQuery(e.target.value);
              setActive(0);
            }}
            onKeyDown={onKeyDown}
          />
        </div>
        <div className="palette__list" ref={listRef}>
          {items.length === 0 ? (
            <div className="palette__empty">{t("没有匹配项")}</div>
          ) : (
            items.map((it, i) => (
              <button
                key={`${it.kind}:${it.account}:${it.path}`}
                className={`palette__item ${i === active ? "palette__item--active" : ""}`}
                onMouseEnter={() => setActive(i)}
                onClick={() => choose(it)}
              >
                <FontAwesomeIcon
                  icon={it.kind === "account" ? faCloud : faBookmark}
                  className="palette__icon"
                />
                <span className="palette__label">{it.label}</span>
                <span className="palette__hint">{it.hint}</span>
              </button>
            ))
          )}
        </div>
      </div>
    </div>
  );
}
