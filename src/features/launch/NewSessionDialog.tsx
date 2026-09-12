// 新建会话对话框:repo 选择(手输 + 原生目录浏览)+ git 校验 + 可选建 worktree。
// 提交流程:worktreeCreate(勾选时)→ sessionCreate(cwd 落 worktree)。
import { useCallback, useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";

import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";

import { sessionCreate, worktreeCreate } from "@/ipc/commands";
import { useSessions } from "@/stores/sessionsStore";
import { useWorktrees } from "@/stores/worktreeStore";

interface Props {
  open: boolean;
  onOpenChange: (v: boolean) => void;
  onError: (msg: string) => void;
}

export default function NewSessionDialog({
  open: isOpen,
  onOpenChange,
  onError,
}: Props) {
  const [repoPath, setRepoPath] = useState("");
  const [useWorktree, setUseWorktree] = useState(true);
  const [baseRef, setBaseRef] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const add = useSessions((s) => s.add);
  const repo = useWorktrees((s) => (repoPath ? s.repos[repoPath] : undefined));

  // 打开且路径非空时校验(防抖从简:路径变化即重校验)
  useEffect(() => {
    if (isOpen && repoPath) void useWorktrees.getState().loadRepo(repoPath);
  }, [isOpen, repoPath]);

  const browse = useCallback(async () => {
    const picked = await open({ directory: true, multiple: false });
    if (typeof picked === "string") setRepoPath(picked);
  }, []);

  const submit = useCallback(async () => {
    setSubmitting(true);
    try {
      let worktreeName: string | undefined;
      if (useWorktree && repo?.info) {
        const wt = await worktreeCreate(repoPath, "shell", baseRef || undefined);
        worktreeName = wt.name;
      }
      const created = await sessionCreate("shell", 80, 24, {
        repoPath: repo?.info ? repoPath : undefined,
        worktreeName,
      });
      add({
        sessionId: created.sessionId,
        state: created.state,
        startedAtMs: Date.now(),
        exitCode: null,
        pid: null,
        repoPath: repo?.info ? repoPath : null,
        worktreeName: worktreeName ?? null,
      });
      onOpenChange(false);
    } catch (e) {
      onError(`创建会话失败:${String(e)}`);
    } finally {
      setSubmitting(false);
    }
  }, [useWorktree, repo, repoPath, baseRef, add, onOpenChange, onError]);

  const repoState = !repoPath
    ? null
    : repo?.info
      ? `${repo.info.currentBranch ?? "detached"} · ${repo.info.isClean ? "干净" : "有未提交变更"}`
      : "不是有效的 git 仓库";

  return (
    <Dialog open={isOpen} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>新建会话</DialogTitle>
        </DialogHeader>
        <div className="grid gap-4 py-2">
          <div className="grid gap-2">
            <Label htmlFor="repo">仓库路径(可选)</Label>
            <div className="flex gap-2">
              <Input
                id="repo"
                value={repoPath}
                placeholder="/path/to/repo"
                onChange={(e) => setRepoPath(e.target.value)}
              />
              <Button variant="secondary" onClick={() => void browse()}>
                浏览…
              </Button>
            </div>
            {repoState && (
              <p className="text-xs text-muted-foreground">{repoState}</p>
            )}
          </div>
          <div className="flex items-center gap-2">
            <Checkbox
              id="wt"
              checked={useWorktree && !!repo?.info}
              disabled={!repo?.info}
              onCheckedChange={(v) => setUseWorktree(v === true)}
            />
            <Label htmlFor="wt">在 nexus worktree 中打开(默认建在 .nx-worktrees/)</Label>
          </div>
          <div className="grid gap-2">
            <Label htmlFor="base">基线 ref(可选,默认 HEAD)</Label>
            <Input
              id="base"
              value={baseRef}
              placeholder="main"
              onChange={(e) => setBaseRef(e.target.value)}
            />
          </div>
        </div>
        <DialogFooter>
          <Button onClick={() => void submit()} disabled={submitting}>
            {submitting ? "创建中…" : "创建"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
