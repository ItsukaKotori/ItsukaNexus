// 与 Rust 契约一一对应的 wire 类型(serde camelCase)。
// 权威定义:src-tauri/src/app.rs(AAppInfo)、lib.rs(PtyChunk/AttachAck/
// SessionCreated)、agent/state.rs(SessionState/StateChange/SessionSnapshot)、
// agent/manager.rs(SessionEvent)、config/model.rs(AppConfig 系)。

/** 与 src-tauri/src/app.rs 的 AppInfo 对齐(serde camelCase) */
export interface AppInfo {
  name: string;
  version: string;
  platform: string;
}

/** 会话状态机(agent/state.rs SessionState 枚举,serde 序列化为小写字符串) */
export type SessionState = "running" | "stopping" | "exited" | "failed";

/** session_create 的返回(与 Rust 侧 SessionCreated 对齐) */
export interface SessionCreated {
  sessionId: string;
  state: SessionState;
}

/** session_attach 输出流帧(lib.rs PtyChunk)。
 *  seq = 0 为 replay 帧(每条订阅至多一条、先行);实时帧从 1 起按会话单调,
 *  跨订阅不回退。接缝经 Rust 侧临界段原子化(I-1)不重复不丢失;
 *  seq 过滤仅作防御性保留。 */
export interface PtyChunk {
  sessionId: string;
  data: string;
  seq: number;
}

/** session_attach 确认:replayedBytes = 订阅时刻的历史回放字节数 */
export interface AttachAck {
  replayedBytes: number;
}

/** session_list 返回元素:会话快照(退出后保留,Rust 表是权威) */
export interface SessionSnapshot {
  sessionId: string;
  state: SessionState;
  startedAtMs: number;
  exitCode: number | null;
  pid: number | null;
  /** M3:启动 cwd 关联的仓库与 worktree(未绑定时为 null) */
  repoPath?: string | null;
  worktreeName?: string | null;
}

/** git_check 返回(gitx/ops.rs GitCheckInfo) */
export interface GitCheckInfo {
  available: boolean;
  version: string | null;
  path: string | null;
  worktreeSupported: boolean;
}

/** git_validate_repo 返回(gitx/ops.rs RepoInfo) */
export interface RepoInfo {
  root: string;
  currentBranch: string | null;
  isClean: boolean;
}

/** worktree_list / worktree_create 返回(gitx/ops.rs WorktreeInfo) */
export interface WorktreeInfo {
  name: string;
  path: string;
  branch: string | null;
}

/** worktree://changed 载荷(gitx/worktree.rs WorktreeChanged;
 *  change 为 serde camelCase 枚举值,首字母小写) */
export interface WorktreeChanged {
  repoPath: string;
  change: "created" | "removed";
}

/** session://state 事件载荷(agent/state.rs StateChange,tag=type) */
export interface StateChange {
  type: "state";
  sessionId: string;
  prev: SessionState;
  next: SessionState;
  atMs: number;
  detail?: string | null;
}

/** session://exit 事件载荷(agent/manager.rs SessionEvent::Exit,tag=type)。
 *  流结束以此事件为准:输出 Channel 不因退出关闭。 */
export interface SessionExitEvent {
  type: "exit";
  sessionId: string;
  code: number;
}

/** session:// 全局事件联合(tag=type 可辨别) */
export type SessionStateEvent = StateChange | SessionExitEvent;

/** 终端外观与行为(config/model.rs TerminalConfig;None = 前端内置默认字体栈) */
export interface TerminalConfig {
  fontFamily: string | null;
  fontSize: number;
  scrollback: number;
}

/** Agent 启动档案(M2 只建模,不驱动 spawn) */
export interface AgentProfile {
  id: string;
  displayName: string;
  command: string;
  argsTemplate: string[];
  env: Record<string, string>;
}

/** config_get / config_save 的载荷(config/model.rs AppConfig) */
export interface AppConfig {
  version: number;
  terminal: TerminalConfig;
  agentProfiles: AgentProfile[];
}
