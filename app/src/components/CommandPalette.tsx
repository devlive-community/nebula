import { useEffect, useMemo, useRef, useState } from "react";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import {
  faBolt,
  faCloud,
  faBookmark,
  faClockRotateLeft,
  faMagnifyingGlass,
} from "@fortawesome/free-solid-svg-icons";
import { useI18n } from "../i18n";
import type { AccountInfo, Bookmark } from "../types";

/** 一个可执行的命令(动作),由 App 提供。 */
export interface PaletteCommand {
  id: string;
  label: string;
  run: () => void;
}

/** 面板里的一行:跳转类(最近 / 账号 / 收藏)或命令类,统一带一个执行动作。 */
export interface PaletteItem {
  key: string;
  kind: "recent" | "account" | "bookmark" | "command";
  /** 展示用的主标题。 */
  label: string;
  /** 展示用的副标题(账号项为厂商,跳转项为账号名,命令项为「命令」)。 */
  hint: string;
  /** 选中执行的动作。 */
  run: () => void;
}

interface Props {
  accounts: AccountInfo[];
  bookmarks: Bookmark[];
  recents: Bookmark[];
  commands: PaletteCommand[];
  onJump: (account: string, path: string) => void;
  onClose: () => void;
}

const ICONS = {
  recent: faClockRotateLeft,
  account: faCloud,
  bookmark: faBookmark,
  command: faBolt,
} as const;

/**
 * 命令面板:Cmd/Ctrl+K 打开,输入即过滤账号与收藏,回车跳转。
 * 上下键选择、Esc 关闭;纯前端,复用已加载的账号 / 收藏数据。
 */
export function CommandPalette({
  accounts,
  bookmarks,
  recents,
  commands,
  onJump,
  onClose,
}: Props) {
  const { t } = useI18n();
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  const items = useMemo<PaletteItem[]>(() => {
    // 顺序:最近访问 → 命令 → 账号根 → 收藏。跳转类按 loc:账号|路径 去重(先出现的胜出)。
    const cmdHint = t("命令");
    const raw: PaletteItem[] = [
      ...recents.map((r) => ({
        key: `loc:${r.account}|${r.path}`,
        kind: "recent" as const,
        label: r.path || "/",
        hint: r.account,
        run: () => onJump(r.account, r.path),
      })),
      ...commands.map((c) => ({
        key: `cmd:${c.id}`,
        kind: "command" as const,
        label: c.label,
        hint: cmdHint,
        run: c.run,
      })),
      ...accounts.map((a) => ({
        key: `loc:${a.id}|`,
        kind: "account" as const,
        label: a.id,
        hint: a.vendor,
        run: () => onJump(a.id, ""),
      })),
      ...bookmarks.map((b) => ({
        key: `loc:${b.account}|${b.path}`,
        kind: "bookmark" as const,
        label: b.path || "/",
        hint: b.account,
        run: () => onJump(b.account, b.path),
      })),
    ];
    const seen = new Set<string>();
    const all = raw.filter((it) => {
      if (seen.has(it.key)) return false;
      seen.add(it.key);
      return true;
    });
    const q = query.trim().toLowerCase();
    if (!q) return all;
    return all.filter(
      (it) =>
        it.label.toLowerCase().includes(q) || it.hint.toLowerCase().includes(q),
    );
  }, [accounts, bookmarks, recents, commands, query, onJump, t]);

  // 过滤结果变化时,把选中项夹回有效范围。
  useEffect(() => {
    setActive((a) => Math.min(a, Math.max(0, items.length - 1)));
  }, [items.length]);

  const choose = (it: PaletteItem | undefined) => {
    if (!it) return;
    it.run();
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
            placeholder={t("跳转或执行命令…")}
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
                key={it.key}
                className={`palette__item ${i === active ? "palette__item--active" : ""}`}
                onMouseEnter={() => setActive(i)}
                onClick={() => choose(it)}
              >
                <FontAwesomeIcon icon={ICONS[it.kind]} className="palette__icon" />
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
