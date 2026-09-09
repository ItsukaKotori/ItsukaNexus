/** 所有 Tauri listen 的封装——前端唯一入口。
 *  每个封装:订阅全局事件 + 按会话 id 过滤 + 返回 unlisten。 */
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type { SessionExitEvent, SessionOutputEvent } from "./types";

export function onSessionOutput(
  sessionId: string,
  cb: (data: string) => void
): Promise<UnlistenFn> {
  return listen<SessionOutputEvent>("session://output", (e) => {
    if (e.payload.id === sessionId) cb(e.payload.data);
  });
}

export function onSessionExit(
  sessionId: string,
  cb: (code: number) => void
): Promise<UnlistenFn> {
  return listen<SessionExitEvent>("session://exit", (e) => {
    if (e.payload.id === sessionId) cb(e.payload.code);
  });
}
