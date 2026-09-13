// 右侧 Git 面板(原型 ctx/git):分支 + 变更列表 + 暂存全部 + 提交。
// 刷新:选中项目变化/窗口聚焦/操作后/refresh 钮(spec §1.3 拉模式,无轮询)。
import { useCallback, useEffect, useState } from "react";
import { ArrowDown, ArrowUp, GitBranch, RefreshCw } from "lucide-react";

import { Button } from "@/components/ui/button";
import { onWorktreeChanged } from "../../ipc/events";
import { gitCommit, gitStage, gitStatus } from "../../ipc/commands";
import type { GitStatus, GitStatusEntry } from "../../ipc/types";
import { useProjects } from "../../stores/projectStore";
import { toast } from "../../stores/toastStore";

const LETTER: Record<string, { ch: string; cls: string }> = {
  modified: { ch: "M", cls: "text-status-warn" },
  added: { ch: "A", cls: "text-status-ok" },
  deleted: { ch: "D", cls: "text-status-err" },
  renamed: { ch: "R", cls: "text-muted-foreground" },
  copied: { ch: "C", cls: "text-muted-foreground" },
  untracked: { ch: "U", cls: "text-muted-foreground" },
  unmerged: { ch: "!", cls: "text-status-err" },
};

/** 状态字母:index 侧优先(先看到将要提交什么) */
function sideOf(e: GitStatusEntry): string | null {
  return (e.index ?? e.worktree) ?? null;
}

export default function GitPanel() {
  const selected = useProjects((s) => s.selected);
  const repoPath = selected?.repoPath ?? null;
  const [status, setStatus] = useState<GitStatus | null>(null);
  const [message, setMessage] = useState("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    if (!repoPath) {
      setStatus(null);
      return;
    }
    try {
      setStatus(await gitStatus(repoPath));
      setErr(null);
    } catch (e) {
      const msg = `状态获取失败:${String(e)}`;
      // 行内 err 区保留面板内反馈,toast 保证全局可见
      setErr(msg);
      toast(msg, "error");
    }
  }, [repoPath]);

  // 选中变化 / 窗口聚焦 / worktree 变更 → 重拉
  useEffect(() => {
    void refresh();
  }, [refresh]);
  useEffect(() => {
    const onFocus = (): void => void refresh();
    window.addEventListener("focus", onFocus);
    const un = onWorktreeChanged((ev) => {
      if (ev.repoPath === repoPath) void refresh();
    });
    return () => {
      window.removeEventListener("focus", onFocus);
      void un.then((u) => u());
    };
  }, [refresh, repoPath]);

  const stageAll = useCallback(async () => {
    if (!repoPath) return;
    setBusy(true);
    try {
      await gitStage(repoPath);
      await refresh();
    } catch (e) {
      const msg = `暂存失败:${String(e)}`;
      setErr(msg);
      toast(msg, "error");
    } finally {
      setBusy(false);
    }
  }, [repoPath, refresh]);

  const commit = useCallback(async () => {
    if (!repoPath || !message.trim()) return;
    setBusy(true);
    try {
      await gitCommit(repoPath, message);
      setMessage("");
      await refresh();
    } catch (e) {
      const msg = `提交失败:${String(e)}`;
      setErr(msg);
      toast(msg, "error");
    } finally {
      setBusy(false);
    }
  }, [repoPath, message, refresh]);

  if (!repoPath) {
    return (
      <p className="p-4 text-center text-xs text-muted-foreground">
        在左侧选择一个项目后显示 Git 状态
      </p>
    );
  }

  return (
    <div className="flex-1 overflow-y-auto">
      {/* 分支行 */}
      <div className="border-b border-border px-3 py-2.5">
        <div className="flex items-center gap-2 text-xs">
          <GitBranch className="size-3.5 text-muted-foreground" />
          <span className="truncate font-mono">
            {status?.branch ?? "…"}
          </span>
          {status && (status.ahead > 0 || status.behind > 0) && (
            <span className="flex items-center gap-1 font-mono text-muted-foreground">
              {status.ahead > 0 && (
                <span className="flex items-center gap-0.5">
                  <ArrowUp className="size-3" />
                  {status.ahead}
                </span>
              )}
              {status.behind > 0 && (
                <span className="flex items-center gap-0.5">
                  <ArrowDown className="size-3" />
                  {status.behind}
                </span>
              )}
            </span>
          )}
          <span className="flex-1" />
          <button
            type="button"
            title="刷新"
            onClick={() => void refresh()}
            className="grid size-6 place-items-center rounded text-muted-foreground hover:bg-accent hover:text-foreground"
          >
            <RefreshCw className="size-3.5" />
          </button>
        </div>
      </div>
      {/* 变更列表 */}
      <div className="px-2 py-2">
        <div className="flex items-center justify-between px-1 pb-1">
          <span className="text-xs font-semibold text-muted-foreground">
            变更文件 · {status?.entries.length ?? 0}
          </span>
          {status && status.entries.length > 0 && (
            <button
              type="button"
              onClick={() => void stageAll()}
              disabled={busy}
              className="text-[11px] text-muted-foreground hover:text-foreground"
            >
              暂存全部
            </button>
          )}
        </div>
        {status?.truncated && (
          <p className="mb-1 rounded bg-status-warn/10 px-2 py-1 text-[11px] text-status-warn">
            变更过多,仅显示前 2000 项
          </p>
        )}
        {status && status.entries.length === 0 && (
          <p className="px-2 py-3 text-xs text-muted-foreground">
            工作区干净,没有未提交的变更。
          </p>
        )}
        {(status?.entries ?? []).map((e) => {
          const side = sideOf(e);
          const meta = side ? LETTER[side] : null;
          return (
            <div
              key={`${e.path}-${e.origPath ?? ""}`}
              title={e.origPath ? `${e.origPath} → ${e.path}` : e.path}
              className="flex items-center gap-2 rounded px-2 py-1 font-mono text-[11px] hover:bg-accent"
            >
              <span className={`w-3 shrink-0 text-center font-semibold ${meta?.cls ?? ""}`}>
                {meta?.ch ?? "?"}
              </span>
              <span className="truncate">{e.path}</span>
            </div>
          );
        })}
      </div>
      {/* 提交区 */}
      <div className="border-t border-border p-3">
        <textarea
          value={message}
          onChange={(e) => setMessage(e.target.value)}
          placeholder="提交信息…"
          className="min-h-[60px] w-full resize-none rounded-md border border-input bg-card px-2.5 py-2 text-xs outline-none focus:border-ring focus:ring-2 focus:ring-ring/40"
        />
        <div className="mt-2 flex items-center justify-between">
          <span className="text-[11px] text-muted-foreground">
            提交只含已暂存内容
          </span>
          <Button
            size="sm"
            onClick={() => void commit()}
            disabled={busy || !message.trim() || (status?.entries.length ?? 0) === 0}
          >
            提交
          </Button>
        </div>
      </div>
      {err && (
        <p className="border-t border-border px-3 py-2 text-[11px] text-destructive">
          {err}
        </p>
      )}
    </div>
  );
}
