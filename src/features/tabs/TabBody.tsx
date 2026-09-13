// tab 内容区:全部已打开 tab 的 pane 恒久挂载、按 active 切换可见(挂载闩锁,
// spec §1.5 红线——切 tab 不销毁 xterm)。M4 仅 terminal kind;后续 kind 在
// switch 处扩展,未知 kind 显示占位。
import TerminalPane from "../terminal/TerminalPane";
import { useTabs } from "../../stores/tabStore";

interface Props {
  onFitted: (id: string, cols: number, rows: number) => void;
}

export default function TabBody({ onFitted }: Props) {
  const tabs = useTabs((s) => s.tabs);
  const activeTabId = useTabs((s) => s.activeTabId);

  return (
    <div className="relative min-h-0 min-w-0 flex-1">
      {tabs.map((tab) => (
        <div
          key={tab.id}
          className={tab.id === activeTabId ? "h-full w-full" : "hidden"}
        >
          <TerminalPane sessionId={tab.sessionId} onFitted={onFitted} />
        </div>
      ))}
      {tabs.length === 0 && (
        <div className="flex h-full items-center justify-center text-[12.5px] text-muted-foreground">
          暂无标签——从左侧项目树或标签栏「+」开始
        </div>
      )}
    </div>
  );
}
