import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import {
  faCloud,
  faMoon,
  faPlus,
  faSun,
  faXmark,
} from "@fortawesome/free-solid-svg-icons";

interface Props {
  accounts: string[];
  current: string | null;
  theme: "dark" | "light";
  onSelect: (id: string) => void;
  onAdd: () => void;
  onRemove: (id: string) => void;
  onToggleTheme: () => void;
}

export function Sidebar({
  accounts,
  current,
  theme,
  onSelect,
  onAdd,
  onRemove,
  onToggleTheme,
}: Props) {
  return (
    <aside className="sidebar">
      <div className="sidebar__brand">
        <FontAwesomeIcon icon={faCloud} className="sidebar__logo" />
        <span>Nebula</span>
      </div>

      <div className="sidebar__section-title">账号</div>
      <nav className="sidebar__accounts">
        {accounts.length === 0 && <div className="sidebar__empty">还没有账号</div>}
        {accounts.map((id) => (
          <div
            key={id}
            className={`account-item ${id === current ? "account-item--active" : ""}`}
            onClick={() => onSelect(id)}
          >
            <span className="account-item__dot" />
            <span className="account-item__name">{id}</span>
            <button
              className="account-item__remove"
              title="移除账号"
              onClick={(e) => {
                e.stopPropagation();
                onRemove(id);
              }}
            >
              <FontAwesomeIcon icon={faXmark} />
            </button>
          </div>
        ))}
      </nav>

      <button className="btn btn--primary sidebar__add" onClick={onAdd}>
        <FontAwesomeIcon icon={faPlus} /> 添加账号
      </button>

      <button className="btn sidebar__theme" onClick={onToggleTheme}>
        <FontAwesomeIcon icon={theme === "dark" ? faSun : faMoon} />
        {theme === "dark" ? " 浅色模式" : " 深色模式"}
      </button>
    </aside>
  );
}
