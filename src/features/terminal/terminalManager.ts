// React 外的常驻终端实例注册表(spec §1.5 性能红线):
// - PTY 输出经 Channel 直达 term.write,不进 React state/渲染;
// - 实例生命周期与组件挂载解耦:tab 只隐藏不卸载,关 tab 才真正 dispose;
// - 每个 entry 一条 Channel(attach 幂等守卫);seq 过滤为防御性保留
//   (接缝已由 Rust 侧临界段原子化,I-1)。
import { Channel } from "@tauri-apps/api/core";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";

import {
  sessionAttach,
  sessionResize,
  sessionSendInput,
  sessionStop,
} from "../../ipc/commands";
import type { AppConfig, PtyChunk, TerminalConfig } from "../../ipc/types";

const DEFAULT_FONT_STACK = "Menlo, Monaco, 'Courier New', monospace";

interface Entry {
  terminal: Terminal;
  fit: FitAddon;
  /** attach 重入守卫:attach 承诺在途 */
  attaching: boolean;
  /** 本会话已挂上 Channel:一条 Channel 用到关 tab,不重复 attach
   *  (重复 attach 会触发后端整段 replay 重发, live 终端里就是重复历史) */
  attached: boolean;
  /** 终端 DOM 已挂载。open 推迟到容器真有尺寸时:xterm 在 display:none
   *  容器里量出的字宽/行高为 0,开了也渲染不出来 */
  opened: boolean;
  /** 已写入的最大实时 seq:接缝重复帧(≤1 帧或替换订阅并行下发)由此丢弃 */
  lastSeq: number;
  /** 会话已终态(Exited/Failed):onData 输入门禁(xterm 无 readonly) */
  closed: boolean;
}

const entries = new Map<string, Entry>();

let config: TerminalConfig = { fontFamily: null, fontSize: 13, scrollback: 5000 };

/** 启动时注入 config_get 的终端段(此后新建的实例都用它) */
export function setTerminalConfig(c: AppConfig["terminal"]): void {
  config = c;
}

export function getEntry(id: string): Entry | undefined {
  return entries.get(id);
}

/** 会话终态后的输入门禁开关(TerminalPane 随 store 状态调用) */
export function setClosed(id: string, closed: boolean): void {
  const e = entries.get(id);
  if (e) e.closed = closed;
}

function logInvokeError(what: string): (e: unknown) => void {
  return (e: unknown) => console.error(`[terminal] ${what} 失败`, e);
}

/** 创建实例(不含 open):先建实例/先 attach 都允许,写入进内部缓冲,
 *  容器可见后再 openAndFit 一次补上 DOM 与尺寸 */
export function createEntry(id: string): Entry {
  const terminal = new Terminal({
    fontFamily: config.fontFamily ?? DEFAULT_FONT_STACK,
    fontSize: config.fontSize,
    cursorBlink: true,
    scrollback: config.scrollback,
  });
  const fit = new FitAddon();
  terminal.loadAddon(fit);
  terminal.onData((data) => {
    const e = entries.get(id);
    if (!e || e.closed) return; // 退出/失败后输入门禁(xterm 无 readonly)
    void sessionSendInput(id, data).catch(logInvokeError("session_send_input"));
  });
  const entry: Entry = {
    terminal,
    fit,
    attaching: false,
    attached: false,
    opened: false,
    lastSeq: 0,
    closed: false,
  };
  entries.set(id, entry);
  return entry;
}

function writeChunk(entry: Entry, msg: PtyChunk): void {
  if (msg.seq === 0) {
    // replay 帧:整段历史,重置实时基线(entry 只有一条 Channel,至多一次)
    entry.lastSeq = 0;
    entry.terminal.write(msg.data);
    return;
  }
  // 接缝/新旧订阅短暂并行的重复帧:只认比已见最大 seq 更新的
  if (msg.seq <= entry.lastSeq) return;
  entry.lastSeq = msg.seq;
  entry.terminal.write(msg.data);
}

/** attach:replay + 后续流直达 term.write(绕过 React)。已挂/在途即幂等跳过 */
export function attachSession(id: string, entry: Entry): void {
  if (entry.attaching || entry.attached) return;
  entry.attaching = true;
  const channel = new Channel<PtyChunk>();
  channel.onmessage = (msg) => writeChunk(entry, msg);
  void sessionAttach(id, channel)
    .then(() => {
      entry.attached = true;
    })
    .catch((e: unknown) => {
      console.error(`[terminal] session_attach 失败 (${id})`, e);
      entry.attached = false; // 失败允许后续再挂
    })
    .finally(() => {
      entry.attaching = false;
    });
}

/** 把终端挂进容器并 fit(容器必须可见、有尺寸)。返回实测尺寸供上报。 */
export function openAndFit(
  id: string,
  container: HTMLElement
): { cols: number; rows: number } | null {
  const entry = entries.get(id);
  if (!entry) return null;
  if (!entry.opened) {
    entry.terminal.open(container);
    entry.opened = true;
  }
  entry.fit.fit();
  return { cols: entry.terminal.cols, rows: entry.terminal.rows };
}

/** 关 tab 时真正释放实例(App 调用;组件卸载不触发) */
export function disposeEntry(id: string): void {
  const e = entries.get(id);
  if (!e) return;
  entries.delete(id);
  e.terminal.dispose();
}

/** 关 tab 的停止步:优雅(缺省)或强杀 */
export function stopSession(id: string, force = false): Promise<void> {
  return sessionStop(id, force);
}

/** 尺寸校正:TerminalPane fit 后上报,这里落到后端(80x24 创建的抖动由此消化) */
export function resizeSession(id: string, cols: number, rows: number): void {
  void sessionResize(id, cols, rows).catch(logInvokeError("session_resize"));
}
