// 项目树(原型 side TREE 结构):工作区 > 项目(branch 徽标) > worktree > session。
// 工作区 = 添加目录(D 自身 / 其 git 子目录各为一个项目行,见 registry add);
// 三个数据源在组件内拼装:projects/detail(projectStore)+ sessions(sessionsStore)。
// session 挂载规则:snap.repoPath === 项目 path 时,有 worktreeName 挂对应
// worktree 下,否则挂项目根;不匹配任何项目的会话只出现在标签里(不进树)。
// 工作区最后一个项目被移除后分组自然消失(纯派生,无持久化折叠态)。
import { useMemo, useState } from "react";
import { ChevronDown, Folder, FolderGit2, X } from "lucide-react";

import NewWorktreePopover from "./NewWorktreePopover";
import type { ProjectEntry, SessionSnapshot } from "../../ipc/types";
import { useProjects } from "../../stores/projectStore";
import { useSessions } from "../../stores/sessionsStore";
import { useTabs } from "../../stores/tabStore";
import { toast } from "../../stores/toastStore";

// 会话状态点(原型 .dot):run 蓝 + 3px 柔光环(color-mix 22%),其余纯色
const RUN_GLOW =
  "shadow-[0_0_0_3px_color-mix(in_oklch,var(--status-run)_22%,transparent)]";
const DOT: Record<string, string> = {
  running: `bg-status-run ${RUN_GLOW}`,
  stopping: "bg-status-warn",
  exited: "bg-gray-400",
  failed: "bg-status-err",
};

function basename(p: string): string {
  return p.split("/").filter(Boolean).pop() ?? p;
}

// 分组:按 workspace 聚合;组序按组内最早 addedAtMs(组序稳定),组内按 addedAtMs
// (同毫秒以 path 为次级键,与 store 合并序一致,防抖)
interface WorkspaceGroup {
  workspace: string;
  list: ProjectEntry[];
}

function groupByWorkspace(projects: ProjectEntry[]): WorkspaceGroup[] {
  const byPath = (a: ProjectEntry, b: ProjectEntry) =>
    a.addedAtMs - b.addedAtMs || (a.path < b.path ? -1 : a.path > b.path ? 1 : 0);
  const map = new Map<string, ProjectEntry[]>();
  for (const p of projects) {
    const list = map.get(p.workspace) ?? [];
    list.push(p);
    map.set(p.workspace, list);
  }
  return [...map.entries()]
    .map(([workspace, list]) => ({ workspace, list: [...list].sort(byPath) }))
    .sort((a, b) => {
      const minA = Math.min(...a.list.map((p) => p.addedAtMs));
      const minB = Math.min(...b.list.map((p) => p.addedAtMs));
      return minA - minB || byPath(a.list[0], b.list[0]);
    });
}

function SessionRow({ snap }: { snap: SessionSnapshot }) {
  const selected = useProjects((s) => s.selected);
  const mine =
    selected?.repoPath === snap.repoPath &&
    selected?.worktreeName === snap.worktreeName;
  return (
    <button
      type="button"
      onClick={() => {
        useProjects.getState().select({
          repoPath: snap.repoPath ?? "",
          worktreeName: snap.worktreeName ?? undefined,
        });
        useTabs.getState().setActive(`term-${snap.sessionId}`);
      }}
      className={`flex w-full items-center gap-1.5 rounded-md py-1 pr-2 pl-[50px] text-left text-[11.5px] transition-colors duration-130 ${
        mine ? "bg-card text-foreground" : "text-muted-foreground hover:bg-accent"
      }`}
    >
      <span className={`size-1.5 shrink-0 rounded-full ${DOT[snap.state]}`} />
      <span className="truncate">终端 · {snap.worktreeName?.split("/").pop() ?? snap.repoPath?.split("/").pop() ?? snap.sessionId.slice(0, 8)}</span>
    </button>
  );
}

function ProjectRow({
  project,
  byWorktree,
}: {
  project: ProjectEntry;
  byWorktree: Map<string, SessionSnapshot[]>;
}) {
  const detail = useProjects((s) => s.detail[project.path]);
  const selected = useProjects((s) => s.selected);
  const removeProject = useProjects((s) => s.removeProject);
  const mine = selected?.repoPath === project.path && !selected?.worktreeName;
  const branch = detail?.info?.currentBranch;
  return (
    <div className="group/project">
      <button
        type="button"
        onClick={() =>
          useProjects.getState().select({ repoPath: project.path })
        }
        className={`flex h-8 w-full items-center gap-1.5 rounded-md px-2 text-left text-[12.5px] transition-colors duration-130 ${
          mine ? "bg-card text-foreground" : "text-foreground hover:bg-accent"
        }`}
      >
        <ChevronDown className="size-3.5 shrink-0 text-muted-foreground/70" />
        <FolderGit2 className="size-3.5 shrink-0 text-muted-foreground" />
        <span className="truncate">{project.name}</span>
        <span className="flex-1" />
        {branch && (
          <span className="shrink-0 rounded border border-border px-1 font-mono text-[9.5px] text-muted-foreground">
            {branch}
          </span>
        )}
        <NewWorktreePopover repoPath={project.path} />
        <span
          role="button"
          aria-label="从列表移除(不删除磁盘)"
          title="从列表移除(不删除磁盘)"
          onClick={(e) => {
            e.stopPropagation();
            void removeProject(project.id).catch((e) =>
              toast(`移除项目失败:${String(e)}`, "error")
            );
          }}
          className="hidden size-4 shrink-0 place-items-center rounded text-muted-foreground/60 hover:bg-secondary hover:text-foreground group-hover/project:grid"
        >
          <X className="size-3" />
        </span>
      </button>
      {/* worktree 子节点:过滤主 worktree(项目行即主检出);其下的 session 按
          byWorktree 匹配挂载(与项目根会话同构,不另造组件) */}
      {(detail?.worktrees ?? [])
        .filter((w) => w.path !== project.path)
        .map((w) => (
          <div key={w.path}>
            <button
              type="button"
              title={w.path}
              onClick={() =>
                useProjects
                  .getState()
                  .select({ repoPath: project.path, worktreeName: w.name })
              }
              className={`flex w-full items-center gap-1.5 rounded-md py-1 pr-2 pl-9 text-left text-[11px] transition-colors duration-130 ${
                selected?.repoPath === project.path && selected?.worktreeName === w.name
                  ? "bg-card text-foreground"
                  : "text-muted-foreground hover:bg-accent"
              }`}
            >
              <span className="size-1.5 shrink-0 rounded-full bg-focus/70" />
              <span className="truncate font-mono">{w.name.split("/").pop() ?? w.name}</span>
            </button>
            {(byWorktree.get(w.name) ?? []).map((s) => (
              <SessionRow key={s.sessionId} snap={s} />
            ))}
          </div>
        ))}
    </div>
  );
}

// 工作区头行(原型 TREE 分组):chevron + 文件夹 + basename + 项目计数;
// 点击折叠/展开(本地 state,默认展开;折叠 chevron 旋转 -90°)
function WorkspaceHeader({
  workspace,
  count,
  collapsed,
  onToggle,
}: {
  workspace: string;
  count: number;
  collapsed: boolean;
  onToggle: () => void;
}) {
  return (
    <button
      type="button"
      title={workspace}
      onClick={onToggle}
      className="flex h-8 w-full items-center gap-1.5 rounded-md px-2 text-left transition-colors duration-130 hover:bg-accent"
    >
      <ChevronDown
        className={`size-3.5 shrink-0 text-muted-foreground/70 transition-transform duration-130 ${
          collapsed ? "-rotate-90" : ""
        }`}
      />
      <Folder className="size-3.5 shrink-0 text-muted-foreground" />
      <span className="truncate text-[12.5px] font-semibold text-foreground">
        {basename(workspace)}
      </span>
      <span className="flex-1" />
      <span className="shrink-0 font-mono text-[10.5px] text-muted-foreground">
        {count}
      </span>
    </button>
  );
}

export default function ProjectTree() {
  const projects = useProjects((s) => s.projects);
  const sessions = useSessions((s) => s.sessions);
  const [collapsed, setCollapsed] = useState<Record<string, boolean>>({});

  const groups = useMemo(() => groupByWorkspace(projects), [projects]);

  if (projects.length === 0) {
    return (
      <p className="p-4 text-center text-[12.5px] text-muted-foreground">
        还没有项目——点右上角「打开目录」开始
      </p>
    );
  }
  return (
    <div className="flex-1 overflow-y-auto px-2 pt-2 pb-3.5">
      {groups.map(({ workspace, list }) => {
        const isCollapsed = !!collapsed[workspace];
        return (
          <div key={workspace} className="mb-1.5">
            <WorkspaceHeader
              workspace={workspace}
              count={list.length}
              collapsed={isCollapsed}
              onToggle={() =>
                setCollapsed((c) => ({ ...c, [workspace]: !c[workspace] }))
              }
            />
            {!isCollapsed &&
              list.map((p) => {
                const mine = Object.values(sessions).filter(
                  (s) => s.repoPath === p.path
                );
                const byWorktree = new Map<string, SessionSnapshot[]>();
                for (const s of mine) {
                  const key = s.worktreeName ?? "";
                  byWorktree.set(key, [...(byWorktree.get(key) ?? []), s]);
                }
                return (
                  // 组内项目整体缩进一级(+12px):ProjectRow / worktree / session 同步右移
                  <div key={p.id} className="pl-3">
                    <ProjectRow project={p} byWorktree={byWorktree} />
                    {/* 挂项目根的会话(无 worktree) */}
                    {(byWorktree.get("") ?? []).map((s) => (
                      <SessionRow key={s.sessionId} snap={s} />
                    ))}
                  </div>
                );
              })}
          </div>
        );
      })}
    </div>
  );
}
