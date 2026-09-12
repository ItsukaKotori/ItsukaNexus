// gitx CLI 实现的集成测试:真实调 git(CI 三平台均预装)。
// 临时 repo 用 tempfile::tempdir(自动清理;spec M3 学习主题点名)。
use std::process::Command;

use nexus_core::error::NexusError;
use nexus_core::gitx::cli::GitCliOps;
use nexus_core::gitx::ops::GitOps;

/// 建一个有一次提交的临时 git repo,返回其根目录(tempdir 句柄由调用方持有)
fn init_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let run = |args: &[&str]| {
        let st = Command::new("git")
            .args(["-C", dir.path().to_str().unwrap()])
            .args(args)
            .status()
            .unwrap();
        assert!(st.success(), "git {args:?} 失败");
    };
    // 不带 -b:老 git(< 2.28)没有该参数,分支名断言只验 Some(非空)
    run(&["init", "-q"]);
    run(&["config", "user.email", "test@nx.local"]);
    run(&["config", "user.name", "nx-test"]);
    std::fs::write(dir.path().join("README.md"), "# t\n").unwrap();
    run(&["add", "."]);
    run(&["commit", "-qm", "init"]);
    dir
}

#[tokio::test]
async fn check_finds_git_and_version() {
    let ops = GitCliOps::new();
    let info = ops.check().await;
    assert!(info.available, "CI/开发机都应装有 git");
    let v = info.version.expect("版本应解析出来");
    assert!(
        v.starts_with(|c: char| c.is_ascii_digit()),
        "版本串形如 2.39.5: {v}"
    );
    assert!(info.worktree_supported, ">= 2.20 才支持 worktree");
    assert!(info.path.is_some());
}

#[tokio::test]
async fn check_with_missing_binary_reports_unavailable() {
    let ops = GitCliOps::with_bin("definitely-not-git-nx");
    let info = ops.check().await;
    assert!(!info.available);
    assert!(info.version.is_none());
    assert!(!info.worktree_supported);
}

#[tokio::test]
async fn validate_repo_roundtrip() {
    let ops = GitCliOps::new();
    let dir = init_repo();
    let info = ops.validate_repo(dir.path()).await.unwrap();
    assert_eq!(info.root, dir.path().canonicalize().unwrap());
    assert!(
        info.current_branch
            .as_deref()
            .is_some_and(|b| !b.is_empty()),
        "init 后应停在某分支上"
    );
    assert!(info.is_clean, "刚 commit 完应干净");

    std::fs::write(dir.path().join("dirty.txt"), "x").unwrap();
    let info = ops.validate_repo(dir.path()).await.unwrap();
    assert!(!info.is_clean, "未跟踪文件应视为脏");
}

#[tokio::test]
async fn validate_rejects_non_repo() {
    let dir = tempfile::tempdir().unwrap();
    let ops = GitCliOps::new();
    match ops.validate_repo(dir.path()).await {
        Err(NexusError::NotARepo(_)) => {}
        other => panic!("期望 NotARepo,得到 {other:?}"),
    }
}
