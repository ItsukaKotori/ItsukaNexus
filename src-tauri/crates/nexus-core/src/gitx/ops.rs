// GitOps trait(spec §1.3"一切外部能力都有 trait 缝"):v1 唯一实现 GitCliOps,
// 未来 git2 读实现也落在这里。类型与 trait 集中于本文件,worktree.rs(Task 8)
// 只做 WorktreeManager 业务,引用此处的 WorktreeInfo。
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::NexusError;

/// git --version 探测结果(worktree_supported = 版本 >= 2.20,spec 风险 #5)
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GitCheckInfo {
    pub available: bool,
    pub version: Option<String>,
    pub path: Option<String>,
    pub worktree_supported: bool,
}

/// validate_repo 结果:仓库根 / 当前分支 / 是否干净(含未跟踪文件)
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoInfo {
    pub root: PathBuf,
    pub current_branch: Option<String>,
    pub is_clean: bool,
}

/// `git worktree list` 的一项。name = 分支名(nexus 命名规范下的 worktree),
/// 外建 worktree 无分支(detached)时用目录名兜底;branch 为 None 即 detached。
/// path 为 git 输出的仓库级真实路径(已含符号链接解析)。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeInfo {
    pub name: String,
    pub path: PathBuf,
    pub branch: Option<String>,
}

#[async_trait::async_trait]
pub trait GitOps: Send + Sync {
    /// 探测系统 git:可用性 / 版本 / 可执行路径 / worktree 支持。
    /// git 缺失不是错误,返回 available=false 的结果即可(UI 据此 gate)。
    async fn check(&self) -> GitCheckInfo;
    /// 校验 path 是 git 仓库并返回 RepoInfo;不是仓库时返回 NotARepo。
    async fn validate_repo(&self, path: &Path) -> Result<RepoInfo, NexusError>;

    // ---- worktree 三方法:trait 一次定型(Task 8 在 GitCliOps 上实现)----

    /// 列出 repo 的全部 worktree(含主 worktree)。
    async fn worktree_list(&self, repo: &Path) -> Result<Vec<WorktreeInfo>, NexusError>;
    /// 从 base_ref(默认 HEAD)创建名为 name 的新 worktree。
    async fn worktree_create(
        &self,
        repo: &Path,
        name: &str,
        base_ref: Option<&str>,
    ) -> Result<(), NexusError>;
    /// 移除 path 处的 worktree;delete_branch 为真时连带删掉其分支(Task 8 三参版)。
    async fn worktree_remove(
        &self,
        repo: &Path,
        path: &Path,
        delete_branch: bool,
    ) -> Result<(), NexusError>;
}
