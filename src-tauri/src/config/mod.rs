// config 子系统(M2 Task 7):serde 建模 + 磁盘读写。
// store 只吃 `&Path`,配置目录由 IPC 层从 tauri 的 app_config_dir() 注入,
// 保 nexus-core 无 tauri 的缝(spec §1.3)。M4 起 AgentProfile 驱动 spawn。
pub mod model;
pub mod store;

pub use model::AppConfig;

/// 默认配置:version=1、终端 13pt/5000 行回滚、内置一条 shell profile
/// (command 取 manager::default_shell 的当前值)。
pub fn default_config() -> AppConfig {
    AppConfig::default()
}
