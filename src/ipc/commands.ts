// 所有 Tauri invoke 的类型安全封装——前端唯一入口。
// 约定:invoke 返回的 promise 保留拒绝语义,调用方必须消化(不许裸 promise
// 悬挂)——fire-and-forget 场景在 terminalManager 里 void + catch,流程性调用
// 在 App/组件里 try-catch 或 .catch。
import { Channel, invoke } from "@tauri-apps/api/core";

import type {
  AppConfig,
  AppInfo,
  AttachAck,
  GitCheckInfo,
  PtyChunk,
  RepoInfo,
  SessionCreated,
  SessionSnapshot,
  WorktreeInfo,
} from "./types";

export function getAppInfo(): Promise<AppInfo> {
  return invoke<AppInfo>("app_info");
}

/** M3:opts 缺省 = 不绑仓库;worktreeName 必须伴随 repoPath(Rust 侧校验) */
export function sessionCreate(
  providerId: string,
  cols: number,
  rows: number,
  opts?: { repoPath?: string; worktreeName?: string }
): Promise<SessionCreated> {
  return invoke<SessionCreated>("session_create", {
    providerId,
    cols,
    rows,
    repoPath: opts?.repoPath ?? null,
    worktreeName: opts?.worktreeName ?? null,
  });
}

/** 订阅会话输出:replay 帧(seq=0)先行,实时帧经同一 Channel 续流。
 *  输出直达 terminalManager 的 term.write,绕过 React(spec §1.5)。
 *  重复 attach 会替换旧订阅;流结束以 session://exit 事件为准,Channel 不关。 */
export function sessionAttach(
  sessionId: string,
  output: Channel<PtyChunk>
): Promise<AttachAck> {
  return invoke<AttachAck>("session_attach", { sessionId, output });
}

/** 会话快照列表:含已退出的会话(刷新恢复的数据源) */
export function sessionList(): Promise<SessionSnapshot[]> {
  return invoke<SessionSnapshot[]>("session_list");
}

export function configGet(): Promise<AppConfig> {
  return invoke<AppConfig>("config_get");
}

/** 保存后返回落盘值(以磁盘为准,而非调用方入参) */
export function configSave(config: AppConfig): Promise<AppConfig> {
  return invoke<AppConfig>("config_save", { config });
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

/** force 缺省 false = 优雅关停(先 \x03、宽限,超时再强杀),命令在进程退出
 *  后才返回;force=true 立即 kill。仅 Running 状态可停,重复停会报错。 */
export function sessionStop(sessionId: string, force = false): Promise<void> {
  return invoke<void>("session_stop", { sessionId, force });
}

/** 会话回收(M3 关 tab 即删):仅终态(Exited/Failed)可删,运行中报错 */
export function sessionDispose(sessionId: string): Promise<void> {
  return invoke<void>("session_dispose", { sessionId });
}

/** 探测系统 git:可用性/版本/路径/worktree 支持(缺失返回 available=false,不报错) */
export function gitCheck(): Promise<GitCheckInfo> {
  return invoke<GitCheckInfo>("git_check");
}

/** 校验路径是 git 仓库,返回根目录/当前分支/是否干净 */
export function gitValidateRepo(repoPath: string): Promise<RepoInfo> {
  return invoke<RepoInfo>("git_validate_repo", { repoPath });
}

/** 列出仓库的 worktree(nexus 命名规范项 + 用户外建项) */
export function worktreeList(repoPath: string): Promise<WorktreeInfo[]> {
  return invoke<WorktreeInfo[]>("worktree_list", { repoPath });
}

/** 按 nexus 规范建 worktree;provider/baseRef 缺省由后端兜底(shell / HEAD) */
export function worktreeCreate(
  repoPath: string,
  provider?: string,
  baseRef?: string
): Promise<WorktreeInfo> {
  return invoke<WorktreeInfo>("worktree_create", {
    repoPath,
    provider: provider ?? null,
    baseRef: baseRef ?? null,
  });
}

/** 按 name 移除 worktree;deleteBranch 连带删本地分支 */
export function worktreeRemove(
  repoPath: string,
  name: string,
  deleteBranch: boolean
): Promise<void> {
  return invoke<void>("worktree_remove", { repoPath, name, deleteBranch });
}
