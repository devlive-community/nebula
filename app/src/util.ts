/** 人类可读的字节数。 */
export function formatBytes(size: number): string {
  if (size <= 0) return "—";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.floor(Math.log(size) / Math.log(1024));
  const n = size / Math.pow(1024, i);
  return `${n.toFixed(i === 0 ? 0 : 1)} ${units[i]}`;
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
