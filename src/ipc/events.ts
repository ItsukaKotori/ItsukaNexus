// 所有 Tauri listen 的封装——前端唯一入口。
// M2 形态:全局订阅(不再按 sessionId 过滤,分发交给 store);
// session://output 已废除——输出走 session_attach 的 per-session Channel。
//
// promise 语义:这里把订阅失败降级为 no-op unlisten(记日志,不 reject),
// 调用方 `void p.then(u => u())` 的链上不会再有悬挂拒绝。
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type { SessionExitEvent, StateChange, WorktreeChanged } from "./types";

function safeListen<T>(
  event: string,
  cb: (payload: T) => void
): Promise<UnlistenFn> {
  return listen<T>(event, (e) => cb(e.payload)).catch((err: unknown) => {
    console.error(`[events] 订阅 ${event} 失败,事件将不可达`, err);
    return (): void => {}; // no-op unlisten
  });
}

/** 会话状态迁移(running/stopping/exited/failed),全局事件 */
export function onSessionStateEvent(
  cb: (sc: StateChange) => void
): Promise<UnlistenFn> {
  return safeListen<StateChange>("session://state", cb);
}

/** 会话退出(流结束的唯一权威信号),全局事件 */
export function onSessionExitEvent(
  cb: (ev: SessionExitEvent) => void
): Promise<UnlistenFn> {
  return safeListen<SessionExitEvent>("session://exit", cb);
}

/** worktree://changed:增删联动(全局事件) */
export function onWorktreeChanged(
  cb: (ev: WorktreeChanged) => void
): Promise<UnlistenFn> {
  return safeListen<WorktreeChanged>("worktree://changed", cb);
}
