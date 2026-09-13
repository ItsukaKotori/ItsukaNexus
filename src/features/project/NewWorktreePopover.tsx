// 项目节点「+」:建 worktree 小弹层(基线 ref + 建完自动开终端)。
// 孤儿回滚(必办#2):worktree 建好而会话失败时,刚建 worktree 无用户数据,
// best-effort 连分支一起回收。
import { useCallback, useEffect, useState } from "react";
import { Plus } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";

import {
  sessionCreate,
  worktreeCreate,
  worktreeRemove,
} from "../../ipc/commands";
import type { SessionSnapshot, WorktreeInfo } from "../../ipc/types";
import { useProjects } from "../../stores/projectStore";
import { useSessions } from "../../stores/sessionsStore";
import { useTabs } from "../../stores/tabStore";

interface Props {
  repoPath: string;
  /** 建完的后续(选中该 worktree 节点等)由组件内部完成 */
  onDone?: () => void;
}

export default function NewWorktreePopover({ repoPath, onDone }: Props) {
  const [open, setOpen] = useState(false);
  const [baseRef, setBaseRef] = useState("");
  const [autoTerminal, setAutoTerminal] = useState(true);
  const [submitting, setSubmitting] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    if (open) {
      setBaseRef("");
      setAutoTerminal(true);
      setErr(null);
    }
  }, [open]);

  const submit = useCallback(async () => {
    setSubmitting(true);
    setErr(null);
    let wt: WorktreeInfo | null = null;
    try {
      wt = await worktreeCreate(repoPath, "shell", baseRef.trim() || undefined);
      useProjects.getState().select({
        repoPath,
        worktreeName: wt.name,
      });
      if (autoTerminal) {
        const created = await sessionCreate("shell", 80, 24, {
          repoPath,
          worktreeName: wt.name,
        });
        const snap: SessionSnapshot = {
          sessionId: created.sessionId,
          state: created.state,
          startedAtMs: Date.now(),
          exitCode: null,
          pid: null,
          repoPath,
          worktreeName: wt.name,
        };
        useSessions.getState().add(snap);
        useTabs.getState().openTerminal(snap);
      }
      setOpen(false);
      onDone?.();
    } catch (e) {
      // 孤儿回滚:worktree 已建而会话失败——刚建无破坏性,直接回收
      if (wt) {
        await worktreeRemove(repoPath, wt.name, true).catch((rmErr) =>
          console.error("[worktree] 孤儿回滚失败,请手动清理", wt?.name, rmErr)
        );
      }
      setErr(`创建失败:${String(e)}`);
    } finally {
      setSubmitting(false);
    }
  }, [repoPath, baseRef, autoTerminal, onDone]);

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <span
          role="button"
          aria-label="新建 worktree"
          title="新建 worktree"
          className="hidden shrink-0 rounded p-0.5 text-muted-foreground/60 hover:bg-secondary hover:text-foreground group-hover/project:block"
        >
          <Plus className="size-3" />
        </span>
      </PopoverTrigger>
      <PopoverContent className="w-72" align="start">
        <div className="grid gap-3">
          <p className="text-xs font-semibold">新建 worktree</p>
          <div className="grid gap-1.5">
            <Label htmlFor="base-ref" className="text-xs">
              基线 ref(默认 HEAD)
            </Label>
            <Input
              id="base-ref"
              value={baseRef}
              placeholder="main"
              onChange={(e) => setBaseRef(e.target.value)}
            />
          </div>
          <div className="flex items-center gap-2">
            <Checkbox
              id="auto-term"
              checked={autoTerminal}
              onCheckedChange={(v) => setAutoTerminal(v === true)}
            />
            <Label htmlFor="auto-term" className="text-xs">
              建完自动开终端
            </Label>
          </div>
          {err && <p className="text-xs text-destructive">{err}</p>}
          <Button size="sm" onClick={() => void submit()} disabled={submitting}>
            {submitting ? "创建中…" : "创建"}
          </Button>
        </div>
      </PopoverContent>
    </Popover>
  );
}
