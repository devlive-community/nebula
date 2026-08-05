import { useEffect, useState } from "react";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import { faPlus, faXmark } from "@fortawesome/free-solid-svg-icons";
import * as api from "../api";
import { useI18n } from "../i18n";
import { Select } from "./Select";
import type { Grant, Permission } from "../types";

interface Props {
  account: string;
  path: string;
  name: string;
  onSaved: (grants: Grant[]) => void;
  onCancel: () => void;
  onError: (msg: string) => void;
}

const PERMISSIONS: Permission[] = [
  "READ",
  "WRITE",
  "READ_ACP",
  "WRITE_ACP",
  "FULL_CONTROL",
];

/**
 * 细粒度对象 ACL 编辑器:按具体账号 ID 授权(而不是公开/私有二态)。加载现有授权 →
 * 增删 grantee/权限行 → 整套覆盖保存。只有 AWS S3 / 华为云 OBS 支持,由
 * `fine_grained_acl` 能力位在调用方门控。
 */
export function GrantsDialog({
  account,
  path,
  name,
  onSaved,
  onCancel,
  onError,
}: Props) {
  const { t } = useI18n();
  const [rows, setRows] = useState<Grant[]>([]);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    let alive = true;
    api
      .objectGrants(account, path)
      .then((grants) => {
        if (alive) {
          setRows(grants);
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

  const setGranteeId = (i: number, granteeId: string) =>
    setRows((r) => r.map((row, j) => (j === i ? { ...row, grantee_id: granteeId } : row)));
  const setPermission = (i: number, permission: Permission) =>
    setRows((r) => r.map((row, j) => (j === i ? { ...row, permission } : row)));
  const addRow = () =>
    setRows((r) => [...r, { grantee_id: "", permission: "READ" }]);
  const removeRow = (i: number) => setRows((r) => r.filter((_, j) => j !== i));

  const save = async () => {
    // 丢弃账号 ID 为空的行,去空白;重复账号 ID 以最后一个为准。
    const cleaned = new Map<string, Permission>();
    for (const row of rows) {
      const id = row.grantee_id.trim();
      if (id) cleaned.set(id, row.permission);
    }
    const grants: Grant[] = [...cleaned.entries()].map(([grantee_id, permission]) => ({
      grantee_id,
      permission,
    }));
    setSaving(true);
    try {
      await api.setObjectGrants(account, path, grants);
      onSaved(grants);
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
            {t("细粒度授权")} · {name}
          </h3>
        </div>
        <div className="modal__body">
          <p className="tags__hint">
            {t("按账号 ID 授权,整套替换,覆盖现有全部授权(包括公开读等预置权限)。")}
          </p>
          {loading ? (
            <p className="tags__hint">{t("加载中…")}</p>
          ) : rows.length === 0 ? (
            <p className="tags__hint">{t("暂无授权,点下方添加。")}</p>
          ) : (
            <div className="tags__rows">
              {rows.map((row, i) => (
                <div className="tags__row" key={i}>
                  <input
                    className="prompt__input tags__key"
                    value={row.grantee_id}
                    placeholder={t("被授权账号 ID")}
                    onChange={(e) => setGranteeId(i, e.target.value)}
                  />
                  <Select
                    value={row.permission}
                    options={PERMISSIONS.map((p) => ({ value: p, label: p }))}
                    onChange={(v) => setPermission(i, v as Permission)}
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
              <FontAwesomeIcon icon={faPlus} /> {t("添加授权")}
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
