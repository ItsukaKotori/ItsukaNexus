// 展示命令(M0):app_info 的 IPC 薄包装,领域函数留在这层的 app.rs。
use crate::app::AppInfo;

#[tauri::command]
pub fn app_info() -> AppInfo {
    crate::app::app_info()
}
