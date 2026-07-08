import { useEffect, useRef, useState } from "react";
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

/** 与应用风格统一的自定义下拉选择器(替代原生 select)。点击外部 / Esc 关闭。 */
export function Select({ value, options, onChange, disabled }: Props) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);

  const selected = options.find((o) => o.value === value);

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (!rootRef.current?.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    window.addEventListener("mousedown", onDown);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("mousedown", onDown);
      window.removeEventListener("keydown", onKey);
    };
  }, [open]);

  return (
    <div className="select" ref={rootRef}>
      <button
        type="button"
        className={`select__trigger ${open ? "select__trigger--open" : ""}`}
        disabled={disabled}
        onClick={() => setOpen((v) => !v)}
      >
        <span>{selected?.label ?? "请选择"}</span>
        <FontAwesomeIcon className="select__chevron" icon={faChevronDown} />
      </button>

      {open && (
        <div className="select__panel">
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
        </div>
      )}
    </div>
  );
}
