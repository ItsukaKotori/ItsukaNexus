// WorktreeManager:命名规范 + 路径决策 + 业务编排 + worktree 变更事件。
// 事件缝与 SessionManager 的 EventSink 同型:构造注入 sink,core 不知道 tauri。
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;

use super::ops::{GitOps, WorktreeInfo};
use crate::error::NexusError;

/// worktree 变更类型(serde camelCase:Created / Removed)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum WorktreeChange {
    Created,
    Removed,
}

/// 事件负载:哪个 repo 发生了什么变更(T10 桥到 worktree://changed 事件)
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeChanged {
    pub repo_path: PathBuf,
    pub change: WorktreeChange,
}

pub type WorktreeEventSink = Arc<dyn Fn(WorktreeChanged) + Send + Sync>;

pub struct WorktreeManager {
    ops: Arc<dyn GitOps>,
    events: WorktreeEventSink,
}

impl WorktreeManager {
    pub fn new(ops: Arc<dyn GitOps>, events: WorktreeEventSink) -> Self {
        Self { ops, events }
    }

    /// 创建 nexus 命名规范的 worktree(分支 = 目录 = name):
    /// `git worktree add -b <name> <repo>/.nx-worktrees/<name> [base_ref]`,
    /// 成功后 emit Created。
    pub async fn create(
        &self,
        repo: &Path,
        provider: &str,
        base_ref: Option<&str>,
    ) -> Result<WorktreeInfo, NexusError> {
        let name = crate::ids::WorktreeName::generate(provider, now_epoch_secs());
        self.ops
            .worktree_create(repo, name.as_str(), base_ref)
            .await?;
        // canonicalize 与 list 输出(git 的真实路径)对齐,便于 UI 两侧互认;
        // 失败(Windows verbatim 前缀等场景外)保留构造路径兜底。
        let path = repo.join(".nx-worktrees").join(name.as_str());
        let path = path.canonicalize().unwrap_or(path);
        (self.events)(WorktreeChanged {
            repo_path: repo.to_path_buf(),
            change: WorktreeChange::Created,
        });
        Ok(WorktreeInfo {
            name: name.to_string(),
            branch: Some(name.to_string()),
            path,
        })
    }

    pub async fn list(&self, repo: &Path) -> Result<Vec<WorktreeInfo>, NexusError> {
        self.ops.worktree_list(repo).await
    }

    /// 按 name 移除(匹配一律按 name 字符串,不比对 path——跨平台路径形态
    /// 差异交给 git);delete_branch 连带删本地分支,末尾 prune 清残留元数据。
    /// 成功后 emit Removed。
    pub async fn remove(
        &self,
        repo: &Path,
        name: &str,
        delete_branch: bool,
    ) -> Result<(), NexusError> {
        let list = self.ops.worktree_list(repo).await?;
        let target =
            list.into_iter()
                .find(|w| w.name == name)
                .ok_or_else(|| NexusError::GitCommand {
                    cmd: "worktree remove".into(),
                    stderr: format!("worktree {name:?} 不存在"),
                })?;
        // remove/branch -D/prune 的串联是实现细节,收在 CLI 实现里(三参签名)
        self.ops
            .worktree_remove(repo, &target.path, delete_branch)
            .await?;
        (self.events)(WorktreeChanged {
            repo_path: repo.to_path_buf(),
            change: WorktreeChange::Removed,
        });
        Ok(())
    }
}

fn now_epoch_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
