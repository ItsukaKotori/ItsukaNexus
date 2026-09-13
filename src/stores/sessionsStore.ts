// 会话单 store(zustand):Rust 会话表是权威,这里是 UI 投影。
// 全局事件在 React 外到达(App 订阅后转发进 store),输出流不经过这里。
// M4:这里只剩会话数据(快照表)——激活态/标签序归 tabStore(视图)。
import { create } from "zustand";

import type {
  SessionExitEvent,
  SessionSnapshot,
  SessionState,
} from "../ipc/types";

interface SessionsState {
  sessions: Record<string, SessionSnapshot>;
  /** 启动恢复:session_list 结果填充 */
  hydrate: (snaps: SessionSnapshot[]) => void;
  add: (snap: SessionSnapshot) => void;
  onState: (sessionId: string, next: SessionState) => void;
  onExit: (ev: SessionExitEvent) => void;
  /** dispose 收尾:从快照表移除(Rust 侧条目已删) */
  removeSession: (id: string) => void;
}

export const useSessions = create<SessionsState>((set) => ({
  sessions: {},
  hydrate: (snaps) =>
    set(() => ({
      sessions: Object.fromEntries(snaps.map((s) => [s.sessionId, s])),
    })),
  add: (snap) =>
    set((st) => ({
      sessions: { ...st.sessions, [snap.sessionId]: snap },
    })),
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
  removeSession: (id) =>
    set((st) => {
      const sessions = { ...st.sessions };
      delete sessions[id];
      return { sessions };
    }),
}));
