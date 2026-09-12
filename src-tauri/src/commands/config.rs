// config_*(M2):AppConfig 读写。store 层保持无 tauri 依赖(spec §1.3)。
use std::path::PathBuf;

use tauri::State;

use nexus_core::config;
use nexus_core::config::model::AppConfig;

/// 配置目录的 State 注入体:setup 里从 app_config_dir() 解析一次,
/// 命令侧只拿 PathBuf——store 层保持无 tauri 依赖(spec §1.3)。
/// 元组字段 pub 供 lib.rs 的 setup 构造注入。
pub struct ConfigDir(pub PathBuf);

#[tauri::command]
pub fn config_get(dir: State<'_, ConfigDir>) -> Result<AppConfig, String> {
    Ok(config::store::load_or_create(&dir.0))
}

/// 保存后重新 load 返回落盘值(以磁盘为准,而非调用方入参)。
#[tauri::command]
pub fn config_save(dir: State<'_, ConfigDir>, config: AppConfig) -> Result<AppConfig, String> {
    config::store::save(&dir.0, &config).map_err(|e| e.to_string())?;
    Ok(config::store::load_or_create(&dir.0))
}
