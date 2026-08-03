import { invoke } from "@tauri-apps/api/core";
import type { AccountInfo, Bookmark, Entry, ExifInfo, FolderStats, ImageData, ImageOps, IncompleteUpload, Integrity, LifecycleRule, Page, RenamePlan, RenameRule, SearchResult, Settings, StorageBreakdown, TextPreview, TransferItem, UploadEntry } from "./types";

/** 类型化的 Tauri command 封装。参数用 camelCase,Tauri 自动映射到 Rust 的 snake_case。 */

export const listAccounts = () => invoke<string[]>("list_accounts");

export const listAccountInfos = () =>
  invoke<AccountInfo[]>("list_account_infos");

export const addAliyunAccount = (
  id: string,
  accessKeyId: string,
  accessKeySecret: string,
  endpoint: string,
) => invoke<void>("add_aliyun_account", { id, accessKeyId, accessKeySecret, endpoint });

export const addHuaweiAccount = (
  id: string,
  accessKeyId: string,
  accessKeySecret: string,
  endpoint: string,
) => invoke<void>("add_huawei_account", { id, accessKeyId, accessKeySecret, endpoint });

export const addQiniuAccount = (
  id: string,
  accessKeyId: string,
  accessKeySecret: string,
  endpoint: string,
) => invoke<void>("add_qiniu_account", { id, accessKeyId, accessKeySecret, endpoint });

export const addAwsAccount = (
  id: string,
  accessKeyId: string,
  accessKeySecret: string,
  endpoint: string,
) => invoke<void>("add_aws_account", { id, accessKeyId, accessKeySecret, endpoint });

export const addR2Account = (
  id: string,
  accessKeyId: string,
  accessKeySecret: string,
  endpoint: string,
) => invoke<void>("add_r2_account", { id, accessKeyId, accessKeySecret, endpoint });

export const addMinioAccount = (
  id: string,
  accessKeyId: string,
  accessKeySecret: string,
  endpoint: string,
) => invoke<void>("add_minio_account", { id, accessKeyId, accessKeySecret, endpoint });

export const addTencentAccount = (
  id: string,
  accessKeyId: string,
  accessKeySecret: string,
  endpoint: string,
) => invoke<void>("add_tencent_account", { id, accessKeyId, accessKeySecret, endpoint });

export const removeAccount = (id: string) => invoke<boolean>("remove_account", { id });

/** 设置某账号的自定义公共域名(CDN / CNAME);空串清除。 */
export const setAccountDomain = (id: string, domain: string) =>
  invoke<void>("set_account_domain", { id, domain });

export const getAccount = (id: string) =>
  invoke<AccountInfo | null>("get_account", { id });

export const browse = (account: string, path: string) =>
  invoke<Entry[]>("browse", { account, path });

export const browsePage = (
  account: string,
  path: string,
  cursor: string | null,
) => invoke<Page>("browse_page", { account, path, cursor });

export const statPath = (account: string, path: string) =>
  invoke<Entry>("stat", { account, path });

export const verifyObject = (account: string, path: string) =>
  invoke<Integrity>("verify_object", { account, path });

/** 转换对象存储类型 / 归档层。 */
export const setStorageClass = (
  account: string,
  path: string,
  storageClass: string,
) => invoke<void>("set_storage_class", { account, path, class: storageClass });

/** 取回(解冻)归档对象,days 为保持天数。 */
export const restoreObject = (account: string, path: string, days: number) =>
  invoke<void>("restore_object", { account, path, days });

/** 递归转换整个文件夹的存储类型。 */
export const setStorageClassFolder = (
  account: string,
  path: string,
  storageClass: string,
) =>
  invoke<void>("set_storage_class_folder", {
    account,
    path,
    class: storageClass,
  });

/** 递归取回整个文件夹里的归档对象。 */
export const restoreFolder = (account: string, path: string, days: number) =>
  invoke<void>("restore_folder", { account, path, days });

/** 下载整个文件夹;返回因本地已存在且一致而被跳过的文件数。 */
export const downloadFolder = (
  account: string,
  remoteRoot: string,
  localDir: string,
  transferId: string,
) =>
  invoke<number>("download_folder", {
    account,
    remoteRoot,
    localDir,
    transferId,
  });

/** 请求取消一个进行中的传输(以传输面板的 id 为键)。 */
export const cancelTransfer = (id: string) =>
  invoke<void>("cancel_transfer", { id });

/** 列出持久化的传输任务(重启后恢复面板)。 */
export const listTransfers = () => invoke<TransferItem[]>("list_transfers");

/** 写入(或覆盖)一条传输任务。 */
export const saveTransfer = (record: TransferItem) =>
  invoke<void>("save_transfer", { record });

/** 删除一条持久化传输任务。 */
export const deleteTransfer = (id: string) =>
  invoke<void>("delete_transfer", { id });

export const migrateFolder = (
  srcAccount: string,
  srcRoot: string,
  dstAccount: string,
  dstDir: string,
  transferId: string,
) =>
  invoke<void>("migrate_folder", {
    srcAccount,
    srcRoot,
    dstAccount,
    dstDir,
    transferId,
  });

/** 移动 / 重命名整个文件夹到新的完整路径(同账号)。 */
export const moveFolder = (
  account: string,
  srcRoot: string,
  dstRoot: string,
  transferId: string,
) => invoke<void>("move_folder", { account, srcRoot, dstRoot, transferId });

/** 复制整个文件夹到新的完整路径(同账号,保留源)。 */
export const copyFolder = (
  account: string,
  srcRoot: string,
  dstRoot: string,
  transferId: string,
) => invoke<void>("copy_folder", { account, srcRoot, dstRoot, transferId });

export const deleteFolder = (account: string, path: string) =>
  invoke<void>("delete_folder", { account, path });

/** 内容搜索:在文本 / PDF 文件里查关键字,返回命中文件与片段。 */
export const searchContent = (
  account: string,
  root: string,
  query: string,
  maxHits: number,
) =>
  invoke<import("./types").ContentSearchResult>("search_content", {
    account,
    root,
    query,
    maxHits,
  });

/** 查找某文件夹下内容重复的对象。 */
export const findDuplicates = (account: string, root: string) =>
  invoke<import("./types").DupResult>("find_duplicates", { account, root });

/** 列出某文件夹下最大的 top 个文件。 */
export const largestFiles = (account: string, root: string, top: number) =>
  invoke<import("./types").LargestFiles>("largest_files", {
    account,
    root,
    top,
  });

/** 导出文件夹清单为 CSV,写到本地 dest。 */
export const exportManifest = (account: string, root: string, dest: string) =>
  invoke<void>("export_manifest", { account, root, dest });

/** 批量给多个对象打标签(merge=保留其它键;否则整体替换)。返回成功数。 */
export const setTagsBatch = (
  id: string,
  account: string,
  paths: string[],
  tags: [string, string][],
  merge: boolean,
) => invoke<number>("set_tags_batch", { id, account, paths, tags, merge });

/** 批量把多个对象设为公开读 / 私有。返回成功数。 */
export const setAclBatch = (
  id: string,
  account: string,
  paths: string[],
  isPublic: boolean,
) => invoke<number>("set_acl_batch", { id, account, paths, public: isPublic });

/** 批量把多个对象移动 / 复制到目标目录。返回成功数。 */
export const moveCopyBatch = (
  id: string,
  account: string,
  paths: string[],
  dstDir: string,
  isMove: boolean,
) =>
  invoke<number>("move_copy_batch", {
    id,
    account,
    paths,
    dstDir,
    isMove,
  });

export const search = (
  account: string,
  root: string,
  query: string,
  minSize: number | null,
  ext: string | null,
  maxResults: number,
) =>
  invoke<SearchResult>("search", {
    account,
    root,
    query,
    minSize,
    ext,
    maxResults,
  });

/** 上传一个文件;返回 true 表示远端内容一致、已秒传跳过。 */
export const uploadFile = (
  account: string,
  remotePath: string,
  localPath: string,
  transferId: string,
  contentType?: string,
) =>
  invoke<boolean>("upload_file", {
    account,
    remotePath,
    localPath,
    transferId,
    contentType: contentType ?? null,
  });

/** 下载一个文件;返回 true 表示本地已存在且一致、已跳过下载。 */
export const downloadFile = (
  account: string,
  remotePath: string,
  localPath: string,
  transferId: string,
) =>
  invoke<boolean>("download_file", {
    account,
    remotePath,
    localPath,
    transferId,
  });

/** 修改对象的内容类型(Content-Type)。 */
export const setContentType = (
  account: string,
  path: string,
  contentType: string,
) => invoke<void>("set_content_type", { account, path, contentType });

/** 读取对象前若干字节并解码为文本,用于预览。 */
export const readPreview = (account: string, path: string, maxBytes: number) =>
  invoke<TextPreview>("read_preview", { account, path, maxBytes });

/** 设置对象为公开读 / 私有。 */
export const setObjectAcl = (account: string, path: string, isPublic: boolean) =>
  invoke<void>("set_object_acl", { account, path, public: isPublic });

/** 取对象的永久公共直链(不签名);未支持返回 null。 */
export const publicUrl = (account: string, path: string) =>
  invoke<string | null>("public_url", { account, path });

/** 列举某桶下未完成(残留)的分片上传。 */
export const incompleteUploads = (account: string, bucket: string) =>
  invoke<IncompleteUpload[]>("incomplete_uploads", { account, bucket });

/** 清理某桶下所有未完成的分片上传,返回清理数量。 */
export const cleanIncompleteUploads = (account: string, bucket: string) =>
  invoke<number>("clean_incomplete_uploads", { account, bucket });

/** 读取对象标签(键值对数组)。 */
export const objectTags = (account: string, path: string) =>
  invoke<[string, string][]>("object_tags", { account, path });

/** 覆盖对象标签(整套替换;空数组即清空)。 */
export const setObjectTags = (
  account: string,
  path: string,
  tags: [string, string][],
) => invoke<void>("set_object_tags", { account, path, tags });

/** 统计文件夹 / Bucket 的文件数与总大小。 */
export const folderStats = (account: string, path: string) =>
  invoke<FolderStats>("folder_stats", { account, path });

/** 统计文件夹 / Bucket 下对象按存储类型的分布。 */
export const storageBreakdown = (account: string, path: string) =>
  invoke<StorageBreakdown>("storage_breakdown", { account, path });

export const createBucket = (account: string, bucket: string) =>
  invoke<void>("create_bucket", { account, bucket });

export const deleteBucket = (account: string, bucket: string) =>
  invoke<void>("delete_bucket", { account, bucket });

export const bucketLifecycle = (account: string, bucket: string) =>
  invoke<LifecycleRule[]>("bucket_lifecycle", { account, bucket });

export const setBucketLifecycle = (
  account: string,
  bucket: string,
  rules: LifecycleRule[],
) => invoke<void>("set_bucket_lifecycle", { account, bucket, rules });

export const deletePath = (account: string, path: string) =>
  invoke<void>("delete", { account, path });

export const createFolder = (account: string, path: string) =>
  invoke<void>("create_folder", { account, path });

export const rename = (account: string, from: string, to: string) =>
  invoke<void>("rename", { account, from, to });

export const planBatchRename = (paths: string[], rule: RenameRule) =>
  invoke<RenamePlan[]>("plan_batch_rename", { paths, rule });

// 图片加速:Rust 侧解码 / 缩放 / EXIF,返回内联 data URL(带 ETag 缓存)
export const imageView = (
  account: string,
  path: string,
  etag: string | null,
  maxEdge: number,
) => invoke<ImageData>("image_view", { account, path, etag, maxEdge });

export const imageThumb = (
  account: string,
  path: string,
  etag: string | null,
  size: number,
) => invoke<ImageData>("image_thumb", { account, path, etag, size });

export const imageExif = (account: string, path: string, etag: string | null) =>
  invoke<ExifInfo>("image_exif", { account, path, etag });

export const imageEditPreview = (
  account: string,
  path: string,
  etag: string | null,
  ops: ImageOps,
  maxEdge: number,
) => invoke<ImageData>("image_edit_preview", { account, path, etag, ops, maxEdge });

export const imageEditSave = (
  account: string,
  path: string,
  etag: string | null,
  ops: ImageOps,
  dest: string,
  format: string,
  quality: number,
) =>
  invoke<void>("image_edit_save", {
    account,
    path,
    etag,
    ops,
    save: { dest, format, quality },
  });

// 文字标注:取全分辨率编辑图 → 前端 canvas 叠加文字 → 上传合成字节
export const imageEditFull = (
  account: string,
  path: string,
  etag: string | null,
  ops: ImageOps,
) => invoke<ImageData>("image_edit_full", { account, path, etag, ops });

export const putImageBytes = (
  account: string,
  dest: string,
  bytes: number[],
  contentType: string,
) => invoke<void>("put_image_bytes", { account, dest, bytes, contentType });

export const saveImageBytesLocal = (dest: string, bytes: number[]) =>
  invoke<void>("save_image_bytes_local", { dest, bytes });

export const imageEditDownload = (
  account: string,
  path: string,
  etag: string | null,
  ops: ImageOps,
  dest: string,
  format: string,
  quality: number,
) =>
  invoke<void>("image_edit_download", {
    account,
    path,
    etag,
    ops,
    save: { dest, format, quality },
  });

export const copy = (account: string, from: string, to: string) =>
  invoke<void>("copy", { account, from, to });

export const copyAcross = (
  srcAccount: string,
  srcPath: string,
  dstAccount: string,
  dstPath: string,
  transferId: string,
) =>
  invoke<void>("copy_across", {
    srcAccount,
    srcPath,
    dstAccount,
    dstPath,
    transferId,
  });

export const presign = (account: string, path: string, expiresSecs: number) =>
  invoke<string>("presign", { account, path, expiresSecs });

/** 生成预签名上传链接(PUT)。 */
export const presignPut = (account: string, path: string, expiresSecs: number) =>
  invoke<string>("presign_put", { account, path, expiresSecs });

export const expandUploadPaths = (paths: string[]) =>
  invoke<UploadEntry[]>("expand_upload_paths", { paths });

export const presignBatch = (
  account: string,
  paths: string[],
  expiresSecs: number,
) => invoke<string[]>("presign_batch", { account, paths, expiresSecs });

export const getSettings = () => invoke<Settings>("get_settings");

export const saveSettings = (settings: Settings) =>
  invoke<void>("save_settings", { settings });

// 收藏夹(持久化到 SQLite 的 bookmarks 表)
export const getBookmarks = () => invoke<Bookmark[]>("bookmarks");

export const addBookmark = (account: string, path: string) =>
  invoke<void>("add_bookmark", { account, path });

export const removeBookmark = (account: string, path: string) =>
  invoke<void>("remove_bookmark", { account, path });

// 最近访问(持久化到 SQLite 的 recent_locations 表)
export const recordVisit = (account: string, path: string) =>
  invoke<void>("record_visit", { account, path });

export const getRecentLocations = () =>
  invoke<Bookmark[]>("recent_locations");

// 界面偏好(持久化到 SQLite 的 ui_prefs 表)
export const getPref = (key: string) => invoke<string | null>("get_pref", { key });

export const setPref = (key: string, value: string) =>
  invoke<void>("set_pref", { key, value });

// PDF 页面编辑(后端 lopdf 解析 / 组装)
export const readFileBytes = (path: string) =>
  invoke<number[]>("read_file_bytes", { path });

export const pdfInfo = (account: string, path: string) =>
  invoke<import("./types").PdfInfo>("pdf_info", { account, path });

export const pdfBytes = (account: string, path: string) =>
  invoke<number[]>("pdf_bytes", { account, path });

export const pdfText = (account: string, path: string) =>
  invoke<string[]>("pdf_text", { account, path });

/** 压缩瘦身,返回 [原大小, 新大小] 字节数。 */
export const pdfCompress = (account: string, path: string, dest: string) =>
  invoke<[number, number]>("pdf_compress", { account, path, dest });

export const pdfSave = (
  account: string,
  path: string,
  sources: number[][],
  asm: import("./types").PdfAssembly,
  dest: string,
) => invoke<void>("pdf_save", { account, path, sources, asm, dest });

export const pdfDownload = (
  account: string,
  path: string,
  sources: number[][],
  asm: import("./types").PdfAssembly,
  dest: string,
) => invoke<void>("pdf_download", { account, path, sources, asm, dest });

export const pdfNumber = (
  account: string,
  path: string,
  sources: number[][],
  asm: import("./types").PdfAssembly,
  opts: import("./types").PdfPageNumbers,
  dest: string,
) => invoke<void>("pdf_number", { account, path, sources, asm, opts, dest });

export const pdfWatermark = (
  account: string,
  path: string,
  sources: number[][],
  asm: import("./types").PdfAssembly,
  wm: import("./types").PdfWatermark,
  dest: string,
) => invoke<void>("pdf_watermark", { account, path, sources, asm, wm, dest });

// 同步与备份
export const syncPreview = (spec: import("./types").SyncSpec) =>
  invoke<[import("./types").SyncDiffItem[], import("./types").SyncDiffSummary]>(
    "sync_preview",
    { spec },
  );

export const syncRun = (
  id: string,
  spec: import("./types").SyncSpec,
  resolutions: Record<string, import("./types").ConflictChoice> = {},
) => invoke<import("./types").SyncReport>("sync_run", { id, spec, resolutions });

export const syncJobs = () => invoke<import("./types").SyncJob[]>("sync_jobs");

export const saveSyncJob = (job: import("./types").SyncJob) =>
  invoke<void>("save_sync_job", { job });

export const deleteSyncJob = (id: string) =>
  invoke<void>("delete_sync_job", { id });

// AI 抠图插件(去背景)
export const mattingSupported = () => invoke<boolean>("matting_supported");

export const mattingInstalled = () => invoke<boolean>("matting_installed");

export const installMattingPlugin = () =>
  invoke<void>("install_matting_plugin");

export const uninstallMattingPlugin = () =>
  invoke<void>("uninstall_matting_plugin");

export const removeBackground = (account: string, path: string) =>
  invoke<number[]>("remove_background", { account, path });
