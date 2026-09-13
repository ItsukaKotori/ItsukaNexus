// 终端面板:容器 div + manager 实例编排 + 尺寸观察 + 会话终态遮罩。
// M2 形态:组件只藏不卸(App 以 display 切换),实例持有在 terminalManager;
// 输出不经 React——Channel 直达 term.write(spec §1.5 红线)。
// M3:订阅会话状态驱动退出遮罩 + 输入门禁(xterm 无 readonly,靠 setClosed)。
import { useEffect, useRef } from "react";
import "@xterm/xterm/css/xterm.css";

import { useSessions } from "../../stores/sessionsStore";
import {
  attachSession,
  createEntry,
  getEntry,
  openAndFit,
  setClosed,
} from "./terminalManager";

interface Props {
  sessionId: string;
  /** fit 实测尺寸后上报(App 转发后端 session_resize) */
  onFitted: (id: string, cols: number, rows: number) => void;
}

const RESIZE_DEBOUNCE_MS = 100;

export default function TerminalPane({ sessionId, onFitted }: Props) {
  const containerRef = useRef<HTMLDivElement>(null);
  // 回调 ref 镜像:RO 回调读"最新"值,避免闭包过期
  const onFittedRef = useRef(onFitted);
  onFittedRef.current = onFitted;

  // running 才放行输入;stopping 也关闸(M-4:击键只会收获后端错误,顺手消除
  // console 噪音);undefined(store 未及)按活着处理。遮罩仍只对终态。
  const state = useSessions((s) => s.sessions[sessionId]?.state);
  const alive = state === "running" || state === undefined;
  const masked = state === "exited" || state === "failed";
  // 镜像给挂载 effect:实例创建时同步落门禁(启动恢复即终态的会话,
  // setClosed effect 先跑、实例尚不存在,必须在 createEntry 后补一次)
  const aliveRef = useRef(alive);
  aliveRef.current = alive;

  useEffect(() => {
    setClosed(sessionId, !alive);
  }, [sessionId, alive]);

  // effect:实例挂载 + attach + 尺寸观察(sessionId 变化即重建编排;
  // cleanup 不 dispose 实例——tab 只藏不卸,释放只在 App closeTab)
  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    let entry = getEntry(sessionId);
    if (!entry) entry = createEntry(sessionId);
    setClosed(sessionId, !aliveRef.current);

    // fit:容器可见才做(隐藏时字宽测量为 0,fit 出来的尺寸是垃圾)
    const fitNow = (): void => {
      if (container.clientWidth === 0 || container.clientHeight === 0) return;
      const dims = openAndFit(sessionId, container);
      if (dims) onFittedRef.current(sessionId, dims.cols, dims.rows);
    };
    fitNow();

    // ResizeObserver 防抖:藏→显切换、窗口拖拽都收敛到一次 fit + 上报
    let resizeTimer: ReturnType<typeof setTimeout> | undefined;
    const ro = new ResizeObserver(() => {
      clearTimeout(resizeTimer);
      resizeTimer = setTimeout(fitNow, RESIZE_DEBOUNCE_MS);
    });
    ro.observe(container);

    attachSession(sessionId, entry);

    return () => {
      clearTimeout(resizeTimer);
      ro.disconnect();
    };
  }, [sessionId]);

  return (
    <div className="relative h-full w-full min-h-0 min-w-0">
      <div
        ref={containerRef}
        className="h-full w-full min-h-0 min-w-0"
      />
      {masked && (
        <div className="absolute inset-0 z-10 flex items-center justify-center bg-background/70 text-sm text-muted-foreground">
          会话已结束(输入已禁用,可关闭 tab)
        </div>
      )}
    </div>
  );
}
