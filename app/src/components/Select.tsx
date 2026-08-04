import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import { faChevronDown, faCheck } from "@fortawesome/free-solid-svg-icons";

export interface SelectOption {
  value: string;
  label: string;
}

interface Props {
  value: string;
  options: SelectOption[];
  onChange: (value: string) => void;
  disabled?: boolean;
}

interface Coords {
  left: number;
  width: number;
  top?: number;
  bottom?: number;
}

/**
 * 与应用风格统一的自定义下拉选择器(替代原生 select)。点击外部 / Esc 关闭。
 * 下拉面板通过 Portal 渲染到 body 并用 fixed 定位,避免被 `.modal` 等
 * `overflow: hidden` 的祖先裁切或被同层元素遮挡。
 */
export function Select({ value, options, onChange, disabled }: Props) {
  const [open, setOpen] = useState(false);
  const [coords, setCoords] = useState<Coords | null>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const panelRef = useRef<HTMLDivElement>(null);

  const selected = options.find((o) => o.value === value);

  // 依据触发按钮位置计算面板坐标;下方空间不足时向上弹出。
  const place = () => {
    const r = triggerRef.current?.getBoundingClientRect();
    if (!r) return;
    const spaceBelow = window.innerHeight - r.bottom;
    const openUp = spaceBelow < 240 && r.top > spaceBelow;
    setCoords({
      left: r.left,
      width: r.width,
      ...(openUp
        ? { bottom: window.innerHeight - r.top + 4 }
        : { top: r.bottom + 4 }),
    });
  };

  const toggle = () => {
    if (disabled) return;
    if (!open) place();
    setOpen((v) => !v);
  };

  useLayoutEffect(() => {
    if (open) place();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      const t = e.target as Node;
      if (triggerRef.current?.contains(t) || panelRef.current?.contains(t))
        return;
      setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    // 面板 fixed 定位,背后页面滚动 / 缩放后需关闭以免错位。scroll 事件不冒泡,
    // 用捕获阶段监听才能感知页面里任意可滚动祖先的滚动——但这也会捕获到面板自身选项
    // 列表的滚动(面板内容超过 max-height 时靠 overflow-y: auto 滚动),所以要排除
    // 事件源自面板内部的情况,不然一滚动列表面板就被关掉。
    const onScroll = (e: Event) => {
      if (panelRef.current?.contains(e.target as Node)) return;
      setOpen(false);
    };
    const onResize = () => setOpen(false);
    window.addEventListener("mousedown", onDown);
    window.addEventListener("keydown", onKey);
    window.addEventListener("resize", onResize);
    window.addEventListener("scroll", onScroll, true);
    return () => {
      window.removeEventListener("mousedown", onDown);
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("resize", onResize);
      window.removeEventListener("scroll", onScroll, true);
    };
  }, [open]);

  return (
    <div className="select">
      <button
        ref={triggerRef}
        type="button"
        className={`select__trigger ${open ? "select__trigger--open" : ""}`}
        disabled={disabled}
        onClick={toggle}
      >
        <span>{selected?.label ?? "请选择"}</span>
        <FontAwesomeIcon className="select__chevron" icon={faChevronDown} />
      </button>

      {open &&
        coords &&
        createPortal(
          <div
            ref={panelRef}
            className="select__panel select__panel--portal"
            style={{
              left: coords.left,
              minWidth: coords.width,
              top: coords.top,
              bottom: coords.bottom,
            }}
          >
            {options.map((o) => (
              <button
                key={o.value}
                type="button"
                className={`select__option ${
                  o.value === value ? "select__option--active" : ""
                }`}
                onClick={() => {
                  onChange(o.value);
                  setOpen(false);
                }}
              >
                <span>{o.label}</span>
                {o.value === value && <FontAwesomeIcon icon={faCheck} />}
              </button>
            ))}
          </div>,
          document.body,
        )}
    </div>
  );
}
