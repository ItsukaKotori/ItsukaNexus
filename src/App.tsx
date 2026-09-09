import { useState } from "react";
import { getAppInfo } from "./ipc/commands";
import type { AppInfo } from "./ipc/types";

function App() {
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function handleLoad() {
    try {
      setInfo(await getAppInfo());
      setError(null);
    } catch (e) {
      // 先清空旧数据，避免上一次成功的 info 与本次错误同时展示
      setInfo(null);
      // Tauri invoke 抛的是 Error，但保险起见兜底非 Error 值
      setError(e instanceof Error ? e.message : String(e));
    }
  }

  return (
    <main style={{ display: "grid", placeItems: "center", minHeight: "100vh", fontFamily: "system-ui" }}>
      <div style={{ textAlign: "center" }}>
        <h1>ItsukaNexus</h1>
        <p>M0 · Agent Development Environment</p>
        <button onClick={handleLoad}>获取应用信息</button>
        {info && (
          <ul style={{ listStyle: "none", padding: 0 }}>
            <li>name: {info.name}</li>
            <li>version: {info.version}</li>
            <li>platform: {info.platform}</li>
          </ul>
        )}
        {error && <p style={{ color: "red" }}>错误: {error}</p>}
      </div>
    </main>
  );
}

export default App;
