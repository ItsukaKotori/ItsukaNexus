// project_*:项目注册表命令(薄封装,领域逻辑在 nexus-core registry 域)。
// 入册校验(D 自身或其 git 子目录)在 ProjectRegistry::add 内完成。
use tauri::State;

use nexus_core::ids::ProjectId;
use nexus_core::registry::{ProjectEntry, ProjectRegistry};

#[tauri::command]
pub async fn project_list(state: State<'_, ProjectRegistry>) -> Result<Vec<ProjectEntry>, String> {
    let list = state.list();
    Ok(list)
}

#[tauri::command]
pub async fn project_add(
    state: State<'_, ProjectRegistry>,
    path: String,
) -> Result<Vec<ProjectEntry>, String> {
    state.add(&path).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn project_remove(
    state: State<'_, ProjectRegistry>,
    project_id: String,
) -> Result<(), String> {
    let id = project_id
        .parse::<ProjectId>()
        .map_err(|e| nexus_core::NexusError::InvalidInput(e).to_string())?;
    if state.remove(id) {
        Ok(())
    } else {
        Err(nexus_core::NexusError::InvalidInput(format!("项目不存在: {project_id}")).to_string())
    }
}
