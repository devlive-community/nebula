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

/** 跨账号迁移进度事件负载(Rust 端 TransferProgress)。 */
export interface TransferProgress {
  to: string;
  transferred: number;
  total: number;
}

/** 文件夹级操作进度事件负载(Rust 端 FolderProgress)。以文件数计量。 */
export interface FolderProgress {
  /** "download" | "migrate" | "delete"。 */
  op: string;
  /** 被操作的文件夹路径。 */
  path: string;
  done: number;
  total: number;
}

/** 账号非敏感信息(编辑回填用,与 Rust 端 AccountInfo 对应)。 */
export interface AccountInfo {
  id: string;
  vendor: string;
  access_key_id: string;
  endpoint: string;
}

/** 展开后待上传的一项:本地路径 + 相对(远端)路径。 */
export interface UploadEntry {
  local: string;
  rel: string;
}

/** 应用设置(与 Rust 端 app_core::Settings 对应)。 */
export interface Settings {
  share_expiry_secs: number;
  concurrency: number;
  /** 全局传输带宽上限,KiB/秒;0 表示不限速。 */
  rate_limit_kib_per_sec: number;
}

/** 传输任务列表中的一项(含重试所需的参数)。 */
export interface TransferItem {
  id: string;
  kind:
    | "上传"
    | "下载"
    | "迁移"
    | "下载文件夹"
    | "迁移文件夹"
    | "转换存储类型"
    | "取回归档";
  name: string;
  account: string;
  remote: string;
  local: string;
  done: number;
  total: number;
  status: "active" | "done" | "error" | "cancelled" | "interrupted";
  /** 瞬时速度(字节/秒),仅字节类传输、仅前端展示用(不持久化)。 */
  speed?: number;
}

/** 分页列举的一页(与 Rust 端 nebula_provider::Page 对应)。 */
export interface Page {
  entries: Entry[];
  /** 下一页游标;null 表示已到末页。 */
  cursor: string | null;
}

/** 内容完整性校验结果(与 Rust 端 app_core::Integrity 对应)。 */
export type Integrity =
  | { status: "verified" }
  | { status: "mismatch"; expected: string; actual: string }
  | { status: "unverifiable"; reason: string };

/** 前缀统计(与 Rust 端 app_core::FolderStats 对应)。 */
export interface FolderStats {
  files: number;
  bytes: number;
  /** 因扫描量触顶而偏小。 */
  truncated: boolean;
}

/** 递归搜索结果(与 Rust 端 app_core::SearchResult 对应)。 */
export interface SearchResult {
  entries: Entry[];
  /** 因结果数或扫描量触顶而提前结束 → 结果可能不完整。 */
  truncated: boolean;
}

/** 与 Rust 端 nebula_provider::Entry 对应。 */
export interface Entry {
  name: string;
  path: string;
  kind: EntryKind;
  size: number;
  last_modified: string | null;
  etag: string | null;
  /** 存储类型 / 归档层(如 STANDARD / IA / ARCHIVE);未知为 null。 */
  storage_class: string | null;
  /** 内容类型(MIME);列举时通常为 null,stat 才有。 */
  content_type: string | null;
}
