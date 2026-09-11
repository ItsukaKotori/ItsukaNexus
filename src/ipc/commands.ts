// 所有 Tauri invoke 的类型安全封装——前端唯一入口。
// 约定:invoke 返回的 promise 保留拒绝语义,调用方必须消化(不许裸 promise
// 悬挂)——fire-and-forget 场景在 terminalManager 里 void + catch,流程性调用
// 在 App/组件里 try-catch 或 .catch。
import { Channel, invoke } from "@tauri-apps/api/core";

import type {
  AppConfig,
  AppInfo,
  AttachAck,
  PtyChunk,
  SessionCreated,
  SessionSnapshot,
} from "./types";

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
