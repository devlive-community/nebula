import { useEffect, useState } from "react";
import * as api from "../api";
import { useI18n } from "../i18n";
import { formatBytes } from "../util";
import { Checkbox } from "./Checkbox";
import type { DupResult } from "../types";

interface Props {
  account: string;
  root: string;
  onClose: () => void;
  /** 删除完成后让父组件刷新当前目录。 */
  onDeleted: () => void;
}

/**
 * 重复文件对话框:扫描目录、按内容分组展示重复对象。每组默认保留第一个、勾选其余,
 * 一键删除选中的冗余副本(不可逆,删前二次确认)。
 */
export function DuplicatesDialog({ account, root, onClose, onDeleted }: Props) {
  const { t } = useI18n();
  const [result, setResult] = useState<DupResult | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  // 勾选待删的对象路径(默认每组保留第一个、选中其余)。
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [deleting, setDeleting] = useState(false);
  const [confirm, setConfirm] = useState(false);

  useEffect(() => {
    let alive = true;
    setLoading(true);
    api
      .findDuplicates(account, root)
      .then((r) => {
        if (!alive) return;
        setResult(r);
        // 默认选中每组除第一个外的所有副本。
        const sel = new Set<string>();
        for (const g of r.groups)
          for (const e of g.entries.slice(1)) sel.add(e.path);
        setSelected(sel);
        setLoading(false);
      })
      .catch((e) => {
        if (!alive) return;
        setError(String(e));
        setLoading(false);
      });
    return () => {
      alive = false;
    };
  }, [account, root]);

  const toggle = (path: string) => {
    setSelected((s) => {
      const n = new Set(s);
      if (n.has(path)) n.delete(path);
      else n.add(path);
      return n;
    });
  };

  const selectedBytes = () => {
    let total = 0;
    if (!result) return 0;
    for (const g of result.groups)
      for (const e of g.entries) if (selected.has(e.path)) total += g.size;
    return total;
  };

  const doDelete = async () => {
    setConfirm(false);
    setDeleting(true);
    setError(null);
    try {
      for (const path of selected) {
        await api.deletePath(account, path);
      }
      onDeleted();
      onClose();
    } catch (e) {
      setError(String(e));
      setDeleting(false);
    }
  };

  const groupCount = result?.groups.length ?? 0;

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal modal--wide" onClick={(e) => e.stopPropagation()}>
        <div className="modal__header">
          <h3>{t("查找重复文件")}</h3>
        </div>
        <div className="modal__body">
          {loading && <div className="dup__state">{t("扫描中…")}</div>}
          {!loading && result && (
            <>
              <div className="dup__summary">
                {groupCount > 0
                  ? t("{g} 组重复 · 可回收 {b}", {
                      g: String(groupCount),
                      b: formatBytes(result.total_wasted),
                    })
                  : t("未发现重复文件")}
                {result.truncated ? ` · ${t("(已达扫描上限)")}` : ""}
              </div>

              <div className="dup__list">
                {result.groups.map((g) => (
                  <div className="dup__group" key={`${g.size}:${g.etag}`}>
                    <div className="dup__group-head">
                      {t("{n} 个副本 · 每个 {s} · 冗余 {w}", {
                        n: String(g.entries.length),
                        s: formatBytes(g.size),
                        w: formatBytes(g.wasted),
                      })}
                    </div>
                    {g.entries.map((e, i) => (
                      <label
                        className="dup__row"
                        key={e.path}
                        onClick={() => toggle(e.path)}
                      >
                        <Checkbox checked={selected.has(e.path)} onChange={() => toggle(e.path)} />
                        <span className="dup__path" title={e.path}>
                          {e.path}
                        </span>
                        {i === 0 && !selected.has(e.path) && (
                          <span className="dup__keep">{t("保留")}</span>
                        )}
                      </label>
                    ))}
                  </div>
                ))}
              </div>
            </>
          )}
          {error && <div className="dup__error">{error}</div>}
        </div>
        <div className="modal__footer">
          <button className="btn" onClick={onClose}>
            {t("关闭")}
          </button>
          {groupCount > 0 && (
            <button
              className="btn btn--danger"
              disabled={deleting || selected.size === 0}
              onClick={() => setConfirm(true)}
            >
              {deleting
                ? t("删除中…")
                : t("删除选中 {n} 个(省 {b})", {
                    n: String(selected.size),
                    b: formatBytes(selectedBytes()),
                  })}
            </button>
          )}
        </div>

        {confirm && (
          <div className="modal-backdrop" onClick={() => setConfirm(false)}>
            <div className="modal modal--sm" onClick={(e) => e.stopPropagation()}>
              <div className="modal__body">
                {t("确认删除选中的 {n} 个副本?此操作不可撤销。", {
                  n: String(selected.size),
                })}
              </div>
              <div className="modal__footer">
                <button className="btn" onClick={() => setConfirm(false)}>
                  {t("取消")}
                </button>
                <button className="btn btn--danger" onClick={doDelete}>
                  {t("删除")}
                </button>
              </div>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
