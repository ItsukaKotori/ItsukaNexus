// 四栏 workbench(spec §1.5):rail(52) | 项目树(272,可折叠) | 标签中心 |
// 右面板(296,可折叠)。本任务侧栏/右面板为占位(Task 6/8 填充)。
import Rail from "./Rail";
import { useWorkbenchLayout } from "./useWorkbenchLayout";
import TabBody from "../features/tabs/TabBody";
import TabStrip from "../features/tabs/TabStrip";

interface Props {
  onCloseTab: (sessionId: string) => void;
  onFitted: (id: string, cols: number, rows: number) => void;
}

export default function Workbench({ onCloseTab, onFitted }: Props) {
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
          <div className="flex min-h-[37px] items-center border-b border-border px-3 text-xs font-semibold tracking-wide text-muted-foreground">
            项目
          </div>
          <div className="flex flex-1 items-center justify-center p-4 text-center text-xs text-muted-foreground">
            项目树(下一步提供)
          </div>
        </aside>
      )}
      <main className="flex min-w-0 flex-1 flex-col">
        <TabStrip onClose={onCloseTab} />
        <TabBody onFitted={onFitted} />
      </main>
      {ctxOpen && (
        <aside className="flex w-[296px] min-w-0 flex-col border-l border-border">
          <div className="flex min-h-[37px] items-center border-b border-border px-3 text-xs font-semibold tracking-wide text-muted-foreground">
            Git
          </div>
          <div className="flex flex-1 items-center justify-center p-4 text-center text-xs text-muted-foreground">
            Git 面板(后续任务提供)
          </div>
        </aside>
      )}
    </div>
  );
}
