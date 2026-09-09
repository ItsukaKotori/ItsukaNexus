// 领域模块声明：app = 应用元信息。命令变多后演进为 commands/ 目录。
// pub 是必须的：集成测试（tests/）作为外部 crate 需通过
// `itsukanexus_lib::app::...` 路径导入，私有模块对外不可见。
pub mod app;

pub mod agent;
pub mod error;
pub mod ids;
pub mod pty;

// Tauri 命令层：只做薄封装（把领域函数暴露给 IPC），
// 业务逻辑保持在 app.rs 纯函数里，这样才可被集成测试直接调用。
// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn app_info() -> app::AppInfo {
    app::app_info()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![app_info])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
