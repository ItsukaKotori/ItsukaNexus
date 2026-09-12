// git/worktree 命令层:薄封装,领域逻辑都在 nexus-core 的 gitx 域。
// worktree 三命令(list/create/remove)入口以 WorktreeManager 持有的 check
// 结果 gate(T7/T8 审查 carry):无 git / 版本过低不进 ops,直接给可读错误;
// git_check / git_validate_repo 不 gate——check 本身就是探测,validate 对
// 无 git 的机器自然报 GitUnavailable。worktree://changed 事件的 IPC 端在
// lib.rs(setup 注入 sink → AppHandle::emit)。
use tauri::State;

use nexus_core::gitx::ops::{GitCheckInfo, RepoInfo, WorktreeInfo};
use nexus_core::gitx::worktree::WorktreeManager;

/// worktree 功能的统一入口 gate:探测结果不满足即拒绝,错误信息直接进 UI。
async fn ensure_worktree_ready(mgr: &WorktreeManager) -> Result<(), String> {
    let info = mgr.check().await;
    if !info.available {
        return Err(
            "未检测到 git,无法使用 worktree 功能。请安装 git 并确认其在 PATH 中后重试".into(),
        );
    }
    if !info.worktree_supported {
        let ver = info.version.as_deref().unwrap_or("未知版本");
        return Err(format!(
            "git 版本过低(当前 {ver}),worktree 功能需要 git ≥ 2.20,请升级 git 后重试"
        ));
    }
    Ok(())
}

/// 探测系统 git:可用性 / 版本 / 路径 / worktree 支持(git 缺失不是错误,
/// 返回 available=false 供 UI gate)。
#[tauri::command]
pub async fn git_check(state: State<'_, WorktreeManager>) -> Result<GitCheckInfo, String> {
    Ok(state.check().await)
}

/// 校验给定路径是 git 仓库并返回根目录 / 当前分支 / 是否干净。
#[tauri::command]
pub async fn git_validate_repo(
    state: State<'_, WorktreeManager>,
    repo_path: String,
) -> Result<RepoInfo, String> {
    state
        .validate_repo(std::path::Path::new(&repo_path))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn worktree_list(
    state: State<'_, WorktreeManager>,
    repo_path: String,
) -> Result<Vec<WorktreeInfo>, String> {
    ensure_worktree_ready(&state).await?;
    state
        .list(std::path::Path::new(&repo_path))
        .await
        .map_err(|e| e.to_string())
}

/// 按 nexus 命名规范创建 worktree(分支 = 目录名),成功后前端经
/// `worktree://changed` 收到 Created。
#[tauri::command]
pub async fn worktree_create(
    state: State<'_, WorktreeManager>,
    repo_path: String,
    provider: Option<String>,
    base_ref: Option<String>,
) -> Result<WorktreeInfo, String> {
    ensure_worktree_ready(&state).await?;
    state
        .create(
            std::path::Path::new(&repo_path),
            provider.as_deref().unwrap_or("shell"),
            base_ref.as_deref(),
        )
        .await
        .map_err(|e| e.to_string())
}

/// 按 name 移除 worktree;delete_branch 连带删本地分支,成功后前端经
/// `worktree://changed` 收到 Removed。
#[tauri::command]
pub async fn worktree_remove(
    state: State<'_, WorktreeManager>,
    repo_path: String,
    name: String,
    delete_branch: bool,
) -> Result<(), String> {
    ensure_worktree_ready(&state).await?;
    state
        .remove(std::path::Path::new(&repo_path), &name, delete_branch)
        .await
        .map_err(|e| e.to_string())
}
