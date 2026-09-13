// 多类型标签条(原型 tabstrip):状态点 + 标题 + kind 徽标 + 关闭钮。
// 数据:tabStore(视图)+ sessionsStore(状态点);关闭回调走 App 的 M3
// dispose 流程(handleClose)。
import { useSessions } from "../../stores/sessionsStore";
import { useTabs } from "../../stores/tabStore";
import type { SessionState } from "../../ipc/types";

const DOT: Record<SessionState, string> = {
  running: "bg-status-run",
  stopping: "bg-status-warn",
  exited: "bg-gray-400",
  failed: "bg-status-err",
};

interface Props {
  onClose: (sessionId: string) => void;
}

export default function TabStrip({ onClose }: Props) {
  const tabs = useTabs((s) => s.tabs);
  const activeTabId = useTabs((s) => s.activeTabId);
  const setActive = useTabs((s) => s.setActive);
  const sessions = useSessions((s) => s.sessions);

  return (
    <div className="flex min-h-[37px] items-stretch border-b border-border">
      <div className="flex min-w-0 items-stretch overflow-x-auto [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
        {tabs.map((tab) => {
          const state = sessions[tab.sessionId]?.state;
          const active = tab.id === activeTabId;
          return (
            <button
              key={tab.id}
              type="button"
              onClick={() => setActive(tab.id)}
              title={tab.repoPath ?? tab.sessionId}
              className={`flex max-w-[232px] items-center gap-1.5 border-r border-border px-2.5 text-xs whitespace-nowrap ${
                active
                  ? "border-t-2 border-t-focus bg-card text-foreground"
                  : "text-muted-foreground hover:bg-accent hover:text-foreground"
              }`}
            >
              {state && (
                <span className={`size-1.5 shrink-0 rounded-full ${DOT[state]}`} />
              )}
              <span className="truncate">{tab.label}</span>
              <span className="font-mono text-[10px] text-muted-foreground/70">
                终端
              </span>
              <span
                role="button"
                aria-label="关闭标签页"
                title="关闭标签页"
                onClick={(e) => {
                  e.stopPropagation();
                  onClose(tab.sessionId);
                }}
                className="grid size-4 place-items-center rounded text-muted-foreground/60 hover:bg-secondary hover:text-foreground"
              >
                ×
              </span>
            </button>
          );
        })}
      </div>
    </div>
  );
}
