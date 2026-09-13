// 左栏(spec §1.5):标题行 + git 探测条 + 项目树。打开目录走 OpenDirOverlay
// (失败经全局 toast 报告,行内红字留在弹层内)。
// 标题行对齐原型 .side-head:11px/600/字距 .05em muted + 26×26 圆角悬停「+」。
import { useState } from "react";
import { Plus } from "lucide-react";

import type { GitCheckInfo } from "../../ipc/types";
import ProjectTree from "./ProjectTree";
import OpenDirOverlay from "./OpenDirOverlay";

export default function ProjectSide({ gitInfo }: { gitInfo: GitCheckInfo | null }) {
  const [openDir, setOpenDir] = useState(false);

  return (
    <aside className="flex min-w-0 flex-1 flex-col">
      <div className="flex min-h-[37px] items-center justify-between border-b border-border pr-2.5 pl-3">
        <span className="text-[11px] font-semibold tracking-[0.05em] text-muted-foreground">
          项目
        </span>
        <button
          type="button"
          title="打开项目目录"
          onClick={() => setOpenDir(true)}
          className="grid size-[26px] place-items-center rounded-md text-muted-foreground transition-colors duration-130 hover:bg-accent hover:text-foreground"
        >
          <Plus className="size-4" />
        </button>
      </div>
      {gitInfo && !gitInfo.worktreeSupported && (
        <div className="border-b border-border bg-status-warn/10 px-3 py-1.5 text-[11px] text-status-warn">
          {gitInfo.available
            ? `git ${gitInfo.version ?? ""} 版本过低——worktree 功能需要 git ≥ 2.20`
            : "未检测到 git——请安装 git ≥ 2.20"}
        </div>
      )}
      <ProjectTree />
      <OpenDirOverlay openState={openDir} onOpenChange={setOpenDir} />
    </aside>
  );
}
