// 四栏 workbench(spec §1.5):rail(52) | 项目树(272,可折叠) | 标签中心 |
// 右面板(296,可折叠,Git 面板——「文件」分段为占位)。
import type { GitCheckInfo } from "../ipc/types";
import Rail from "./Rail";
import { useWorkbenchLayout } from "./useWorkbenchLayout";
import GitPanel from "../features/gitpanel/GitPanel";
import ProjectSide from "../features/project/ProjectSide";
import TabBody from "../features/tabs/TabBody";
import TabStrip from "../features/tabs/TabStrip";

interface Props {
  gitInfo: GitCheckInfo | null;
  onCloseTab: (sessionId: string) => void;
  onFitted: (id: string, cols: number, rows: number) => void;
}

export default function Workbench({ gitInfo, onCloseTab, onFitted }: Props) {
  const { sideOpen, ctxOpen, toggleSide, toggleCtx } = useWorkbenchLayout();
  return (
    <div className="flex h-screen min-w-0">
      <Rail
        sideOpen={sideOpen}
        ctxOpen={ctxOpen}
        onToggleSide={toggleSide}
        onToggleCtx={toggleCtx}
      />
      {/* 折叠即条件卸载 aside——不触及 <main>,终端闩锁不受影响 */}
      {sideOpen && (
        <aside className="flex w-[272px] min-w-0 flex-col border-r border-border">
          <ProjectSide gitInfo={gitInfo} />
        </aside>
      )}
      <main className="flex min-w-0 flex-1 flex-col">
        <TabStrip onClose={onCloseTab} />
        <TabBody onFitted={onFitted} />
      </main>
      {ctxOpen && (
        <aside className="flex w-[296px] min-w-0 flex-col border-l border-border">
          <div className="flex min-h-[37px] items-center gap-1 border-b border-border px-2">
            <span className="rounded bg-card px-2 py-1 text-xs text-foreground">Git</span>
            <span
              title="文件(后续里程碑提供)"
              className="cursor-not-allowed rounded px-2 py-1 text-xs text-muted-foreground/50"
            >
              文件
            </span>
          </div>
          <GitPanel />
        </aside>
      )}
    </div>
  );
}
