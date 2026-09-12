// session_*(M2):创建/订阅/输入/resize/关停/快照列表。
use serde::Serialize;
use tauri::State;

use nexus_core::agent::manager::{SessionManager, Subscription};
use nexus_core::agent::state::{SessionSnapshot, SessionState};
use nexus_core::ids::SessionId;

/// IPC 输出流载荷,只定义在这一层(裁定 M2-P2:nexus-core 不依赖 tauri,
/// core 侧用 OutputFrame,IPC 层映射自己的 wire 类型)。
/// sessionId 冗余于订阅会话本身,但保留以钉住 spec §1.4 的前端契约。
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PtyChunk {
    session_id: SessionId,
    data: String,
    seq: u64,
}

/// session_attach 确认:replayedBytes = 订阅时刻的历史回放字节数。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachAck {
    replayed_bytes: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionCreated {
    session_id: SessionId,
    state: SessionState,
}

/// 创建会话。repo_path + worktree_name 同时给出时,shell 的 cwd 落在
/// `<repo>/.nx-worktrees/<worktreeName>`(目录必须已存在,由 worktree_create
/// 先建);两者都缺省 = 纯 shell 会话(cwd 继承本进程,快照两字段 None)。
/// canonicalize 一并归一 macOS /var ↔ /private/var 形态;Windows verbatim
/// 前缀(`\\?\`)对 portable-pty 的 cwd 是安全的(内部自行处理),不剥。
#[tauri::command]
pub async fn session_create(
    state: State<'_, SessionManager>,
    provider_id: String,
    cols: Option<u16>,
    rows: Option<u16>,
    repo_path: Option<String>,
    worktree_name: Option<String>,
) -> Result<SessionCreated, String> {
    let launch = match (&repo_path, &worktree_name) {
        (Some(repo), Some(name)) => {
            let wt_dir = std::path::PathBuf::from(repo)
                .join(".nx-worktrees")
                .join(name);
            let canonical = wt_dir.canonicalize().map_err(|e| {
                nexus_core::NexusError::InvalidInput(format!(
                    "worktree 不存在 {}: {e}",
                    wt_dir.display()
                ))
                .to_string()
            })?;
            Some(nexus_core::agent::manager::LaunchSpec {
                cwd: canonical,
                repo_path: Some(repo.clone()),
                worktree_name: Some(name.clone()),
            })
        }
        (None, Some(_)) => {
            return Err(nexus_core::NexusError::InvalidInput(
                "指定 worktreeName 时必须同时指定 repoPath".into(),
            )
            .to_string());
        }
        // (Some, None) 与 (None, None):repo_path 单独给出没有落点语义,
        // 与全缺省同型(M2 行为)
        _ => None,
    };
    let id = state
        .create(
            &provider_id,
            cols.unwrap_or(80),
            rows.unwrap_or(24),
            launch.as_ref(),
        )
        .await
        .map_err(|e| e.to_string())?;
    Ok(SessionCreated {
        session_id: id,
        state: SessionState::Running,
    })
}

/// 订阅会话输出:历史回放先经 Channel 发一条 `seq: 0` 的 PtyChunk(仅非空),
/// 随后 spawn 转发任务承接订阅流,逐帧下发给前端。重复 attach 会替换旧订阅,
/// 旧转发任务随旧接收端关闭自行结束。
///
/// 前端契约(T8 依赖):
/// - 接缝原子化(I-1):replay 尾帧与实时流的交接、重复 attach 的替换都是
///   互斥临界段,replay 与实时流不重复不丢失;
/// - 会话退出后订阅发送端被置空、转发任务随之结束;终态下 attach 返回
///   replay + 已关闭流。流结束仍以 `session://exit` 事件为权威信号。
#[tauri::command]
pub async fn session_attach(
    state: State<'_, SessionManager>,
    session_id: String,
    output: tauri::ipc::Channel<PtyChunk>,
) -> Result<AttachAck, String> {
    let id = parse_id(session_id)?;
    let Subscription { replay, rx } = state.subscribe(id).map_err(|e| e.to_string())?;
    let replayed = replay.len() as u64;
    if !replay.is_empty() {
        // seq=0 为 replay 专属:实时帧从 1 起单调,消费端可据此区分
        let _ = output.send(PtyChunk {
            session_id: id,
            data: replay,
            seq: 0,
        });
    }
    tokio::spawn(async move {
        let mut rx = rx;
        while let Some(frame) = rx.recv().await {
            if output
                .send(PtyChunk {
                    session_id: id,
                    data: frame.data,
                    seq: frame.seq,
                })
                .is_err()
            {
                break; // 前端 Channel 失效(webview 重载):转发任务自行收尾
            }
        }
        // recv() 返回 None = 订阅被替换或会话终态断订阅;
        // 流结束以 Exit 事件为准(见命令 doc 注释)
    });
    Ok(AttachAck {
        replayed_bytes: replayed,
    })
}

#[tauri::command]
pub async fn session_send_input(
    state: State<'_, SessionManager>,
    session_id: String,
    data: String,
) -> Result<(), String> {
    let id: SessionId = parse_id(session_id)?;
    state.send_input(id, &data).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn session_resize(
    state: State<'_, SessionManager>,
    session_id: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    let id: SessionId = parse_id(session_id)?;
    state.resize(id, cols, rows).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn session_stop(
    state: State<'_, SessionManager>,
    session_id: String,
    force: Option<bool>, // M2 起接通:缺省 false = 优雅关停(先 \x03、宽限,超时再强杀)
) -> Result<(), String> {
    let id: SessionId = parse_id(session_id)?;
    state
        .stop(id, force.unwrap_or(false))
        .await
        .map_err(|e| e.to_string())
}

/// 会话回收(M3 "关 tab 即删"):仅终态(Exited/Failed)可删,运行中报
/// "会话未在运行",不存在报"会话不存在";条目删除后 replay/订阅随之释放。
#[tauri::command]
pub async fn session_dispose(
    state: State<'_, SessionManager>,
    session_id: String,
) -> Result<(), String> {
    let id = parse_id(session_id)?;
    state.dispose(id).map_err(|e| e.to_string())
}

/// 会话快照列表:含已退出的会话(侧边栏显示 Exited/Failed)。
#[tauri::command]
pub async fn session_list(
    state: State<'_, SessionManager>,
) -> Result<Vec<SessionSnapshot>, String> {
    Ok(state.list())
}

pub fn parse_id(s: String) -> Result<SessionId, String> {
    s.parse::<SessionId>()
}
