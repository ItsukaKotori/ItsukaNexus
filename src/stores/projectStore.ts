// 项目域 store(spec §1.2):注册表镜像 + 每项目 git 状态/worktree 列表 + 选中节点。
// 选中节点是「新建终端」的 cwd 决策依据(T7);树结构不进 store(组件内派生)。
import { create } from "zustand";

import {
  gitValidateRepo,
  projectAdd,
  projectList,
  projectRemove,
  worktreeList,
} from "../ipc/commands";
import type { ProjectEntry, RepoInfo, WorktreeInfo } from "../ipc/types";

export interface ProjectDetail {
  info: RepoInfo | null;
  worktrees: WorktreeInfo[];
}

export interface Selection {
  repoPath: string;
  worktreeName?: string;
}

interface ProjectState {
  projects: ProjectEntry[];
  detail: Record<string, ProjectDetail>;
  selected: Selection | null;
  loadAll: () => Promise<void>;
  refresh: (repoPath: string) => Promise<void>;
  addProject: (path: string) => Promise<ProjectEntry>;
  removeProject: (id: string) => Promise<void>;
  select: (node: Selection) => void;
  /** worktree://changed 联动:该 repo 的列表失效重拉 */
  applyWorktreeChange: (ev: { repoPath: string }) => void;
}

export const useProjects = create<ProjectState>((set, get) => ({
  projects: [],
  detail: {},
  selected: null,
  loadAll: async () => {
    try {
      const projects = await projectList();
      set({ projects });
      await Promise.all(projects.map((p) => get().refresh(p.path)));
    } catch (e) {
      console.error("[project] project_list 失败", e);
    }
  },
  refresh: async (repoPath) => {
    try {
      const info = await gitValidateRepo(repoPath);
      const worktrees = await worktreeList(repoPath).catch(() => []);
      set((st) => ({ detail: { ...st.detail, [repoPath]: { info, worktrees } } }));
    } catch (e) {
      console.error("[project] 刷新失败", repoPath, e);
      set((st) => ({
        detail: { ...st.detail, [repoPath]: { info: null, worktrees: [] } },
      }));
    }
  },
  addProject: async (path) => {
    const entry = await projectAdd(path); // 非 git 目录等错误向上抛给 UI
    set((st) => ({
      projects: [...st.projects.filter((p) => p.id !== entry.id), entry].sort(
        (a, b) => a.addedAtMs - b.addedAtMs
      ),
    }));
    await get().refresh(entry.path);
    get().select({ repoPath: entry.path });
    return entry;
  },
  removeProject: async (id) => {
    await projectRemove(id);
    const st = get();
    const hit = st.projects.find((p) => p.id === id);
    set((cur) => {
      const detail = { ...cur.detail };
      if (hit) delete detail[hit.path];
      return {
        projects: cur.projects.filter((p) => p.id !== id),
        detail,
        selected:
          hit && cur.selected?.repoPath === hit.path ? null : cur.selected,
      };
    });
  },
  select: (node) => set({ selected: node }),
  applyWorktreeChange: (ev) => {
    if (get().detail[ev.repoPath]) void get().refresh(ev.repoPath);
  },
}));
