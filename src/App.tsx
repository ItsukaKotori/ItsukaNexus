// M1 UI:顶栏(标题 + 应用信息 + 新建/停止)+ 全屏单终端。
// M2 引入 zustand 与多 tab 后,这里瘦身成 AppShell 布局。
import { useCallback, useRef, useState } from "react";

import TerminalPane from "./features/terminal/TerminalPane";
import { getAppInfo, sessionCreate, sessionStop } from "./ipc/commands";
import type { AppInfo } from "./ipc/types";

function App() {
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [sessionId, setSessionId] = useState<string | null>(null);
  const [status, setStatus] = useState<string>("无会话");
  const [error, setError] = useState<string | null>(null);
  // StrictMode 下 effect 双执行会让 onReady 触发两次;ref 守卫保证只 create 一次
  const creatingRef = useRef(false);

  const handleTerminalReady = useCallback(
    async (cols: number, rows: number) => {
      if (creatingRef.current) return;
      creatingRef.current = true;
      try {
        setError(null);
        const created = await sessionCreate("shell", cols, rows);
        setSessionId(created.sessionId);
        setStatus("运行中");
        if (!info) {
          try {
            setInfo(await getAppInfo());
          } catch {
            /* 顶栏信息拿不到不影响终端 */
          }
        }
      } catch (e) {
        setError(String(e));
        setStatus("创建失败");
      } finally {
        creatingRef.current = false;
      }
    },
    [info]
  );

  const handleStop = useCallback(async () => {
    if (!sessionId) return;
    try {
      await sessionStop(sessionId);
    } catch (e) {
      setError(String(e));
    }
    setSessionId(null);
    setStatus("无会话");
  }, [sessionId]);

  const handleExit = useCallback(() => {
    setSessionId(null);
    setStatus("已退出");
  }, []);

  return (
    <main
      style={{
        display: "flex",
        flexDirection: "column",
        height: "100vh",
        fontFamily: "system-ui",
      }}
    >
      <header
        style={{
          display: "flex",
          alignItems: "center",
          gap: 12,
          padding: "8px 16px",
          borderBottom: "1px solid #ddd",
        }}
      >
        <strong>ItsukaNexus</strong>
        {info && (
          <span style={{ color: "#888", fontSize: 13 }}>
            v{info.version} · {info.platform}
          </span>
        )}
        <span style={{ flex: 1 }} />
        <span style={{ fontSize: 13 }}>{status}</span>
        <button onClick={handleStop} disabled={!sessionId}>
          停止
        </button>
      </header>
      <div style={{ flex: 1, minHeight: 0, padding: 4 }}>
        <TerminalPane
          sessionId={sessionId}
          onReady={handleTerminalReady}
          onExit={handleExit}
        />
      </div>
      {error && (
        <footer style={{ color: "red", padding: "4px 16px", fontSize: 13 }}>
          {error}
        </footer>
      )}
    </main>
  );
}

export default App;
