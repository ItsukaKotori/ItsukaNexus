// 布局派生钩子(spec §1.5,orca useAppChromeLayout 最小版):
// 回答"侧栏/右面板是否展开";开合持久化到 localStorage。
import { useCallback, useState } from "react";

function readFlag(key: string): boolean {
  return localStorage.getItem(key) !== "0";
}

export function useWorkbenchLayout() {
  const [sideOpen, setSideOpen] = useState(() => readFlag("nx.sideOpen"));
  const [ctxOpen, setCtxOpen] = useState(() => readFlag("nx.ctxOpen"));
  const toggleSide = useCallback(() => {
    setSideOpen((v) => {
      localStorage.setItem("nx.sideOpen", v ? "0" : "1");
      return !v;
    });
  }, []);
  const toggleCtx = useCallback(() => {
    setCtxOpen((v) => {
      localStorage.setItem("nx.ctxOpen", v ? "0" : "1");
      return !v;
    });
  }, []);
  return { sideOpen, ctxOpen, toggleSide, toggleCtx };
}
