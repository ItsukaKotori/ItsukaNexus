// 终端面板:容器 div + xterm 实例生命周期 + 输入/resize 双向流。
// 性能红线(spec §1.5):输出数据不经 React state,事件回调直达 term.write。
import { useEffect, useRef } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";

import { sessionResize, sessionSendInput } from "../../ipc/commands";
import { onSessionExit, onSessionOutput } from "../../ipc/events";

interface Props {
  sessionId: string | null;
  /** fit 实测出初始尺寸后回调(父组件此时才 session_create) */
  onReady: (cols: number, rows: number) => void;
  /** 会话进程退出回调 */
  onExit: (code: number) => void;
}

const RESIZE_DEBOUNCE_MS = 100;

export default function TerminalPane({ sessionId, onReady, onExit }: Props) {
  const containerRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  // 当前会话 id 的 ref 镜像:输入/resize 回调要读"最新"值,避免闭包过期
  const sessionIdRef = useRef<string | null>(null);
  sessionIdRef.current = sessionId;

  // effect 1:终端实例与容器尺寸观察(挂载一次;StrictMode 双执行靠 cleanup 配对)
  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    const term = new Terminal({
      fontFamily: "Menlo, Monaco, 'Courier New', monospace",
      fontSize: 13,
      cursorBlink: true,
      scrollback: 5000,
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(container);
    fit.fit();
    termRef.current = term;

    // 初始尺寸上报 → 父组件 session_create(避免 80x24 默认的重排抖动)
    onReady(term.cols, term.rows);

    term.onData((data) => {
      const id = sessionIdRef.current;
      if (id) void sessionSendInput(id, data);
    });

    let resizeTimer: ReturnType<typeof setTimeout> | undefined;
    const ro = new ResizeObserver(() => {
      clearTimeout(resizeTimer);
      resizeTimer = setTimeout(() => {
        fit.fit();
        const id = sessionIdRef.current;
        if (id) void sessionResize(id, term.cols, term.rows);
      }, RESIZE_DEBOUNCE_MS);
    });
    ro.observe(container);

    return () => {
      clearTimeout(resizeTimer);
      ro.disconnect();
      term.dispose();
      termRef.current = null;
    };
    // onReady 故意不进依赖:仅挂载时上报一次
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // effect 2:会话输出/退出订阅(sessionId 变化时重订)
  useEffect(() => {
    if (!sessionId) return;
    let alive = true;
    let unOutput: (() => void) | undefined;
    let unExit: (() => void) | undefined;

    void onSessionOutput(sessionId, (data) => {
      termRef.current?.write(data);
    }).then((u) => {
      if (alive) unOutput = u;
      else u();
    });

    void onSessionExit(sessionId, (code) => {
      termRef.current?.writeln(
        `\x1b[90m[进程已退出,退出码 ${code}]\x1b[0m`
      );
      onExit(code);
    }).then((u) => {
      if (alive) unExit = u;
      else u();
    });

    return () => {
      alive = false;
      unOutput?.();
      unExit?.();
    };
    // onExit 故意不进依赖:行为只依赖挂载时的 props 语义
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sessionId]);

  return (
    <div
      ref={containerRef}
      style={{ width: "100%", height: "100%", minWidth: 0, minHeight: 0 }}
    />
  );
}
