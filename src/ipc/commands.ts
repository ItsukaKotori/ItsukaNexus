import { invoke } from "@tauri-apps/api/core";
import type { AppInfo, SessionCreated } from "./types";

/** 所有 Tauri invoke 的类型安全封装——前端唯一入口 */

export function getAppInfo(): Promise<AppInfo> {
  return invoke<AppInfo>("app_info");
}

export function sessionCreate(
  providerId: string,
  cols: number,
  rows: number
): Promise<SessionCreated> {
  return invoke<SessionCreated>("session_create", { providerId, cols, rows });
}

export function sessionSendInput(
  sessionId: string,
  data: string
): Promise<void> {
  return invoke<void>("session_send_input", { sessionId, data });
}

export function sessionResize(
  sessionId: string,
  cols: number,
  rows: number
): Promise<void> {
  return invoke<void>("session_resize", { sessionId, cols, rows });
}

export function sessionStop(sessionId: string): Promise<void> {
  return invoke<void>("session_stop", { sessionId, force: true });
}
