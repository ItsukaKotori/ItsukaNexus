// GitCliOps:tokio::process 调 git CLI。一律 `git -C <path>`,不依赖进程 cwd。
use std::path::{Path, PathBuf};

use super::ops::{GitCheckInfo, GitOps, RepoInfo, WorktreeInfo};
use crate::error::NexusError;

pub struct GitCliOps {
    git_bin: String,
}

impl GitCliOps {
    pub fn new() -> Self {
        Self {
            git_bin: "git".into(),
        }
    }

    /// 测试注入:用不存在的二进制名模拟"系统无 git"(不碰全局 PATH——
    /// 进程级全局状态在并行测试里是竞态源头)
    pub fn with_bin(bin: &str) -> Self {
        Self {
            git_bin: bin.to_string(),
        }
    }

    /// 执行 git 并返回 stdout;非零退出 → GitCommand(stderr 透传),
    /// 起不动进程(二进制不存在等)→ GitUnavailable。
    async fn run(&self, repo: Option<&Path>, args: &[&str]) -> Result<String, NexusError> {
        let mut cmd = tokio::process::Command::new(&self.git_bin);
        if let Some(r) = repo {
            cmd.arg("-C").arg(r);
        }
        cmd.args(args);
        let cmd_repr = format!("{:?}", args);
        let out = cmd
            .output()
            .await
            .map_err(|e| NexusError::GitUnavailable(format!("无法执行 git({e})")))?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
            return Err(NexusError::GitCommand {
                cmd: cmd_repr,
                stderr,
            });
        }
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    }
}

impl Default for GitCliOps {
    fn default() -> Self {
        Self::new()
    }
}

/// "git version 2.39.5 (Apple Git-101)" → "2.39.5"
fn parse_version(s: &str) -> Option<String> {
    let mut it = s.split_whitespace();
    while let Some(w) = it.next() {
        if w == "version" {
            let v = it.next()?;
            let dots = v.split('.').count();
            return if dots >= 2 { Some(v.to_string()) } else { None };
        }
    }
    None
}

/// 主次版本 >= 2.20(worktree porcelain 稳定期,spec 风险 #5)
fn supports_worktree(version: &str) -> bool {
    let mut it = version.split('.');
    let major: u32 = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let minor: u32 = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    (major, minor) >= (2, 20)
}

/// 找 git 的可执行路径;找不到/拿不到输出都不致命,返回 None(check 的 path 字段可空)。
/// unix 走 `sh -c 'command -v git'`,windows 走 `where git`。
fn which_git() -> Option<String> {
    #[cfg(windows)]
    let out = std::process::Command::new("where")
        .arg("git")
        .output()
        .ok()?;
    #[cfg(not(windows))]
    let out = std::process::Command::new("sh")
        .args(["-c", "command -v git"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
}

#[async_trait::async_trait]
impl GitOps for GitCliOps {
    async fn check(&self) -> GitCheckInfo {
        // 不走 run():git 缺失是"探测结果"而非错误
        match self.run(None, &["--version"]).await {
            Ok(out) => {
                let version = parse_version(&out);
                let worktree_supported = version.as_deref().map(supports_worktree).unwrap_or(false);
                GitCheckInfo {
                    available: true,
                    worktree_supported,
                    version,
                    path: which_git(),
                }
            }
            Err(_) => GitCheckInfo::default(),
        }
    }

    async fn validate_repo(&self, path: &Path) -> Result<RepoInfo, NexusError> {
        let root = match self
            .run(Some(path), &["rev-parse", "--show-toplevel"])
            .await
        {
            Ok(s) => PathBuf::from(s.trim()),
            Err(NexusError::GitCommand { stderr, .. }) => {
                return Err(NexusError::NotARepo(format!(
                    "{}: {stderr}",
                    path.display()
                )));
            }
            Err(e) => return Err(e),
        };
        let root = root.canonicalize().unwrap_or(root);
        let branch_out = self.run(Some(&root), &["branch", "--show-current"]).await?;
        let current_branch = {
            let b = branch_out.trim();
            if b.is_empty() {
                None
            } else {
                Some(b.to_string())
            }
        };
        let status = self.run(Some(&root), &["status", "--porcelain"]).await?;
        Ok(RepoInfo {
            root,
            current_branch,
            is_clean: status.trim().is_empty(),
        })
    }

    // ---- worktree 三方法:Task 8 落地 ----
    // trait 一次定型、占位用 unimplemented!():Task 7 的测试不触达它们。

    async fn worktree_list(&self, _repo: &Path) -> Result<Vec<WorktreeInfo>, NexusError> {
        unimplemented!("Task 8 落地")
    }

    async fn worktree_create(
        &self,
        _repo: &Path,
        _name: &str,
        _base_ref: Option<&str>,
    ) -> Result<(), NexusError> {
        unimplemented!("Task 8 落地")
    }

    async fn worktree_remove(
        &self,
        _repo: &Path,
        _path: &Path,
        _delete_branch: bool,
    ) -> Result<(), NexusError> {
        unimplemented!("Task 8 落地")
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_version, supports_worktree};

    #[test]
    fn parse_version_extracts_version_token() {
        assert_eq!(
            parse_version("git version 2.39.5 (Apple Git-101)"),
            Some("2.39.5".into())
        );
        assert_eq!(parse_version("git version 2.43.0"), Some("2.43.0".into()));
        // Windows 版带四段
        assert_eq!(
            parse_version("git version 2.44.0.windows.1"),
            Some("2.44.0.windows.1".into())
        );
    }

    #[test]
    fn parse_version_rejects_unexpected_output() {
        assert_eq!(parse_version("not git at all"), None);
        // 只有一段数字,不像 x.y 形态的版本
        assert_eq!(parse_version("git version 2"), None);
        assert_eq!(parse_version(""), None);
        // 没有 version 关键字
        assert_eq!(parse_version("2.39.5"), None);
    }

    #[test]
    fn supports_worktree_threshold_is_2_20() {
        assert!(supports_worktree("2.20.0"), "2.20 是稳定期下限");
        assert!(supports_worktree("2.39.5"));
        assert!(supports_worktree("2.50.1"));
        assert!(supports_worktree("3.0.0"));
        assert!(!supports_worktree("2.19.4"));
        assert!(!supports_worktree("2.7.0"));
        assert!(!supports_worktree("1.9.0"));
    }
}
