import { useEffect, useRef, useState } from "react";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import { faPen, faCheck, faXmark } from "@fortawesome/free-solid-svg-icons";
import { breadcrumbs } from "../util";
import { useI18n } from "../i18n";

interface Props {
  path: string;
  onNavigate: (path: string) => void;
}

/** 路径面包屑:逐层点击导航,或点铅笔切换成输入框,直接输入 / 粘贴完整路径回车跳转。 */
export function Breadcrumb({ path, onNavigate }: Props) {
  const { t } = useI18n();
  const crumbs = breadcrumbs(path);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(path);
  const inputRef = useRef<HTMLInputElement>(null);

  const startEdit = () => {
    setDraft(path);
    setEditing(true);
  };
  useEffect(() => {
    if (editing) {
      inputRef.current?.focus();
      inputRef.current?.select();
    }
  }, [editing]);

  const commit = () => {
    // 归一化:去首尾空白与两端多余斜杠(根为空串)。
    const p = draft.trim().replace(/^\/+/, "").replace(/\/+$/, "");
    setEditing(false);
    if (p !== path) onNavigate(p);
  };

  if (editing) {
    return (
      <div className="breadcrumb breadcrumb--edit">
        <input
          ref={inputRef}
          className="breadcrumb__input"
          value={draft}
          placeholder={t("输入路径,如 bucket/dir/")}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") commit();
            else if (e.key === "Escape") setEditing(false);
          }}
        />
        <button className="breadcrumb__editbtn" onClick={commit} title={t("跳转")}>
          <FontAwesomeIcon icon={faCheck} />
        </button>
        <button
          className="breadcrumb__editbtn"
          onClick={() => setEditing(false)}
          title={t("取消")}
        >
          <FontAwesomeIcon icon={faXmark} />
        </button>
      </div>
    );
  }

  return (
    <div className="breadcrumb">
      {crumbs.map((c, i) => (
        <span key={c.path} className="breadcrumb__item">
          <button className="breadcrumb__link" onClick={() => onNavigate(c.path)}>
            {c.label}
          </button>
          {i < crumbs.length - 1 && <span className="breadcrumb__sep">/</span>}
        </span>
      ))}
      <button
        className="breadcrumb__editbtn"
        onClick={startEdit}
        title={t("编辑路径")}
      >
        <FontAwesomeIcon icon={faPen} />
      </button>
    </div>
  );
}
