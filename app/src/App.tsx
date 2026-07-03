import { useCallback, useEffect, useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { listen } from "@tauri-apps/api/event";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import { faCloud, faPlus } from "@fortawesome/free-solid-svg-icons";
import type { DownloadProgress, Entry, Transfer, UploadProgress } from "./types";
import * as api from "./api";
import { baseName, joinRemote, parentPath } from "./util";
import { Sidebar } from "./components/Sidebar";
import { AccountForm } from "./components/AccountForm";
import { Breadcrumb } from "./components/Breadcrumb";
import { Toolbar } from "./components/Toolbar";
import { FileList } from "./components/FileList";

export default function App() {
  const [accounts, setAccounts] = useState<string[]>([]);
  const [current, setCurrent] = useState<string | null>(null);
  const [path, setPath] = useState("");
  const [entries, setEntries] = useState<Entry[]>([]);
  const [loading, setLoading] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [showForm, setShowForm] = useState(false);
  const [transfer, setTransfer] = useState<Transfer | null>(null);

  useEffect(() => {
    const unUpload = listen<UploadProgress>("upload-progress", (e) => {
      setTransfer({
        label: "上传",
        path: e.payload.path,
        done: e.payload.uploaded,
        total: e.payload.total,
      });
    });
    const unDownload = listen<DownloadProgress>("download-progress", (e) => {
      setTransfer({
        label: "下载",
        path: e.payload.path,
        done: e.payload.downloaded,
        total: e.payload.total,
      });
    });
    return () => {
      unUpload.then((off) => off());
      unDownload.then((off) => off());
    };
  }, []);

  const refreshAccounts = useCallback(async () => {
    const list = await api.listAccounts();
    setAccounts(list);
    setCurrent((cur) => cur ?? list[0] ?? null);
  }, []);

  useEffect(() => {
    refreshAccounts();
  }, [refreshAccounts]);

  const load = useCallback(async () => {
    if (!current) {
      setEntries([]);
      return;
    }
    setLoading(true);
    setError(null);
    try {
      setEntries(await api.browse(current, path));
    } catch (e) {
      setError(String(e));
      setEntries([]);
    } finally {
      setLoading(false);
    }
  }, [current, path]);

  useEffect(() => {
    load();
  }, [load]);

  const selectAccount = (id: string) => {
    setCurrent(id);
    setPath("");
  };

  const addAccount = async (
    id: string,
    ak: string,
    sk: string,
    endpoint: string,
  ) => {
    setShowForm(false);
    setError(null);
    try {
      await api.addAliyunAccount(id, ak, sk, endpoint);
      await refreshAccounts();
      setCurrent(id);
      setPath("");
    } catch (e) {
      setError(String(e));
    }
  };

  const removeAccount = async (id: string) => {
    await api.removeAccount(id);
    if (current === id) {
      setCurrent(null);
      setPath("");
      setEntries([]);
    }
    await refreshAccounts();
  };

  const upload = async () => {
    if (!current || !path) return;
    const selected = await open({ multiple: false, title: "选择要上传的文件" });
    if (typeof selected !== "string") return;
    const remote = joinRemote(path, baseName(selected));
    setBusy(true);
    setError(null);
    try {
      await api.uploadFile(current, remote, selected);
      await load();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
      setTransfer(null);
    }
  };

  const download = async (entry: Entry) => {
    if (!current) return;
    const target = await save({ defaultPath: entry.name });
    if (typeof target !== "string") return;
    setBusy(true);
    setError(null);
    try {
      await api.downloadFile(current, entry.path, target);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
      setTransfer(null);
    }
  };

  const remove = async (entry: Entry) => {
    if (!current) return;
    if (!window.confirm(`确定删除 ${entry.name}?`)) return;
    setBusy(true);
    setError(null);
    try {
      await api.deletePath(current, entry.path);
      await load();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="layout">
      <Sidebar
        accounts={accounts}
        current={current}
        onSelect={selectAccount}
        onAdd={() => setShowForm(true)}
        onRemove={removeAccount}
      />

      <main className="main">
        {current ? (
          <>
            <div className="main__header">
              <Breadcrumb path={path} onNavigate={setPath} />
              <Toolbar
                canGoUp={path !== ""}
                canUpload={path !== ""}
                busy={busy}
                onUp={() => setPath(parentPath(path))}
                onRefresh={load}
                onUpload={upload}
              />
            </div>

            {error && <div className="error-banner">{error}</div>}

            <FileList
              entries={entries}
              loading={loading}
              onOpenDir={(e) => setPath(e.path.endsWith("/") ? e.path : e.path + "/")}
              onDownload={download}
              onDelete={remove}
            />

            {transfer && transfer.total > 0 && (
              <div className="upload-progress">
                <div className="upload-progress__info">
                  <span className="upload-progress__name">
                    {transfer.label} {baseName(transfer.path)}
                  </span>
                  <span className="upload-progress__pct">
                    {Math.round((transfer.done / transfer.total) * 100)}%
                  </span>
                </div>
                <div className="upload-progress__track">
                  <div
                    className="upload-progress__fill"
                    style={{
                      width: `${(transfer.done / transfer.total) * 100}%`,
                    }}
                  />
                </div>
              </div>
            )}
          </>
        ) : (
          <div className="empty-state">
            <FontAwesomeIcon icon={faCloud} className="empty-state__icon" />
            <h2>欢迎使用 Nebula</h2>
            <p>添加一个云账号开始管理你的对象存储</p>
            <button className="btn btn--primary" onClick={() => setShowForm(true)}>
              <FontAwesomeIcon icon={faPlus} /> 添加账号
            </button>
          </div>
        )}
      </main>

      {showForm && (
        <AccountForm onSubmit={addAccount} onClose={() => setShowForm(false)} />
      )}
    </div>
  );
}
