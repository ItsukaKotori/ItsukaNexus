// 终端面板:容器 div + manager 实例编排 + 尺寸观察。
// M2 形态:组件只藏不卸(App 以 display 切换),实例持有在 terminalManager;
// 输出不经 React——Channel 直达 term.write(spec §1.5 红线)。
import { useEffect, useRef } from "react";
import "@xterm/xterm/css/xterm.css";

import {
  attachSession,
  createEntry,
  getEntry,
  openAndFit,
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

  // effect:实例挂载 + attach + 尺寸观察(sessionId 变化即重建编排;
  // cleanup 不 dispose 实例——tab 只藏不卸,释放只在 App closeTab)
  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    let entry = getEntry(sessionId);
    if (!entry) entry = createEntry(sessionId);

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
    <div
      ref={containerRef}
      style={{ width: "100%", height: "100%", minWidth: 0, minHeight: 0 }}
    />
  );
}
