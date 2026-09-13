// 左侧图标导航(原型 rail):项目(本里程碑)/任务·市场·设置(占位禁用)。
// 折叠钮控制两侧面板(useWorkbenchLayout)。
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
        className="relative grid size-[38px] place-items-center rounded-md bg-card text-foreground"
      >
        <LayoutGrid className="size-5" />
        {/* 激活指示条(原型:左侧 3px 竖条) */}
        <span className="absolute -left-2 h-[18px] w-[3px] rounded-full bg-focus" />
      </button>
      <button
        type="button"
        title="任务(后续里程碑提供)"
        disabled
        className="grid size-[38px] cursor-not-allowed place-items-center rounded-md text-muted-foreground/50"
      >
        <ListChecks className="size-5" />
      </button>
      <button
        type="button"
        title="插件市场(后续里程碑提供)"
        disabled
        className="grid size-[38px] cursor-not-allowed place-items-center rounded-md text-muted-foreground/50"
      >
        <Blocks className="size-5" />
      </button>
      <div className="flex-1" />
      <button
        type="button"
        title="设置(后续里程碑提供)"
        disabled
        className="grid size-[38px] cursor-not-allowed place-items-center rounded-md text-muted-foreground/50"
      >
        <Settings className="size-5" />
      </button>
      <button
        type="button"
        title={sideOpen ? "收起项目栏" : "展开项目栏"}
        onClick={onToggleSide}
        className="grid size-[38px] place-items-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground"
      >
        <PanelLeft className="size-5" />
      </button>
      <button
        type="button"
        title={ctxOpen ? "收起右面板" : "展开右面板"}
        onClick={onToggleCtx}
        className="grid size-[38px] place-items-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground"
      >
        <PanelRight className="size-5" />
      </button>
    </nav>
  );
}
