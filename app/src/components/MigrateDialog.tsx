import { useCallback, useEffect, useMemo, useState } from "react";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import { faArrowUp, faFolder, faRightLeft } from "@fortawesome/free-solid-svg-icons";
import type { AccountInfo, Entry } from "../types";
import * as api from "../api";
import { VENDORS } from "../vendors";
import { baseName, breadcrumbs, joinRemote, parentPath } from "../util";
import { Select } from "./Select";

interface Props {
  /** 所有账号,用于选择迁移目标。 */
  accounts: AccountInfo[];
  /** 源账号 id。 */
  srcAccount: string;
  /** 源对象完整路径,如 bucket/a/b.txt。 */
  from: string;
  /** 源是否为文件夹(整目录迁移)。true 时确认回传的是**目标目录**,而非目标对象路径。 */
  isFolder?: boolean;
  onConfirm: (dstAccount: string, dstPath: string) => void;
  onCancel: () => void;
}

const accountLabel = (a: AccountInfo): string => {
  const vendor = (VENDORS as Record<string, { label: string }>)[a.vendor]?.label ?? a.vendor;
  return `${vendor} · ${a.id}`;
};

/**
 * 跨账号 / 跨云迁移复制:选一个目标账号,级联浏览进目标账号的某个 bucket / 目录,
 * 再把源对象复制过去(保留源对象)。目标账号默认排除源账号本身。
 */
export function MigrateDialog({ accounts, srcAccount, from, isFolder, onConfirm, onCancel }: Props) {
  const options = useMemo(
    () => accounts.map((a) => ({ value: a.id, label: accountLabel(a) })),
    [accounts],
  );
  const [dstAccount, setDstAccount] = useState(
    () => accounts.find((a) => a.id !== srcAccount)?.id ?? accounts[0]?.id ?? "",
  );
  const [pickPath, setPickPath] = useState("");
  const [dirs, setDirs] = useState<Entry[]>([]);
  const [loading, setLoading] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  const load = useCallback(async () => {
    if (!dstAccount) return;
    setLoading(true);
    setErr(null);
    try {
      const entries = await api.browse(dstAccount, pickPath);
      setDirs(entries.filter((e) => e.kind === "directory"));
    } catch (e) {
      setErr(String(e));
      setDirs([]);
    } finally {
      setLoading(false);
    }
  }, [dstAccount, pickPath]);

  useEffect(() => {
    load();
  }, [load]);

  // 切换目标账号时回到根,重新浏览。
  const pickAccount = (id: string) => {
    setDstAccount(id);
    setPickPath("");
  };

  // 文件夹迁移:落到「目标目录/源文件夹名/…」,确认回传目标目录(pickPath)。
  const srcFolderName = from.replace(/\/+$/, "").split("/").pop() ?? "";
  const target = isFolder
    ? pickPath
      ? joinRemote(pickPath, srcFolderName) + "/"
      : ""
    : pickPath
      ? joinRemote(pickPath, baseName(from))
      : "";
  const sameObject = !isFolder && dstAccount === srcAccount && target === from;
  const canConfirm = dstAccount !== "" && pickPath !== "" && target !== "" && !sameObject;
  const crumbs = breadcrumbs(pickPath);

  return (
    <div className="modal-backdrop" onClick={onCancel}>
      <div className="modal modal--picker" onClick={(e) => e.stopPropagation()}>
        <div className="modal__header">
          <h3>迁移到其他账号</h3>
        </div>

        <div className="modal__body">
          <div className="picker__account">
            <span className="picker__account-label">目标账号</span>
            <Select value={dstAccount} options={options} onChange={pickAccount} />
          </div>

          <div className="picker__bar">
            <button
              className="btn"
              disabled={pickPath === ""}
              onClick={() => setPickPath(parentPath(pickPath))}
              title="上一层"
            >
              <FontAwesomeIcon icon={faArrowUp} />
            </button>
            <div className="picker__crumbs">
              {crumbs.map((c, i) => (
                <span key={c.path}>
                  <button
                    className="breadcrumb__link"
                    onClick={() => setPickPath(c.path)}
                  >
                    {c.label}
                  </button>
                  {i < crumbs.length - 1 && (
                    <span className="breadcrumb__sep">/</span>
                  )}
                </span>
              ))}
            </div>
          </div>

          <div className="picker__list">
            {loading ? (
              <div className="picker__state">加载中…</div>
            ) : err ? (
              <div className="picker__state">{err}</div>
            ) : dirs.length === 0 ? (
              <div className="picker__state">没有子目录</div>
            ) : (
              dirs.map((d) => (
                <button
                  key={d.path}
                  className="picker__item"
                  onClick={() =>
                    setPickPath(d.path.endsWith("/") ? d.path : d.path + "/")
                  }
                >
                  <FontAwesomeIcon icon={faFolder} className="picker__item-icon" />
                  <span>{d.name}</span>
                </button>
              ))
            )}
          </div>

          <div className="picker__target">
            目标:{target || "请进入一个 bucket / 目录"}
          </div>
        </div>

        <div className="modal__footer">
          <button className="btn" onClick={onCancel}>
            取消
          </button>
          <button
            className="btn btn--primary"
            disabled={!canConfirm}
            onClick={() => onConfirm(dstAccount, isFolder ? pickPath : target)}
          >
            <FontAwesomeIcon icon={faRightLeft} /> 迁移到此
          </button>
        </div>
      </div>
    </div>
  );
}
