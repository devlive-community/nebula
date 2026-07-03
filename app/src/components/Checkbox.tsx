import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import { faCheck, faMinus } from "@fortawesome/free-solid-svg-icons";

interface Props {
  checked: boolean;
  indeterminate?: boolean;
  title?: string;
  onChange: () => void;
}

/** 自定义勾选框(button 实现),契合深色主题,支持选中 / 半选。 */
export function Checkbox({ checked, indeterminate, title, onChange }: Props) {
  const on = checked || indeterminate;
  return (
    <button
      type="button"
      role="checkbox"
      aria-checked={indeterminate ? "mixed" : checked}
      title={title}
      className={`checkbox ${on ? "checkbox--on" : ""}`}
      onClick={(e) => {
        e.stopPropagation();
        onChange();
      }}
    >
      {indeterminate ? (
        <FontAwesomeIcon icon={faMinus} />
      ) : checked ? (
        <FontAwesomeIcon icon={faCheck} />
      ) : null}
    </button>
  );
}
