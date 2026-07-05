export type EntryKind = "file" | "directory";

/** 上传进度事件负载(Rust 端 UploadProgress)。 */
export interface UploadProgress {
  path: string;
  uploaded: number;
  total: number;
}

/** 下载进度事件负载(Rust 端 DownloadProgress)。 */
export interface DownloadProgress {
  path: string;
  downloaded: number;
  total: number;
}

/** 应用设置(与 Rust 端 app_core::Settings 对应)。 */
export interface Settings {
  share_expiry_secs: number;
  concurrency: number;
}

/** 传输任务列表中的一项。 */
export interface TransferItem {
  id: string;
  kind: "上传" | "下载";
  name: string;
  done: number;
  total: number;
  status: "active" | "done" | "error";
}

/** 与 Rust 端 nebula_provider::Entry 对应。 */
export interface Entry {
  name: string;
  path: string;
  kind: EntryKind;
  size: number;
  last_modified: string | null;
  etag: string | null;
}
