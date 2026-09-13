// 打开目录(原型 openDirOverlay/palette):手输 + 原生浏览 + 最近打开(=注册表)。
// 面板对齐原型 .palette:13vh 顶部落下、border-strong 描边、r-lg 圆角、pop 阴影。
// 提交即 project_add(后端校验 git 仓库、归一 canonical、追加 exclude)。
import { useCallback, useEffect, useState } from "react";
import { Folder, FolderOpen } from "lucide-react";
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
      {/* 原型 .palette:顶部 13vh、r-lg(→rounded-xl)、border-strong、pop 阴影
          (top/translate 用 ! 压过 DialogContent 默认的居中定位) */}
      <DialogContent className="top-[13vh]! translate-y-0! gap-0 overflow-hidden rounded-xl border-border-strong p-0 shadow-[0_10px_24px_oklch(0.2_0_0_/_0.22)] sm:max-w-md">
        <DialogHeader className="border-b border-border px-4 py-3">
          <DialogTitle className="text-sm font-semibold">打开项目目录</DialogTitle>
        </DialogHeader>
        {/* 输入行(原型 .pinput):前置 Folder 图标 + 无边框透明输入 */}
        <div className="flex items-center gap-2 px-4 py-3">
          <Folder className="size-4 shrink-0 text-muted-foreground" />
          <Input
            value={path}
            placeholder="/path/to/repo(须为 git 仓库)"
            onChange={(e) => setPath(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") void submit();
            }}
            className="h-8 rounded-md border-0 bg-transparent px-0 text-[13px] shadow-none focus-visible:border-0 focus-visible:shadow-none focus-visible:ring-0 placeholder:text-faint"
          />
          <Button variant="secondary" size="sm" onClick={() => void browse()}>
            <FolderOpen className="size-3.5" />
            浏览…
          </Button>
        </div>
        {projects.length > 0 && (
          <div className="border-t border-border p-1.5">
            {/* 原型 .lab:11px/600/大写字距 */}
            <p className="px-2 pt-1.5 pb-1 text-[11px] font-semibold tracking-[0.05em] text-muted-foreground">
              最近打开
            </p>
            <div className="max-h-40 overflow-y-auto p-1">
              {projects.map((p) => (
                <button
                  key={p.id}
                  type="button"
                  onClick={() => setPath(p.path)}
                  className="flex w-full items-center justify-between gap-3 rounded-md px-2.5 py-2 text-left text-[12.5px] transition-colors duration-130 hover:bg-accent hover:text-foreground"
                >
                  <span className="shrink-0">{p.name}</span>
                  <span className="truncate font-mono text-[10.5px] text-faint">
                    {p.path}
                  </span>
                </button>
              ))}
            </div>
          </div>
        )}
        {err && (
          <p className="border-t border-border px-4 py-2 text-[11px] text-destructive">
            {err}
          </p>
        )}
        <DialogFooter className="gap-2 border-t border-border px-4 py-3">
          <Button variant="ghost" size="sm" onClick={() => onOpenChange(false)}>
            取消
          </Button>
          <Button
            size="sm"
            onClick={() => void submit()}
            disabled={submitting || !path.trim()}
          >
            {submitting ? "打开中…" : "打开"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
