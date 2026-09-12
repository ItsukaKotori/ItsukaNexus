// IPC 命令层:每个域一个文件,generate_handler 清单是唯一注册点(spec §1.2)。
// 注意用 glob 再导出:tauri 的 generate_handler![commands::foo] 走的是
// #[tauri::command] 在定义模块生成的隐藏包装宏(__cmd__foo),`pub use foo::foo`
// 只搬函数搬不走宏;glob 能把函数与隐藏宏一并带上。
pub mod app;
pub mod config;
pub mod session;
pub mod worktree;

pub use app::*;
pub use config::*;
pub use session::*;
pub use worktree::*;
