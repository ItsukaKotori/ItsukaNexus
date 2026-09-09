import { invoke } from "@tauri-apps/api/core";
import type { AppInfo } from "./types";

/** 所有 Tauri invoke 的类型安全封装——前端唯一入口 */
export function getAppInfo(): Promise<AppInfo> {
  return invoke<AppInfo>("app_info");
}
