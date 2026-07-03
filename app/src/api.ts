import { invoke } from "@tauri-apps/api/core";
import type { Entry } from "./types";

/** 类型化的 Tauri command 封装。参数用 camelCase,Tauri 自动映射到 Rust 的 snake_case。 */

export const listAccounts = () => invoke<string[]>("list_accounts");

export const addAliyunAccount = (
  id: string,
  accessKeyId: string,
  accessKeySecret: string,
  endpoint: string,
) => invoke<void>("add_aliyun_account", { id, accessKeyId, accessKeySecret, endpoint });

export const removeAccount = (id: string) => invoke<boolean>("remove_account", { id });

export const browse = (account: string, path: string) =>
  invoke<Entry[]>("browse", { account, path });

export const statPath = (account: string, path: string) =>
  invoke<Entry>("stat", { account, path });

export const uploadFile = (
  account: string,
  remotePath: string,
  localPath: string,
  contentType?: string,
) =>
  invoke<void>("upload_file", {
    account,
    remotePath,
    localPath,
    contentType: contentType ?? null,
  });

export const downloadFile = (account: string, remotePath: string, localPath: string) =>
  invoke<void>("download_file", { account, remotePath, localPath });

export const deletePath = (account: string, path: string) =>
  invoke<void>("delete", { account, path });

export const createFolder = (account: string, path: string) =>
  invoke<void>("create_folder", { account, path });

export const rename = (account: string, from: string, to: string) =>
  invoke<void>("rename", { account, from, to });

export const copy = (account: string, from: string, to: string) =>
  invoke<void>("copy", { account, from, to });
