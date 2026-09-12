// IPC 层:tauri Builder 组装 + State 注入。命令在 commands/ 目录(spec §1.2)。
pub mod app; // M0 的 app_info 领域函数留这层(展示命令,无领域逻辑)
mod commands;

use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager};

use commands::ConfigDir;
use nexus_core::agent::manager::{SessionEvent, SessionManager};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .build(),
        )
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // 事件缝的 IPC 端:SessionEvent(State/Exit)→ emit,Output 走 Channel 不经这
            let handle: AppHandle = app.handle().clone();
            let sink = Arc::new(move |ev: SessionEvent| {
                let event = match &ev {
                    SessionEvent::Output { .. } => return,
                    SessionEvent::State(_) => "session://state",
                    SessionEvent::Exit { .. } => "session://exit",
                };
                let _ = handle.emit(event, ev);
            });
            app.manage(SessionManager::new(sink));
            app.manage(ConfigDir(
                app.path().app_config_dir().expect("解析应用配置目录失败"),
            ));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::session_create,
            commands::session_attach,
            commands::session_send_input,
            commands::session_resize,
            commands::session_stop,
            commands::session_list,
            commands::config_get,
            commands::config_save
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
