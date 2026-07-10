import { invoke } from "@tauri-apps/api/core";
import type { AccountInfo, Entry, Integrity, Page, SearchResult, Settings, TransferItem, UploadEntry } from "./types";

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

export const downloadFolder = (
  account: string,
  remoteRoot: string,
  localDir: string,
  transferId: string,
) =>
  invoke<void>("download_folder", { account, remoteRoot, localDir, transferId });

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

export const deleteFolder = (account: string, path: string) =>
  invoke<void>("delete_folder", { account, path });

export const search = (
  account: string,
  root: string,
  query: string,
  maxResults: number,
) => invoke<SearchResult>("search", { account, root, query, maxResults });

export const uploadFile = (
  account: string,
  remotePath: string,
  localPath: string,
  transferId: string,
  contentType?: string,
) =>
  invoke<void>("upload_file", {
    account,
    remotePath,
    localPath,
    transferId,
    contentType: contentType ?? null,
  });

export const downloadFile = (
  account: string,
  remotePath: string,
  localPath: string,
  transferId: string,
) => invoke<void>("download_file", { account, remotePath, localPath, transferId });

export const deletePath = (account: string, path: string) =>
  invoke<void>("delete", { account, path });

export const createFolder = (account: string, path: string) =>
  invoke<void>("create_folder", { account, path });

export const rename = (account: string, from: string, to: string) =>
  invoke<void>("rename", { account, from, to });

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
