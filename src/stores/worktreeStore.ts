// worktree 域 store(spec §1.2):repo 校验结果 + worktree 列表缓存,
// worktree://changed 事件驱动刷新(M3 最小形态:M4 fleet 再扩展)。
import { create } from "zustand";

import {
  gitValidateRepo,
  worktreeList,
  worktreeRemove,
} from "../ipc/commands";
import type { RepoInfo, WorktreeInfo } from "../ipc/types";

interface RepoEntry {
  info: RepoInfo | null;
  worktrees: WorktreeInfo[];
}

interface WorktreeState {
  repos: Record<string, RepoEntry>;
  /** 校验并加载一个 repo(目录选择/手输后调用) */
  loadRepo: (path: string) => Promise<void>;
  refresh: (path: string) => Promise<void>;
  /** 事件联动:同名 worktree 列表失效重拉 */
  applyChange: (ev: { repoPath: string }) => void;
  removeWorktree: (
    repoPath: string,
    name: string,
    deleteBranch: boolean
  ) => Promise<void>;
}

export const useWorktrees = create<WorktreeState>((set, get) => ({
  repos: {},
  loadRepo: async (path) => {
    try {
      const info = await gitValidateRepo(path);
      const worktrees = await worktreeList(path).catch(() => []);
      set((st) => ({ repos: { ...st.repos, [path]: { info, worktrees } } }));
    } catch (e) {
      console.error("[worktree] 校验失败", path, e);
      set((st) => ({ repos: { ...st.repos, [path]: { info: null, worktrees: [] } } }));
    }
  },
  refresh: async (path) => {
    const worktrees = await worktreeList(path).catch(() => []);
    set((st) => ({
      repos: { ...st.repos, [path]: { info: st.repos[path]?.info ?? null, worktrees } },
    }));
  },
  applyChange: (ev) => {
    if (get().repos[ev.repoPath]) void get().refresh(ev.repoPath);
  },
  removeWorktree: async (repoPath, name, deleteBranch) => {
    await worktreeRemove(repoPath, name, deleteBranch);
    await get().refresh(repoPath);
  },
}));
