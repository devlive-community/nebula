import { useState } from "react";

interface Props {
  title: string;
  placeholder?: string;
  initial?: string;
  submitLabel?: string;
  onSubmit: (value: string) => void;
  onCancel: () => void;
}

export function PromptDialog({
  title,
  placeholder,
  initial = "",
  submitLabel = "确定",
  onSubmit,
  onCancel,
}: Props) {
  const [value, setValue] = useState(initial);
  const trimmed = value.trim();

  return (
    <div className="modal-backdrop" onClick={onCancel}>
      <div className="modal modal--sm" onClick={(e) => e.stopPropagation()}>
        <div className="modal__header">
          <h3>{title}</h3>
        </div>
        <div className="modal__body">
          <input
            className="prompt__input"
            value={value}
            placeholder={placeholder}
            autoFocus
            onChange={(e) => setValue(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && trimmed) onSubmit(trimmed);
            }}
          />
        </div>
        <div className="modal__footer">
          <button className="btn" onClick={onCancel}>
            取消
          </button>
          <button
            className="btn btn--primary"
            disabled={!trimmed}
            onClick={() => onSubmit(trimmed)}
          >
            {submitLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
