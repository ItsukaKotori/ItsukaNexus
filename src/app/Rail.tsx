// 左侧图标导航(原型 rail):项目(本里程碑)/任务·市场·设置(占位禁用)。
// 折叠钮控制两侧面板(useWorkbenchLayout)。
// 样式对齐原型 .rail/.rail-btn:38×38、r-md 圆角、激活 = surface 底 + 左侧
// 3px×18px accent 竖条(left:-8px);禁用钮 currentColor 60% 不可点但可见。
import {
  Blocks,
  LayoutGrid,
  ListChecks,
  PanelLeft,
  PanelRight,
  Settings,
} from "lucide-react";

interface Props {
  sideOpen: boolean;
  ctxOpen: boolean;
  onToggleSide: () => void;
  onToggleCtx: () => void;
}

const BTN =
  "grid size-[38px] place-items-center rounded-lg transition-colors duration-130";

export default function Rail({
  sideOpen,
  ctxOpen,
  onToggleSide,
  onToggleCtx,
}: Props) {
  return (
    <nav
      aria-label="主导航"
      className="flex w-[52px] flex-col items-center gap-1 border-r border-border py-2"
    >
      <button
        type="button"
        title="项目"
        className={`${BTN} relative bg-card text-foreground`}
      >
        <LayoutGrid className="size-5" />
        {/* 激活指示条(原型 .rail-btn.active::before:left:-8px、3×18px) */}
        <span className="absolute top-1/2 -left-2 h-[18px] w-[3px] -translate-y-1/2 rounded-full bg-focus" />
      </button>
      <button
        type="button"
        title="任务(后续里程碑提供)"
        disabled
        className={`${BTN} cursor-not-allowed text-foreground/60`}
      >
        <ListChecks className="size-5" />
      </button>
      <button
        type="button"
        title="插件市场(后续里程碑提供)"
        disabled
        className={`${BTN} cursor-not-allowed text-foreground/60`}
      >
        <Blocks className="size-5" />
      </button>
      {/* spacer:禁用组与底部区之间(原型 .rail.spacer) */}
      <div className="flex-1" />
      <button
        type="button"
        title="设置(后续里程碑提供)"
        disabled
        className={`${BTN} cursor-not-allowed text-foreground/60`}
      >
        <Settings className="size-5" />
      </button>
      <button
        type="button"
        title={sideOpen ? "收起项目栏" : "展开项目栏"}
        onClick={onToggleSide}
        className={`${BTN} text-muted-foreground hover:bg-accent hover:text-foreground`}
      >
        <PanelLeft className="size-5" />
      </button>
      <button
        type="button"
        title={ctxOpen ? "收起右面板" : "展开右面板"}
        onClick={onToggleCtx}
        className={`${BTN} text-muted-foreground hover:bg-accent hover:text-foreground`}
      >
        <PanelRight className="size-5" />
      </button>
    </nav>
  );
}
