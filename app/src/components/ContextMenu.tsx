import { useEffect } from "react";

export interface MenuItem {
  label: string;
  danger?: boolean;
  onClick: () => void;
}

interface Props {
  x: number;
  y: number;
  items: MenuItem[];
  onClose: () => void;
}

/** 简单的右键上下文菜单,点击外部 / 滚动 / Esc 时关闭。 */
export function ContextMenu({ x, y, items, onClose }: Props) {
  useEffect(() => {
    const close = () => onClose();
    window.addEventListener("click", close);
    window.addEventListener("scroll", close, true);
    window.addEventListener("resize", close);
    return () => {
      window.removeEventListener("click", close);
      window.removeEventListener("scroll", close, true);
      window.removeEventListener("resize", close);
    };
  }, [onClose]);

  // 避免超出视口。
  const left = Math.min(x, window.innerWidth - 200);
  const top = Math.min(y, window.innerHeight - (items.length * 34 + 12));

  return (
    <div
      className="context-menu"
      style={{ left, top }}
      onClick={(e) => e.stopPropagation()}
      onContextMenu={(e) => e.preventDefault()}
    >
      {items.map((it) => (
        <button
          key={it.label}
          className={`context-menu__item ${
            it.danger ? "context-menu__item--danger" : ""
          }`}
          onClick={() => {
            it.onClick();
            onClose();
          }}
        >
          {it.label}
        </button>
      ))}
    </div>
  );
}
