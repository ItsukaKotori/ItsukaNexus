// 多类型标签条(原型 tabstrip):状态点 + 标题 + kind 徽标 + 关闭钮 + 「+」。
// 数据:tabStore(视图)+ sessionsStore(状态点);关闭回调走 App 的 M3
// dispose 流程(handleClose);「+」即时建终端,不弹对话框(orca 哲学)。
import { useCallback } from "react";
import { Plus } from "lucide-react";

import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";

import { sessionCreate } from "../../ipc/commands";
import type { SessionSnapshot, SessionState } from "../../ipc/types";
import { useProjects } from "../../stores/projectStore";
import { useSessions } from "../../stores/sessionsStore";
import { useTabs } from "../../stores/tabStore";
import { toast } from "../../stores/toastStore";

const DOT: Record<SessionState, string> = {
  running: "bg-status-run",
  stopping: "bg-status-warn",
  exited: "bg-gray-400",
  failed: "bg-status-err",
};

interface Props {
  onClose: (sessionId: string) => void;
}

export default function TabStrip({ onClose }: Props) {
  const tabs = useTabs((s) => s.tabs);
  const activeTabId = useTabs((s) => s.activeTabId);
  const setActive = useTabs((s) => s.setActive);
  const sessions = useSessions((s) => s.sessions);
  const selected = useProjects((s) => s.selected);
  const selectedLabel = selected
    ? selected.worktreeName
      ? selected.worktreeName.split("/").pop()
      : selected.repoPath.split("/").pop()
    : null;

  /** orca 哲学:无对话框即时创建;cwd = 项目树选中节点 */
  const newTerminal = useCallback(async () => {
    if (!selected) return;
    const opts = selected.worktreeName
      ? { repoPath: selected.repoPath, worktreeName: selected.worktreeName }
      : { repoPath: selected.repoPath };
    try {
      const created = await sessionCreate("shell", 80, 24, opts);
      const snap: SessionSnapshot = {
        sessionId: created.sessionId,
        state: created.state,
        startedAtMs: Date.now(),
        exitCode: null,
        pid: null,
        repoPath: selected.repoPath,
        worktreeName: selected.worktreeName ?? null,
      };
      useSessions.getState().add(snap);
      useTabs.getState().openTerminal(snap);
    } catch (e) {
      toast(`新建终端失败:${String(e)}`, "error");
    }
  }, [selected]);

  return (
    <div className="flex min-h-[37px] items-stretch border-b border-border">
      <div className="flex min-w-0 items-stretch overflow-x-auto [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
        {tabs.map((tab) => {
          const state = sessions[tab.sessionId]?.state;
          const active = tab.id === activeTabId;
          return (
            // 外层用 div[role=tab]:关闭钮(role=button)不能嵌在真 <button> 里
            <div
              key={tab.id}
              role="tab"
              aria-selected={active}
              tabIndex={0}
              onClick={() => setActive(tab.id)}
              onKeyDown={(e) => {
                if (e.key === "Enter" || e.key === " ") {
                  e.preventDefault();
                  setActive(tab.id);
                }
              }}
              title={tab.repoPath ?? tab.sessionId}
              className={`flex max-w-[232px] cursor-pointer items-center gap-1.5 border-r border-border px-2.5 text-xs whitespace-nowrap select-none ${
                active
                  ? "border-t-2 border-t-focus bg-card text-foreground"
                  : "text-muted-foreground hover:bg-accent hover:text-foreground"
              }`}
            >
              {state && (
                <span className={`size-1.5 shrink-0 rounded-full ${DOT[state]}`} />
              )}
              <span className="truncate">{tab.label}</span>
              <span className="font-mono text-[10px] text-muted-foreground/70">
                终端
              </span>
              <span
                role="button"
                aria-label="关闭标签页"
                title="关闭标签页"
                tabIndex={0}
                onKeyDown={(e) => {
                  if (e.key === "Enter" || e.key === " ") {
                    e.preventDefault();
                    e.stopPropagation();
                    onClose(tab.sessionId);
                  }
                }}
                onClick={(e) => {
                  e.stopPropagation();
                  onClose(tab.sessionId);
                }}
                className="grid size-4 place-items-center rounded text-muted-foreground/60 hover:bg-secondary hover:text-foreground"
              >
                ×
              </span>
            </div>
          );
        })}
      </div>
      <div className="ml-auto flex items-center pr-2">
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <button
              type="button"
              title="新建标签页"
              aria-label="新建标签页"
              className="grid size-6 place-items-center rounded text-muted-foreground hover:bg-accent hover:text-foreground"
            >
              <Plus className="size-4" />
            </button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end">
            <DropdownMenuItem disabled={!selected} onClick={() => void newTerminal()}>
              新建终端{selectedLabel ? ` · ${selectedLabel}` : ""}
            </DropdownMenuItem>
            {!selected && (
              <p className="px-2 py-1 text-[11px] text-muted-foreground">
                先在左侧打开并选中一个项目
              </p>
            )}
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
    </div>
  );
}
