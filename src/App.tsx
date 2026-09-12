// M2 AppShell:顶栏(应用信息 + 新建)+ 会话 tab 栏 + 常驻终端区。
// 所有会话的 pane 恒久挂载,display 切换可见性——输出直达 term.write,
// 状态经全局事件驱动 store,React 只负责壳(spec §1.5)。
import { useCallback, useEffect, useRef, useState } from "react";

import { Button } from "@/components/ui/button";

import TerminalPane from "./features/terminal/TerminalPane";
import TerminalTabs from "./features/terminal/TerminalTabs";
import {
  disposeEntry,
  resizeSession,
  setTerminalConfig,
  stopSession,
} from "./features/terminal/terminalManager";
import {
  getAppInfo,
  configGet,
  sessionCreate,
  sessionList,
} from "./ipc/commands";
import { onSessionExitEvent, onSessionStateEvent } from "./ipc/events";
import type { AppInfo } from "./ipc/types";
import { useSessions } from "./stores/sessionsStore";

function App() {
  const sessions = useSessions((s) => s.sessions);
  const activeId = useSessions((s) => s.activeId);
  const add = useSessions((s) => s.add);
  const setActive = useSessions((s) => s.setActive);
  const hydrate = useSessions((s) => s.hydrate);
  const closeTab = useSessions((s) => s.closeTab);

  const [info, setInfo] = useState<AppInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  // 启动流程只跑一次(StrictMode 下 effect 双执行)
  const bootedRef = useRef(false);
  // 停止在途的会话:关 tab 双击/停止中再点不去重入 stop(仅 Running 可停)
  const closingRef = useRef(new Set<string>());

  // 启动恢复:app_info → config 注入(先于任何终端实例创建)→ session_list
  useEffect(() => {
    if (bootedRef.current) return;
    bootedRef.current = true;
    void (async () => {
      try {
        setInfo(await getAppInfo());
      } catch (e) {
        console.error("[app] app_info 失败", e);
      }
      try {
        setTerminalConfig((await configGet()).terminal);
      } catch (e) {
        console.error("[app] config_get 失败,用内置默认终端配置", e);
      }
      try {
        hydrate(await sessionList());
      } catch (e) {
        console.error("[app] session_list 失败,按空会话启动", e);
      }
    })();
  }, []);

  // 全局事件 → store(订阅随组件生命周期;StrictMode 重挂会重订,事件幂等)
  useEffect(() => {
    const unState = onSessionStateEvent((sc) =>
      useSessions.getState().onState(sc.sessionId, sc.next)
    );
    const unExit = onSessionExitEvent((ev) =>
      useSessions.getState().onExit(ev)
    );
    return () => {
      void unState.then((u) => u());
      void unExit.then((u) => u());
    };
  }, []);

  // 新建:容器尺寸未知,先 80x24,pane 挂载 fit 后经 resizeSession 校正
  const handleNew = useCallback(() => {
    setError(null);
    void sessionCreate("shell", 80, 24)
      .then((created) => {
        add({
          sessionId: created.sessionId,
          state: created.state,
          startedAtMs: Date.now(),
          exitCode: null,
          pid: null,
        });
      })
      .catch((e: unknown) => setError(`创建会话失败:${String(e)}`));
  }, [add]);

  // pane fit 上报 → 后端 resize(80x24 创建的初始校正 + 窗口变化)
  const handleFitted = useCallback((id: string, cols: number, rows: number) => {
    resizeSession(id, cols, rows);
  }, []);

  // 关 tab:已退出/失败直接本地移除;活着先优雅停(stop 等进程退出才返回,
  // 停止期间徽章走 Stopping;退出事件到达时 store 已先行更新)
  const handleClose = useCallback(
    (id: string) => {
      const snap = useSessions.getState().sessions[id];
      if (closingRef.current.has(id)) return;

      const finish = (): void => {
        closingRef.current.delete(id);
        disposeEntry(id);
        closeTab(id);
      };

      if (!snap || snap.state === "exited" || snap.state === "failed") {
        finish();
        return;
      }
      closingRef.current.add(id);
      void (async () => {
        try {
          await stopSession(id);
        } catch {
          // 竞态兜底:进程已自行退出/已在停止中,后端会拒停——以 store 现状为准
          const cur = useSessions.getState().sessions[id];
          if (cur && cur.state !== "exited" && cur.state !== "failed") {
            closingRef.current.delete(id);
            setError(`停止会话失败:当前状态 ${cur.state}`);
            return;
          }
        }
        finish();
      })();
    },
    [closeTab]
  );

  const sessionIds = Object.keys(sessions);

  return (
    <main className="flex h-screen flex-col">
      <header className="flex items-center gap-3 border-b px-4 py-2">
        <strong>ItsukaNexus</strong>
        {info && (
          <span className="text-[13px] text-muted-foreground">
            v{info.version} · {info.platform}
          </span>
        )}
        <span className="flex-1" />
        <Button size="sm" onClick={handleNew}>
          新建会话
        </Button>
      </header>

      <TerminalTabs
        sessions={sessions}
        activeId={activeId}
        onSelect={setActive}
        onClose={handleClose}
      />

      <div className="min-h-0 flex-1 p-1">
        {sessionIds.map((id) => (
          <div
            key={id}
            className={id === activeId ? "block h-full w-full" : "hidden"}
          >
            <TerminalPane sessionId={id} onFitted={handleFitted} />
          </div>
        ))}
        {sessionIds.length === 0 && (
          <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
            暂无会话——点右上角「新建会话」开始
          </div>
        )}
      </div>

      {error && (
        <footer className="bg-destructive/10 px-4 py-1 text-sm text-red-400">
          {error}
        </footer>
      )}
    </main>
  );
}

export default App;
