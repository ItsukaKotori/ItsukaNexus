// 领域模块声明：命令变多后演进为 commands/ 目录。
pub mod agent;
pub mod app;
pub mod error;
pub mod ids;
pub mod pty;

use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use agent::manager::{SessionEvent, SessionManager};
use ids::SessionId;

// ---------- app_info(M0)----------

#[tauri::command]
fn app_info() -> app::AppInfo {
    app::app_info()
}

// ---------- session_*(M1)----------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionCreated {
    session_id: SessionId,
    state: &'static str, // M1 固定 "running";M2 起换真正的 SessionState
}

#[tauri::command]
async fn session_create(
    state: State<'_, SessionManager>,
    provider_id: String,
    cols: Option<u16>,
    rows: Option<u16>,
) -> Result<SessionCreated, String> {
    let id = state
        .create(&provider_id, cols.unwrap_or(80), rows.unwrap_or(24))
        .await
        .map_err(|e| e.to_string())?;
    Ok(SessionCreated {
        session_id: id,
        state: "running",
    })
}

#[tauri::command]
async fn session_send_input(
    state: State<'_, SessionManager>,
    session_id: String,
    data: String,
) -> Result<(), String> {
    let id: SessionId = parse_id(session_id)?;
    state.send_input(id, &data).await.map_err(|e| e.to_string())
}

#[tauri::command]
fn session_resize(
    state: State<SessionManager>,
    session_id: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    let id: SessionId = parse_id(session_id)?;
    state.resize(id, cols, rows).map_err(|e| e.to_string())
}

#[tauri::command]
async fn session_stop(
    state: State<'_, SessionManager>,
    session_id: String,
    force: Option<bool>, // M2 起接通:false 走优雅关停(宽限后强杀),默认强杀
) -> Result<(), String> {
    let id: SessionId = parse_id(session_id)?;
    state
        .stop(id, force.unwrap_or(false))
        .await
        .map_err(|e| e.to_string())
}

fn parse_id(s: String) -> Result<SessionId, String> {
    s.parse::<uuid::Uuid>()
        .map(SessionId::from)
        .map_err(|e| format!("非法 session id {s:?}: {e}"))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // 事件缝的 IPC 端:把 SessionEvent 转 emit
            // (M2 重构为 EventBus 订阅;M1 一条闭包足够)
            let handle: AppHandle = app.handle().clone();
            let sink = Arc::new(move |ev: SessionEvent| {
                let event = match &ev {
                    // M2 起 Output 不再走全局 sink(per-session 订阅下发),
                    // 该臂仅为穷尽匹配保留,Task 6 随"session://output 废除"删除
                    SessionEvent::Output { .. } => "session://output",
                    SessionEvent::State(_) => "session://state",
                    SessionEvent::Exit { .. } => "session://exit",
                };
                let _ = handle.emit(event, ev);
            });
            app.manage(SessionManager::new(sink));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_info,
            session_create,
            session_send_input,
            session_resize,
            session_stop
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
