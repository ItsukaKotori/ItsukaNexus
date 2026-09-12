// config_*(M2):AppConfig 读写。store 层保持无 tauri 依赖(spec §1.3)。
// M3 起 async 化:磁盘 IO 走 spawn_blocking,不占 tauri 执行器(必办#4)。
use std::path::PathBuf;

use tauri::State;

use nexus_core::config::model::AppConfig;

/// 配置目录的 State 注入体:setup 里从 app_config_dir() 解析一次,
/// 命令侧只拿 PathBuf——store 层保持无 tauri 依赖(spec §1.3)。
/// 元组字段 pub 供 lib.rs 的 setup 构造注入。
pub struct ConfigDir(pub PathBuf);

/// 读配置:load_or_create 的 bool(是否落盘了默认)对 IPC 调用方无意义,取 `.0`。
#[tauri::command]
pub async fn config_get(dir: State<'_, ConfigDir>) -> Result<AppConfig, String> {
    let d = dir.0.clone();
    tokio::task::spawn_blocking(move || nexus_core::config::store::load_or_create(&d).0)
        .await
        .map_err(|e| e.to_string())
}

/// 保存后重新 load 返回落盘值(以磁盘为准,而非调用方入参)。
/// 闭包两层 Result:内层 `NexusError`(IO 失败 → Err 字符串),
/// 外层 JoinError(任务 panic/取消 → 同样 Err 字符串)。
#[tauri::command]
pub async fn config_save(
    dir: State<'_, ConfigDir>,
    config: AppConfig,
) -> Result<AppConfig, String> {
    let d = dir.0.clone();
    tokio::task::spawn_blocking(move || -> Result<AppConfig, nexus_core::NexusError> {
        nexus_core::config::store::save(&d, &config)?;
        Ok(nexus_core::config::store::load_or_create(&d).0)
    })
    .await
    .map_err(|e| e.to_string())? // JoinError
    .map_err(|e| e.to_string()) // NexusError
}
