import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import {
  faCloud,
  faGear,
  faMoon,
  faPen,
  faPlus,
  faSun,
  faXmark,
} from "@fortawesome/free-solid-svg-icons";
import { Logo } from "./Logo";
import { Tooltip } from "./Tooltip";
import { vendorMeta } from "../vendors";
import { useI18n } from "../i18n";
import type { AccountInfo } from "../types";

interface Props {
  accounts: AccountInfo[];
  current: string | null;
  theme: "dark" | "light";
  width: number;
  onSelect: (id: string) => void;
  onAdd: () => void;
  onEdit: (id: string) => void;
  onRemove: (id: string) => void;
  onToggleTheme: () => void;
  onSettings: () => void;
}

export function Sidebar({
  accounts,
  current,
  theme,
  width,
  onSelect,
  onAdd,
  onEdit,
  onRemove,
  onToggleTheme,
  onSettings,
}: Props) {
  const { t } = useI18n();
  return (
    <aside className="sidebar" style={{ width }}>
      <div className="sidebar__brand">
        <Logo size={24} />
        <span>Nebula</span>
      </div>

      <div className="sidebar__section-title">{t("账号")}</div>
      <nav className="sidebar__accounts">
        {accounts.length === 0 && (
          <div className="sidebar__empty">{t("还没有账号")}</div>
        )}
        {accounts.map((acc) => {
          const meta = vendorMeta(acc.vendor);
          return (
            <div
              key={acc.id}
              className={`account-item ${acc.id === current ? "account-item--active" : ""}`}
              onClick={() => onSelect(acc.id)}
            >
              <FontAwesomeIcon
                className="account-item__icon"
                icon={faCloud}
                style={{ color: meta.color }}
                title={meta.label}
              />
              <span className="account-item__name" title={acc.id}>
                {acc.id}
              </span>
              <Tooltip label={t("编辑账号")}>
                <button
                  className="account-item__action"
                  onClick={(e) => {
                    e.stopPropagation();
                    onEdit(acc.id);
                  }}
                >
                  <FontAwesomeIcon icon={faPen} />
                </button>
              </Tooltip>
              <Tooltip label={t("移除账号")}>
                <button
                  className="account-item__action account-item__action--danger"
                  onClick={(e) => {
                    e.stopPropagation();
                    onRemove(acc.id);
                  }}
                >
                  <FontAwesomeIcon icon={faXmark} />
                </button>
              </Tooltip>
            </div>
          );
        })}
      </nav>

      <button className="btn btn--primary sidebar__add" onClick={onAdd}>
        <FontAwesomeIcon icon={faPlus} /> {t("添加账号")}
      </button>

      <div className="sidebar__footer">
        <button className="btn" onClick={onToggleTheme}>
          <FontAwesomeIcon icon={theme === "dark" ? faSun : faMoon} />{" "}
          {theme === "dark" ? t("浅色") : t("深色")}
        </button>
        <button className="btn" onClick={onSettings} title={t("设置")}>
          <FontAwesomeIcon icon={faGear} /> {t("设置")}
        </button>
      </div>
    </aside>
  );
}
