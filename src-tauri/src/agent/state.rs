// 会话状态机(Rust 单一权威,spec §1.3):穷尽 match 保证新增状态时
// 编译器强制处理所有迁移点。M2 简化:Created 一闪不入表,
// WaitingInput 预留 v1.1 启发式推断(spec 明示的有意简化)。
use serde::Serialize;

use crate::ids::SessionId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SessionState {
    Running,
    /// 用户/管理端已请求停止,等待子进程退出
    Stopping,
    /// 子进程已退出(exit_code 有值)
    Exited,
    /// 非正常失败(spawn 失败、PTY 错误、wait 错误)
    Failed,
}

impl SessionState {
    /// 迁移合法性(单一权威;终态吸收一切 = false)
    pub fn can_transition_to(&self, next: SessionState) -> bool {
        use SessionState::*;
        matches!(
            (self, next),
            (Running, Stopping)
                | (Running, Exited)
                | (Running, Failed)
                | (Stopping, Exited)
                | (Stopping, Failed)
        )
    }
}

/// session://state 事件载荷(spec §1.4)
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StateChange {
    pub session_id: SessionId,
    pub prev: SessionState,
    pub next: SessionState,
    pub at_ms: u64,
    pub detail: Option<String>,
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// session_list 返回元素:会话快照(退出后保留,侧边栏显示 Exited/Failed)
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSnapshot {
    pub session_id: SessionId,
    pub state: SessionState,
    pub started_at_ms: u64,
    pub exit_code: Option<i32>,
    pub pid: Option<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legal_transitions() {
        use SessionState::*;
        assert!(Running.can_transition_to(Stopping));
        assert!(Running.can_transition_to(Failed));
        assert!(Running.can_transition_to(Exited));
        assert!(Stopping.can_transition_to(Exited));
        assert!(Stopping.can_transition_to(Failed));
    }

    #[test]
    fn terminal_states_absorb_nothing() {
        use SessionState::*;
        for from in [Exited, Failed] {
            for to in [Running, Stopping, Exited, Failed] {
                assert!(!from.can_transition_to(to), "{from:?} -> {to:?} 应被拒绝");
            }
        }
    }

    #[test]
    fn running_to_running_is_idempotent_rejected() {
        use SessionState::*;
        assert!(!Running.can_transition_to(Running));
    }

    #[test]
    fn snapshot_serializes_camel_case() {
        let snap = SessionSnapshot {
            session_id: SessionId::new(),
            state: SessionState::Running,
            started_at_ms: 1234,
            exit_code: None,
            pid: Some(42),
        };
        let json = serde_json::to_string(&snap).unwrap();
        assert!(json.contains("\"sessionId\""));
        assert!(json.contains("\"startedAtMs\""));
        assert!(json.contains("\"exitCode\""));
        assert!(!json.contains("session_id"), "键名必须 camelCase");
    }
}
