// 自动更新封装：基于 Tauri v2 updater / process 插件。
// - checkForUpdate：查询是否有新版本（无则返回 null）。
// - installAndRelaunch：下载并安装，完成后重启应用。
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

export type { Update };

/** 查询更新；无更新或未配置时返回 null。调用方自行 try/catch（启动静默检查不应打扰用户）。 */
export async function checkForUpdate(): Promise<Update | null> {
  return await check();
}

/** 下载并安装更新，完成后重启。onProgress 回传已下载字节与总字节（总字节可能为 null）。 */
export async function installAndRelaunch(
  update: Update,
  onProgress?: (downloaded: number, total: number | null) => void,
): Promise<void> {
  let downloaded = 0;
  let total: number | null = null;
  await update.downloadAndInstall((event) => {
    switch (event.event) {
      case "Started":
        total = event.data.contentLength ?? null;
        onProgress?.(downloaded, total);
        break;
      case "Progress":
        downloaded += event.data.chunkLength;
        onProgress?.(downloaded, total);
        break;
      case "Finished":
        break;
    }
  });
  await relaunch();
}
