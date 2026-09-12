// IPC 层:tauri Builder 组装 + State 注入。命令在 commands/ 目录(spec §1.2)。
pub mod app; // M0 的 app_info 领域函数留这层(展示命令,无领域逻辑)
mod commands;

use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager};

use commands::ConfigDir;
use nexus_core::agent::manager::{SessionEvent, SessionManager};
use nexus_core::gitx::cli::GitCliOps;
use nexus_core::gitx::ops::GitOps;
use nexus_core::gitx::worktree::{WorktreeChanged, WorktreeManager};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .build(),
        )
        .plugin(tauri_plugin_opener::init())
        // 前端目录选择器的后端:Rust 侧只注册,API 由 JS 包调用(Task 10 接)
        .plugin(tauri_plugin_dialog::init())
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
            // worktree 事件缝的 IPC 端:WorktreeChanged → emit
            // (载荷 = WorktreeChanged,camelCase;前端按 repoPath + change 刷新)
            let wt_handle: AppHandle = app.handle().clone();
            let wt_events = Arc::new(move |ev: WorktreeChanged| {
                let _ = wt_handle.emit("worktree://changed", ev);
            });
            let ops: Arc<dyn GitOps> = Arc::new(GitCliOps::new());
            app.manage(WorktreeManager::new(ops, wt_events));
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
            commands::session_dispose,
            commands::session_list,
            commands::config_get,
            commands::config_save,
            commands::git_check,
            commands::git_validate_repo,
            commands::worktree_list,
            commands::worktree_create,
            commands::worktree_remove
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
