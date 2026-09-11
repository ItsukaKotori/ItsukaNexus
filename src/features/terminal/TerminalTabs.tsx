// 会话 tab 栏:序号标题 + 状态徽章(着色)+ 关闭按钮。
// 徽章配色:running 绿 / stopping 黄 / exited 灰 / failed 红。
import type { SessionSnapshot, SessionState } from "../../ipc/types";

const STATE_META: Record<SessionState, { label: string; color: string }> = {
  running: { label: "Running", color: "#16a34a" },
  stopping: { label: "Stopping", color: "#d97706" },
  exited: { label: "Exited", color: "#6b7280" },
  failed: { label: "Failed", color: "#dc2626" },
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
    <nav
      style={{
        display: "flex",
        alignItems: "center",
        gap: 4,
        padding: "4px 8px",
        borderBottom: "1px solid #ddd",
        overflowX: "auto",
        minHeight: 30,
      }}
    >
      {Object.keys(sessions).map((id, index) => {
        const snap = sessions[id];
        const meta = STATE_META[snap.state];
        const active = id === activeId;
        const live = snap.state === "running" || snap.state === "stopping";
        return (
          <div
            key={id}
            onClick={() => onSelect(id)}
            title={id}
            style={{
              display: "flex",
              alignItems: "center",
              gap: 6,
              padding: "3px 4px 3px 10px",
              borderRadius: 6,
              fontSize: 13,
              whiteSpace: "nowrap",
              cursor: "pointer",
              userSelect: "none",
              background: active ? "#e5e7eb" : "transparent",
              border: `1px solid ${active ? "#9ca3af" : "transparent"}`,
            }}
          >
            <span style={{ fontWeight: 600 }}>#{index + 1}</span>
            <span style={{ display: "inline-flex", alignItems: "center", gap: 4, color: meta.color }}>
              <span
                style={{
                  width: 8,
                  height: 8,
                  borderRadius: "50%",
                  background: meta.color,
                  display: "inline-block",
                }}
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
              style={{
                border: "none",
                background: "transparent",
                color: "#6b7280",
                cursor: "pointer",
                fontSize: 14,
                lineHeight: 1,
                padding: "2px 6px",
                borderRadius: 4,
              }}
            >
              ×
            </button>
          </div>
        );
      })}
    </nav>
  );
}
