import { useState } from "react";
import * as api from "../api";
import { useI18n } from "../i18n";
import { joinRemote } from "../util";

interface Props {
  account: string;
  /** 当前目录(对象将建在此目录下)。 */
  dir: string;
  onClose: () => void;
  onCreated: () => void;
}

/** 在当前目录新建一个文本文件:填文件名与内容,直接作为对象上传(UTF-8)。 */
export function NewTextFileDialog({ account, dir, onClose, onCreated }: Props) {
  const { t } = useI18n();
  const [name, setName] = useState("");
  const [content, setContent] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const mimeFor = (fn: string) =>
    /\.md$/i.test(fn) ? "text/markdown; charset=utf-8" : "text/plain; charset=utf-8";

  const create = async () => {
    const fn = name.trim();
    if (!fn) {
      setError(t("请输入文件名"));
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const dest = joinRemote(dir, fn);
      const bytes = Array.from(new TextEncoder().encode(content));
      await api.putImageBytes(account, dest, bytes, mimeFor(fn));
      onCreated();
      onClose();
    } catch (e) {
      setError(String(e));
      setBusy(false);
    }
  };

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal modal--wide" onClick={(e) => e.stopPropagation()}>
        <div className="modal__header">
          <h3>{t("新建文本文件")}</h3>
        </div>
        <div className="modal__body">
          <label className="field">
            <span>{t("文件名")}</span>
            <input
              autoFocus
              value={name}
              placeholder={t("如 readme.md")}
              onChange={(e) => setName(e.target.value)}
            />
          </label>
          <textarea
            className="newtext__body"
            rows={12}
            value={content}
            placeholder={t("在此输入文件内容…")}
            onChange={(e) => setContent(e.target.value)}
          />
          {error && <div className="sync__error">{error}</div>}
        </div>
        <div className="modal__footer">
          <button className="btn" onClick={onClose}>
            {t("取消")}
          </button>
          <button
            className="btn btn--primary"
            disabled={busy || !name.trim()}
            onClick={create}
          >
            {busy ? t("创建中…") : t("创建")}
          </button>
        </div>
      </div>
    </div>
  );
}
