// 会话单 store(zustand):Rust 会话表是权威,这里是 UI 投影。
// 全局事件在 React 外到达(App 订阅后转发进 store),输出流不经过这里。
import { create } from "zustand";

import type {
  SessionExitEvent,
  SessionSnapshot,
  SessionState,
} from "../ipc/types";

interface SessionsState {
  sessions: Record<string, SessionSnapshot>;
  activeId: string | null;
  /** 启动恢复:session_list 结果填充 */
  hydrate: (snaps: SessionSnapshot[]) => void;
  add: (snap: SessionSnapshot) => void;
  setActive: (id: string | null) => void;
  onState: (sessionId: string, next: SessionState) => void;
  onExit: (ev: SessionExitEvent) => void;
  /** 本地移除 tab(会话可能已退出;条目仍留在 Rust 侧列表里) */
  closeTab: (id: string) => void;
}

export const useSessions = create<SessionsState>((set) => ({
  sessions: {},
  activeId: null,
  hydrate: (snaps) =>
    set(() => ({
      sessions: Object.fromEntries(snaps.map((s) => [s.sessionId, s])),
      activeId: snaps.find((s) => s.state === "running")?.sessionId ?? snaps[0]?.sessionId ?? null,
    })),
  add: (snap) =>
    set((st) => ({
      sessions: { ...st.sessions, [snap.sessionId]: snap },
      activeId: snap.sessionId,
    })),
  setActive: (id) => set({ activeId: id }),
  onState: (sessionId, next) =>
    set((st) => {
      const cur = st.sessions[sessionId];
      if (!cur) return st;
      // 终态吸收:Exited/Failed 后不再接受状态更新(镜像后端 can_transition_to)
      if (cur.state === "exited" || cur.state === "failed") return st;
      return { sessions: { ...st.sessions, [sessionId]: { ...cur, state: next } } };
    }),
  onExit: (ev) =>
    set((st) => {
      const cur = st.sessions[ev.sessionId];
      if (!cur) return st;
      return {
        sessions: {
          ...st.sessions,
          [ev.sessionId]: { ...cur, state: "exited", exitCode: ev.code },
        },
      };
    }),
  closeTab: (id) =>
    set((st) => {
      const sessions = { ...st.sessions };
      delete sessions[id];
      const ids = Object.keys(sessions);
      return {
        sessions,
        activeId: st.activeId === id ? (ids[ids.length - 1] ?? null) : st.activeId,
      };
    }),
}));
