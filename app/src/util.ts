import type { Entry } from "./types";

/**
 * 是否为 bucket(而非普通文件夹)。
 *
 * bucket 是根层级的顶级目录,路径无内部斜杠(如 `my-bucket`);
 * 桶内文件夹路径含斜杠(如 `my-bucket/sub/`)。UI 据此区分图标与右键菜单。
 */
export function isBucket(entry: Entry): boolean {
  return (
    entry.kind === "directory" &&
    !entry.path.replace(/\/+$/, "").includes("/")
  );
}

/** 人类可读的字节数。 */
export function formatBytes(size: number): string {
  if (size <= 0) return "—";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.floor(Math.log(size) / Math.log(1024));
  const n = size / Math.pow(1024, i);
  return `${n.toFixed(i === 0 ? 0 : 1)} ${units[i]}`;
}

/** 各家云常见存储类型 → 友好中文标签;未收录的原样显示。 */
const STORAGE_LABELS: Record<string, string> = {
  STANDARD: "标准",
  IA: "低频访问",
  STANDARD_IA: "低频访问",
  WARM: "低频 (WARM)",
  ARCHIVE: "归档",
  COLD: "归档 (COLD)",
  GLACIER: "归档 (Glacier)",
  DEEP_ARCHIVE: "深度归档",
  COLD_ARCHIVE: "冷归档",
  INTELLIGENT_TIERING: "智能分层",
  REDUCED_REDUNDANCY: "低冗余",
};

/** 存储类型 → 友好标签;null / 未知回退。 */
export function storageLabel(sc: string | null): string {
  return sc ? (STORAGE_LABELS[sc.toUpperCase()] ?? sc) : "标准";
}

/** 人类可读的时长(秒),用于传输 ETA。非有限 / 负数返回空串。 */
export function formatDuration(secs: number): string {
  if (!Number.isFinite(secs) || secs < 0) return "";
  const s = Math.round(secs);
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m${s % 60}s`;
  const h = Math.floor(m / 60);
  return `${h}h${m % 60}m`;
}

/** 以最多 `limit` 个并发执行 `worker`,全部完成后 resolve。 */
export async function runPool<T>(
  items: T[],
  limit: number,
  worker: (item: T) => Promise<void>,
): Promise<void> {
  let cursor = 0;
  const runners = Array.from(
    { length: Math.min(limit, items.length) },
    async () => {
      while (cursor < items.length) {
        const item = items[cursor++];
        await worker(item);
      }
    },
  );
  await Promise.all(runners);
}

/** 可当作文本预览的扩展名(代码 / 配置 / 数据 / 纯文本)。 */
const TEXT_EXTS = new Set([
  "txt", "md", "markdown", "log", "csv", "tsv", "json", "json5", "jsonl",
  "xml", "yaml", "yml", "toml", "ini", "conf", "cfg", "env", "properties",
  "html", "htm", "css", "scss", "less", "svg",
  "js", "jsx", "mjs", "cjs", "ts", "tsx", "vue", "svelte",
  "rs", "go", "py", "rb", "php", "java", "kt", "kts", "scala", "swift",
  "c", "h", "cpp", "cc", "hpp", "cs", "m", "mm", "dart", "lua", "pl", "r",
  "sh", "bash", "zsh", "fish", "ps1", "bat", "sql", "graphql", "gql",
  "dockerfile", "makefile", "gitignore", "editorconfig",
]);

/** 判断文件是否可预览,返回预览类型或 null。SVG 归为图片(可渲染)。 */
export function previewKind(
  name: string,
): "image" | "video" | "audio" | "pdf" | "text" | null {
  const ext = name.includes(".") ? name.split(".").pop()!.toLowerCase() : "";
  if (["jpg", "jpeg", "png", "gif", "webp", "svg", "bmp", "avif"].includes(ext))
    return "image";
  if (["mp4", "webm", "mov", "m4v", "ogg"].includes(ext)) return "video";
  if (["mp3", "wav", "flac", "m4a", "aac", "opus", "oga", "wma"].includes(ext))
    return "audio";
  if (ext === "pdf") return "pdf";
  // 无扩展名时按常见文件名兜底(Dockerfile / Makefile 等)。
  const base = name.toLowerCase();
  if (TEXT_EXTS.has(ext) || TEXT_EXTS.has(base)) return "text";
  return null;
}

/** 按扩展名粗略猜测文件类型,用于详情展示。 */
export function guessType(name: string): string {
  const ext = name.includes(".") ? name.split(".").pop()!.toLowerCase() : "";
  if (!ext) return "文件";
  const map: Record<string, string> = {
    jpg: "图片",
    jpeg: "图片",
    png: "图片",
    gif: "图片",
    webp: "图片",
    svg: "图片",
    mp4: "视频",
    mov: "视频",
    mkv: "视频",
    webm: "视频",
    mp3: "音频",
    wav: "音频",
    flac: "音频",
    pdf: "PDF",
    zip: "压缩包",
    tar: "压缩包",
    gz: "压缩包",
    rar: "压缩包",
    txt: "文本",
    md: "文本",
    json: "JSON",
    csv: "CSV",
    html: "网页",
    css: "样式",
    js: "代码",
    ts: "代码",
    rs: "代码",
    py: "代码",
    go: "代码",
  };
  return map[ext] ?? ext.toUpperCase();
}

/** 把 ISO 时间(如 2026-07-03T08:12:04.000Z)格式化为本地时区的可读字符串。 */
export function formatDate(iso: string | null): string {
  if (!iso) return "—";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  const p = (n: number) => String(n).padStart(2, "0");
  return (
    `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ` +
    `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`
  );
}

/** 取路径的父目录(用于"返回上一层")。根("")保持不变。 */
export function parentPath(path: string): string {
  const trimmed = path.replace(/\/+$/, "");
  if (!trimmed) return "";
  const idx = trimmed.lastIndexOf("/");
  return idx === -1 ? "" : trimmed.slice(0, idx + 1);
}

/** 把路径拆成面包屑片段:返回 [{ label, path }]。 */
export function breadcrumbs(path: string): { label: string; path: string }[] {
  const crumbs = [{ label: "全部 Bucket", path: "" }];
  const clean = path.replace(/\/+$/, "");
  if (!clean) return crumbs;
  const parts = clean.split("/");
  let acc = "";
  parts.forEach((part, i) => {
    acc += part + (i < parts.length - 1 ? "/" : "");
    crumbs.push({ label: part, path: i === 0 ? part : acc });
  });
  return crumbs;
}

/** 拼接远端上传路径:当前目录 + 文件名。 */
export function joinRemote(dir: string, name: string): string {
  if (!dir) return name;
  return dir.endsWith("/") ? dir + name : dir + "/" + name;
}

/** 从本地路径取文件名。 */
export function baseName(localPath: string): string {
  const parts = localPath.split(/[\\/]/);
  return parts[parts.length - 1] || localPath;
}
