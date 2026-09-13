// 打开目录(原型 openDirOverlay):手输 + 原生浏览 + 最近打开(=注册表)。
// 提交即 project_add(后端校验 git 仓库、归一 canonical、追加 exclude)。
import { useCallback, useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle,
} from "@/components/ui/dialog";

import { useProjects } from "../../stores/projectStore";
import { toast } from "../../stores/toastStore";

interface Props {
  openState: boolean;
  onOpenChange: (v: boolean) => void;
}

export default function OpenDirOverlay({ openState, onOpenChange }: Props) {
  const [path, setPath] = useState("");
  const [err, setErr] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const projects = useProjects((s) => s.projects);

  useEffect(() => {
    if (openState) {
      setPath("");
      setErr(null);
    }
  }, [openState]);

  const browse = useCallback(async () => {
    const picked = await open({ directory: true, multiple: false });
    if (typeof picked === "string") setPath(picked);
  }, []);

  const submit = useCallback(async () => {
    const p = path.trim();
    if (!p) return;
    setSubmitting(true);
    setErr(null);
    try {
      await useProjects.getState().addProject(p);
      onOpenChange(false);
    } catch (e) {
      // 行内红字留在弹层内,toast 保证全局可见
      const msg = `打开失败:${String(e)}`;
      setErr(msg);
      toast(msg, "error");
    } finally {
      setSubmitting(false);
    }
  }, [path, onOpenChange]);

  return (
    <Dialog open={openState} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>打开项目目录</DialogTitle>
        </DialogHeader>
        <div className="grid gap-3 py-1">
          <div className="flex gap-2">
            <Input
              value={path}
              placeholder="/path/to/repo(须为 git 仓库)"
              onChange={(e) => setPath(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") void submit();
              }}
            />
            <Button variant="secondary" onClick={() => void browse()}>
              浏览…
            </Button>
          </div>
          {projects.length > 0 && (
            <div>
              <p className="mb-1 text-xs text-muted-foreground">最近打开</p>
              <div className="max-h-40 overflow-y-auto">
                {projects.map((p) => (
                  <button
                    key={p.id}
                    type="button"
                    onClick={() => setPath(p.path)}
                    className="flex w-full items-center justify-between rounded px-2 py-1 text-left text-xs hover:bg-accent"
                  >
                    <span>{p.name}</span>
                    <span className="truncate pl-3 font-mono text-[10px] text-muted-foreground">
                      {p.path}
                    </span>
                  </button>
                ))}
              </div>
            </div>
          )}
          {err && <p className="text-xs text-destructive">{err}</p>}
        </div>
        <DialogFooter>
          <Button variant="ghost" onClick={() => onOpenChange(false)}>
            取消
          </Button>
          <Button onClick={() => void submit()} disabled={submitting || !path.trim()}>
            {submitting ? "打开中…" : "打开"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
