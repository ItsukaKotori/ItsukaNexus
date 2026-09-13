// tab 视图层(spec §1.5):session 是数据(sessionsStore),tab 是视图(这里)。
// 单一 tab 模型:kind 可扩展(M4 仅 terminal);id 稳定 = `term-<sessionId>`。
// 不做 tab 持久化:重启由 session_list 重建(rebuildFromSessions)。
import { create } from "zustand";

import type { SessionSnapshot } from "../ipc/types";

export type TabKind = "terminal";

export interface Tab {
  id: string;
  kind: TabKind;
  sessionId: string;
  label: string;
  /** 会话快照的落点字段镜像(树上挂节点/新建终端 cwd 决策用) */
  repoPath: string | null;
  worktreeName: string | null;
}

function tabOf(snap: SessionSnapshot, label: string): Tab {
  return {
    id: `term-${snap.sessionId}`,
    kind: "terminal",
    sessionId: snap.sessionId,
    label,
    repoPath: snap.repoPath ?? null,
    worktreeName: snap.worktreeName ?? null,
  };
}

function lowestFreeLabel(taken: string[]): string {
  let n = 1;
  while (taken.includes(`终端 ${n}`)) n += 1;
  return `终端 ${n}`;
}

interface TabState {
  tabs: Tab[];
  activeTabId: string | null;
  /** 新会话落 tab(已存在则仅激活) */
  openTerminal: (snap: SessionSnapshot) => void;
  setActive: (id: string | null) => void;
  /** 关 tab 视图(会话数据的清理由调用方——App 的 dispose 流程——负责) */
  closeTab: (id: string) => void;
  /** 启动恢复:按快照序(后端已按 startedAtMs 排)重建终端 tab */
  rebuildFromSessions: (snaps: SessionSnapshot[]) => void;
}

export const useTabs = create<TabState>((set) => ({
  tabs: [],
  activeTabId: null,
  openTerminal: (snap) =>
    set((st) => {
      const id = `term-${snap.sessionId}`;
      if (st.tabs.some((t) => t.id === id)) return { activeTabId: id };
      const label = lowestFreeLabel(st.tabs.map((t) => t.label));
      return { tabs: [...st.tabs, tabOf(snap, label)], activeTabId: id };
    }),
  setActive: (id) => set({ activeTabId: id }),
  closeTab: (id) =>
    set((st) => {
      const idx = st.tabs.findIndex((t) => t.id === id);
      if (idx < 0) return st;
      const tabs = st.tabs.filter((t) => t.id !== id);
      const activeTabId =
        st.activeTabId === id
          ? (tabs[Math.max(0, idx - 1)]?.id ?? null)
          : st.activeTabId;
      return { tabs, activeTabId };
    }),
  rebuildFromSessions: (snaps) =>
    set(() => {
      const taken: string[] = [];
      const tabs = snaps.map((s) => {
        const label = lowestFreeLabel(taken);
        taken.push(label);
        return tabOf(s, label);
      });
      return { tabs, activeTabId: tabs[0]?.id ?? null };
    }),
}));
