/** 与 src-tauri/src/app.rs 的 AppInfo 对齐（serde camelCase） */
export interface AppInfo {
  name: string;
  version: string;
  platform: string;
}

/** session_create 的返回(与 Rust 侧 SessionCreated 对齐,serde camelCase) */
export interface SessionCreated {
  sessionId: string;
  state: string;
}

/** session://output 事件载荷(agent::manager::SessionEvent::Output,serde tag=type) */
export interface SessionOutputEvent {
  type: "output";
  id: string;
  data: string;
}

/** session://exit 事件载荷 */
export interface SessionExitEvent {
  type: "exit";
  id: string;
  code: number;
}
