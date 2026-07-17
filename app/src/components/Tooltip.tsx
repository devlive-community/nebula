import { useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";

interface Props {
  /** 提示文字。 */
  label: string;
  /** 显示在触发元素的上方还是下方。默认上方。 */
  side?: "top" | "bottom";
  children: ReactNode;
}

/**
 * 轻量提示组件:悬停短暂延迟后显示,内容用 portal 渲染到 body,
 * 以 fixed 定位,不会被工具栏 / 滚动容器的 overflow 裁掉。跟随应用主题。
 */
export function Tooltip({ label, side = "top", children }: Props) {
  const ref = useRef<HTMLSpanElement>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const [pos, setPos] = useState<{ x: number; y: number } | null>(null);

  const show = () => {
    const el = ref.current;
    if (!el) return;
    const r = el.getBoundingClientRect();
    const x = r.left + r.width / 2;
    const y = side === "top" ? r.top : r.bottom;
    timer.current = setTimeout(() => setPos({ x, y }), 320);
  };

  const hide = () => {
    if (timer.current) clearTimeout(timer.current);
    timer.current = null;
    setPos(null);
  };

  return (
    <span
      ref={ref}
      className="tooltip-trigger"
      onMouseEnter={show}
      onMouseLeave={hide}
      onMouseDown={hide}
    >
      {children}
      {pos &&
        label &&
        createPortal(
          <span
            className={`tooltip tooltip--${side}`}
            style={{ left: pos.x, top: pos.y }}
            role="tooltip"
          >
            {label}
          </span>,
          document.body,
        )}
    </span>
  );
}
