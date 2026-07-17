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
  /** 自定义公共域名(CDN / CNAME);空表示未配置。 */
  custom_domain: string;
}

/** 一个收藏的位置(账号 + 路径),与 Rust 端 app_core::Bookmark 对应。 */
export interface Bookmark {
  account: string;
  path: string;
}

/** Rust 渲染好的一张图(内联 data URL + 尺寸),与 app_core::ImageData 对应。 */
export interface ImageData {
  data_url: string;
  width: number;
  height: number;
  orig_width: number;
  orig_height: number;
}

/** 图片编辑操作(与 app_core::Ops 对应)。几何操作先应用,再颜色调整。 */
export interface ImageOps {
  crop?: { x: number; y: number; width: number; height: number } | null;
  rotate?: number;
  straighten?: number;
  flip_h?: boolean;
  flip_v?: boolean;
  brightness?: number;
  contrast?: number;
  saturation?: number;
  temperature?: number;
  sharpen?: number;
  hue?: number;
  blur?: number;
  grayscale?: boolean;
  invert?: boolean;
  resize?: { width: number; height: number } | null;
}

/** EXIF 摘要(字段全部可选),与 nebula_image::ExifInfo 对应。 */
export interface ExifInfo {
  make?: string | null;
  model?: string | null;
  lens?: string | null;
  taken_at?: string | null;
  exposure?: string | null;
  aperture?: string | null;
  iso?: string | null;
  focal_length?: string | null;
  orientation?: number | null;
  gps_lat?: number | null;
  gps_lon?: number | null;
}

/** 批量重命名规则(与 Rust 端 app_core::RenameRule 对应)。 */
export interface RenameRule {
  /** "prefix" | "suffix" | "replace" */
  mode: string;
  /** 前缀 / 后缀 / 查找串 */
  a: string;
  /** 替换串(仅 replace 用) */
  b: string;
}

/** 一条重命名计划:from → to(与 Rust 端 app_core::RenamePlan 对应)。 */
export interface RenamePlan {
  from: string;
  to: string;
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
    | "重命名文件夹"
    | "移动文件夹"
    | "复制文件夹"
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
  /** 迁移的源端账号 / 路径,供面板内重试再次发起(仅本会话;不持久化,重启后为空)。 */
  srcAccount?: string;
  srcPath?: string;
  /** 上传因远端内容一致而被秒传跳过。 */
  skipped?: boolean;
}

/** 文本预览结果(与 Rust 端 app_core::TextPreview 对应)。 */
export interface TextPreview {
  text: string;
  /** 内容超过上限被截断。 */
  truncated: boolean;
}

/** 未完成(残留)的分片上传(与 Rust 端 nebula_provider::IncompleteUpload 对应)。 */
export interface IncompleteUpload {
  key: string;
  upload_id: string;
  /** 发起时间(ISO 8601);未知为空。 */
  initiated: string;
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

/** 单个存储类型的小计(与 Rust 端 app_core::ClassStat 对应)。 */
export interface ClassStat {
  class: string;
  files: number;
  bytes: number;
}

/** 存储类型分布(与 Rust 端 app_core::StorageBreakdown 对应)。 */
export interface StorageBreakdown {
  files: number;
  bytes: number;
  classes: ClassStat[];
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
