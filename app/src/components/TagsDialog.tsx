import { useEffect, useState } from "react";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import { faPlus, faXmark } from "@fortawesome/free-solid-svg-icons";
import * as api from "../api";
import { useI18n } from "../i18n";

interface Props {
  account: string;
  path: string;
  name: string;
  onSaved: (tags: [string, string][]) => void;
  onCancel: () => void;
  onError: (msg: string) => void;
}

/** 对象标签编辑器:加载现有标签 → 增删键值行 → 整套覆盖保存。 */
export function TagsDialog({
  account,
  path,
  name,
  onSaved,
  onCancel,
  onError,
}: Props) {
  const { t } = useI18n();
  const [rows, setRows] = useState<[string, string][]>([]);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    let alive = true;
    api
      .objectTags(account, path)
      .then((tags) => {
        if (alive) {
          setRows(tags);
          setLoading(false);
        }
      })
      .catch((e) => {
        if (alive) {
          onError(String(e));
          onCancel();
        }
      });
    return () => {
      alive = false;
    };
  }, [account, path]);

  const setKey = (i: number, k: string) =>
    setRows((r) => r.map((row, j) => (j === i ? [k, row[1]] : row)));
  const setVal = (i: number, v: string) =>
    setRows((r) => r.map((row, j) => (j === i ? [row[0], v] : row)));
  const addRow = () => setRows((r) => [...r, ["", ""]]);
  const removeRow = (i: number) =>
    setRows((r) => r.filter((_, j) => j !== i));

  const save = async () => {
    // 丢弃键为空的行,键去空白;重复键以最后一个为准。
    const cleaned = new Map<string, string>();
    for (const [k, v] of rows) {
      const key = k.trim();
      if (key) cleaned.set(key, v);
    }
    const tags = [...cleaned.entries()] as [string, string][];
    setSaving(true);
    try {
      await api.setObjectTags(account, path, tags);
      onSaved(tags);
    } catch (e) {
      onError(String(e));
      setSaving(false);
    }
  };

  return (
    <div className="modal-backdrop" onClick={onCancel}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <div className="modal__header">
          <h3>
            {t("对象标签")} · {name}
          </h3>
        </div>
        <div className="modal__body">
          {loading ? (
            <p className="tags__hint">{t("加载中…")}</p>
          ) : rows.length === 0 ? (
            <p className="tags__hint">{t("暂无标签,点下方添加。")}</p>
          ) : (
            <div className="tags__rows">
              {rows.map((row, i) => (
                <div className="tags__row" key={i}>
                  <input
                    className="prompt__input tags__key"
                    value={row[0]}
                    placeholder={t("键")}
                    onChange={(e) => setKey(i, e.target.value)}
                  />
                  <input
                    className="prompt__input tags__val"
                    value={row[1]}
                    placeholder={t("值")}
                    onChange={(e) => setVal(i, e.target.value)}
                  />
                  <button
                    className="tags__remove"
                    title={t("移除")}
                    onClick={() => removeRow(i)}
                  >
                    <FontAwesomeIcon icon={faXmark} />
                  </button>
                </div>
              ))}
            </div>
          )}
          {!loading && (
            <button className="tags__add" onClick={addRow}>
              <FontAwesomeIcon icon={faPlus} /> {t("添加标签")}
            </button>
          )}
        </div>
        <div className="modal__footer">
          <button className="btn" onClick={onCancel}>
            {t("取消")}
          </button>
          <button
            className="btn btn--primary"
            disabled={loading || saving}
            onClick={save}
          >
            {t("保存")}
          </button>
        </div>
      </div>
    </div>
  );
}
