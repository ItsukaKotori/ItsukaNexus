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

    // ---- worktree 三方法(Task 8)----

    async fn worktree_list(&self, repo: &Path) -> Result<Vec<WorktreeInfo>, NexusError> {
        let out = self
            .run(Some(repo), &["worktree", "list", "--porcelain"])
            .await?;
        Ok(parse_worktree_porcelain(&out))
    }

    async fn worktree_create(
        &self,
        repo: &Path,
        name: &str,
        base_ref: Option<&str>,
    ) -> Result<(), NexusError> {
        // 路径决策在 manager 也有一份(返回给调用方);这里保持一致:
        // <repo>/.nx-worktrees/<name>,name 内含 '/' 由 git 创建中间目录。
        let path = repo.join(".nx-worktrees").join(name);
        let path_str = path.to_string_lossy().into_owned();
        let mut args: Vec<&str> = vec!["worktree", "add", "-b", name, &path_str];
        if let Some(b) = base_ref {
            args.push(b);
        }
        self.run(Some(repo), &args).await?;
        Ok(())
    }

    async fn worktree_remove(
        &self,
        repo: &Path,
        path: &Path,
        delete_branch: bool,
    ) -> Result<(), NexusError> {
        // 分支名要在移除前查(移除后 git 不再认识该 worktree)。
        // delete_branch 失败不致命:分支可能已被人删/已被合并删除,警告即可。
        let branch = self.worktree_branch_of(repo, path).await;
        self.run(Some(repo), &["worktree", "remove", &path.to_string_lossy()])
            .await?;
        if delete_branch {
            if let Some(b) = branch {
                if let Err(e) = self.run(Some(repo), &["branch", "-D", &b]).await {
                    log::warn!("分支 {b} 删除失败(可能已不存在): {e}");
                }
            }
        }
        // prune 吸收失败:残留元数据无害
        let _ = self.run(Some(repo), &["worktree", "prune"]).await;
        Ok(())
    }
}

impl GitCliOps {
    /// 从 porcelain 列表反查 path 处 worktree 的分支名(路径比较两边归一:
    /// macOS /var ↔ /private/var、Windows verbatim 前缀,canonicalize 幂等)。
    async fn worktree_branch_of(&self, repo: &Path, path: &Path) -> Option<String> {
        let out = self
            .run(Some(repo), &["worktree", "list", "--porcelain"])
            .await
            .ok()?;
        let target = normalize_existing(path);
        parse_worktree_porcelain(&out)
            .into_iter()
            .find(|w| normalize_existing(&w.path) == target)
            .and_then(|w| w.branch)
    }
}

/// 路径比较的归一:存在则 canonicalize,不存在则原样(退化为字面量比较)。
fn normalize_existing(p: &Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
}

/// porcelain 契约:块以空行分隔;行关键词 worktree/HEAD/branch/bare/detached。
/// 只认关键词,未知行跳过(git 小版本变动不炸);bare 块跳过。
/// 整行取路径:含空格/中文的 repo 路径不受列位置猜测影响。
pub(crate) fn parse_worktree_porcelain(out: &str) -> Vec<WorktreeInfo> {
    let mut result = Vec::new();
    let mut path: Option<PathBuf> = None;
    let mut branch: Option<String> = None;
    let mut bare = false;
    let flush = |path: &mut Option<PathBuf>,
                 branch: &mut Option<String>,
                 bare: &mut bool,
                 result: &mut Vec<WorktreeInfo>| {
        if let Some(p) = path.take() {
            if !*bare {
                // branch 存短名(去 refs/heads/ 前缀):与 name 同源(spec §1.3
                // name == 分支名 == 目录名),也与 RepoInfo::current_branch 一致;
                // `git branch -D` 只吃短名,全限定 ref 反而不认。
                let short = branch
                    .as_deref()
                    .map(|b| b.trim_start_matches("refs/heads/").to_string())
                    .filter(|b| !b.is_empty());
                let name = short
                    .clone()
                    .or_else(|| p.file_name().map(|f| f.to_string_lossy().into_owned()))
                    .unwrap_or_default();
                result.push(WorktreeInfo {
                    name,
                    path: p,
                    branch: short,
                });
            }
        }
        *branch = None;
        *bare = false;
    };
    for line in out.lines() {
        if line.is_empty() {
            flush(&mut path, &mut branch, &mut bare, &mut result);
            continue;
        }
        if let Some(p) = line.strip_prefix("worktree ") {
            flush(&mut path, &mut branch, &mut bare, &mut result);
            path = Some(PathBuf::from(p));
        } else if let Some(b) = line.strip_prefix("branch ") {
            branch = Some(b.to_string());
        } else if line == "bare" {
            bare = true;
        }
        // HEAD/detached/locked/prunable 等:本场景不需要,跳过
    }
    flush(&mut path, &mut branch, &mut bare, &mut result);
    result
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{parse_version, parse_worktree_porcelain, supports_worktree};

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

    #[test]
    fn parse_porcelain_standard_two_worktrees() {
        // 标准输出快照:主 worktree + 一个 nexus worktree;主 repo 路径含空格
        let out = "\
worktree /tmp/repo with space
HEAD 6cf78bbf16892e48ecef12a92d66d98c7c938453
branch refs/heads/main

worktree /tmp/repo with space/.nx-worktrees/nexus/shell-231114-221320-ab12
HEAD 6cf78bbf16892e48ecef12a92d66d98c7c938453
branch refs/heads/nexus/shell-231114-221320-ab12
";
        let ws = parse_worktree_porcelain(out);
        assert_eq!(ws.len(), 2);
        assert_eq!(ws[0].name, "main");
        assert_eq!(ws[0].branch.as_deref(), Some("main"));
        assert_eq!(ws[0].path, Path::new("/tmp/repo with space"));
        assert_eq!(ws[1].name, "nexus/shell-231114-221320-ab12");
        assert_eq!(
            ws[1].branch.as_deref(),
            Some("nexus/shell-231114-221320-ab12")
        );
        assert_eq!(
            ws[1].path,
            Path::new("/tmp/repo with space/.nx-worktrees/nexus/shell-231114-221320-ab12")
        );
    }

    #[test]
    fn parse_porcelain_skips_bare_block() {
        let out = "\
worktree /srv/bare.git
bare

worktree /srv/checkout
HEAD 6cf78bbf16892e48ecef12a92d66d98c7c938453
branch refs/heads/main
";
        let ws = parse_worktree_porcelain(out);
        assert_eq!(ws.len(), 1, "bare 块不应进列表");
        assert_eq!(ws[0].name, "main");
        assert_eq!(ws[0].path, Path::new("/srv/checkout"));
    }

    #[test]
    fn parse_porcelain_detached_falls_back_to_dir_name() {
        let out = "\
worktree /repo/.nx-worktrees/det-231114-221320-ab12
HEAD 6cf78bbf16892e48ecef12a92d66d98c7c938453
detached
";
        let ws = parse_worktree_porcelain(out);
        assert_eq!(ws.len(), 1);
        assert!(ws[0].branch.is_none(), "detached 无 branch");
        assert_eq!(ws[0].name, "det-231114-221320-ab12", "目录名兜底");
    }
}
