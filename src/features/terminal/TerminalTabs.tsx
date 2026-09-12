// 会话 tab 栏:序号标题 + 状态徽章(着色)+ 关闭按钮。
// 徽章配色:running 绿 / stopping 黄 / exited 灰 / failed 红(Tailwind 类)。
import type { SessionSnapshot, SessionState } from "../../ipc/types";

const STATE_META: Record<
  SessionState,
  { label: string; text: string; dot: string }
> = {
  running: { label: "Running", text: "text-green-500", dot: "bg-green-500" },
  stopping: {
    label: "Stopping",
    text: "text-amber-500",
    dot: "bg-amber-500",
  },
  exited: { label: "Exited", text: "text-gray-400", dot: "bg-gray-400" },
  failed: { label: "Failed", text: "text-red-500", dot: "bg-red-500" },
};

interface Props {
  sessions: Record<string, SessionSnapshot>;
  activeId: string | null;
  onSelect: (id: string) => void;
  onClose: (id: string) => void;
}

export default function TerminalTabs({
  sessions,
  activeId,
  onSelect,
  onClose,
}: Props) {
  return (
    <nav className="flex min-h-[30px] items-center gap-1 overflow-x-auto border-b border-border px-2 py-1">
      {Object.keys(sessions).map((id, index) => {
        const snap = sessions[id];
        const meta = STATE_META[snap.state];
        const active = id === activeId;
        const live = snap.state === "running" || snap.state === "stopping";
        // worktree 完整名是 nexus/<provider>-<时间戳>-<rand>,tab 上只留最后段
        const wtShort = snap.worktreeName
          ? snap.worktreeName.split("/").pop()
          : null;
        return (
          <div
            key={id}
            onClick={() => onSelect(id)}
            title={id}
            className={`flex cursor-pointer items-center gap-1.5 rounded-md border py-[3px] pr-1 pl-2.5 text-[13px] whitespace-nowrap select-none ${
              active ? "border-border bg-secondary" : "border-transparent"
            }`}
          >
            <span className="font-semibold">#{index + 1}</span>
            {wtShort && (
              <span
                className="text-muted-foreground"
                title={snap.worktreeName ?? undefined}
              >
                {wtShort.length > 16 ? `${wtShort.slice(0, 15)}…` : wtShort}
              </span>
            )}
            <span
              className={`inline-flex items-center gap-1 ${meta.text}`}
            >
              <span
                className={`inline-block size-2 rounded-full ${meta.dot}`}
              />
              {meta.label}
            </span>
            <button
              type="button"
              aria-label={live ? "停止并关闭会话" : "关闭会话 tab"}
              title={live ? "优雅停止后关闭" : "关闭 tab"}
              onClick={(e) => {
                e.stopPropagation();
                onClose(id);
              }}
              className="cursor-pointer rounded border-none bg-transparent px-1.5 py-0.5 text-sm leading-none text-muted-foreground hover:bg-accent hover:text-foreground"
            >
              ×
            </button>
          </div>
        );
      })}
    </nav>
  );
}
