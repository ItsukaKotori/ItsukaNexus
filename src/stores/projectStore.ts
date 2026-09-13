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
    // 后端返回该工作区全部条目(新入册 + 已在册重挂);错误向上抛给 UI
    const entries = await projectAdd(path);
    let firstNew: ProjectEntry | undefined;
    set((st) => {
      const byId = new Map(st.projects.map((p) => [p.id, p] as const));
      const knownPaths = new Set(st.projects.map((p) => p.path));
      for (const e of entries) {
        if (!knownPaths.has(e.path) && !firstNew) firstNew = e;
        byId.set(e.id, e); // 去重 by id:已在册条目以其重挂后的 workspace 覆盖
      }
      const projects = [...byId.values()].sort(
        (a, b) => a.addedAtMs - b.addedAtMs || (a.path < b.path ? -1 : a.path > b.path ? 1 : 0)
      );
      return { projects };
    });
    await Promise.all(entries.map((e) => get().refresh(e.path)));
    // 选中第一个新条目;整单皆旧(重复打开同工作区)回落首个,与旧选中语义一致
    const hit = firstNew ?? entries[0];
    if (!hit) throw new Error("project_add 返回空列表"); // 后端成功必非空,防御性兜底
    get().select({ repoPath: hit.path });
    return hit;
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
