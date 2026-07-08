import { check, type Update } from "@tauri-apps/plugin-updater";

export type { Update };

/** 向更新端点查询是否有新版本;有则返回 Update,否则 null。失败会抛出。 */
export async function checkForUpdate(): Promise<Update | null> {
  return await check();
}
