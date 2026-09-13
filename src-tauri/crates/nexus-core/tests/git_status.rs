// gitx status/stage/commit:porcelain v2 -z 集成 + 快照由 status.rs 内联单测钉住。
use std::path::Path;
use std::process::Command;

use nexus_core::gitx::cli::GitCliOps;
use nexus_core::gitx::ops::{FileStatus, GitOps};

fn run_git(dir: &Path, args: &[&str]) {
    let st = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .status()
        .unwrap();
    assert!(st.success(), "git {args:?} 失败");
}

fn init_repo(dir: &Path) {
    run_git(dir, &["init", "-q", "-b", "main"]);
    run_git(dir, &["config", "user.email", "test@nx.local"]);
    run_git(dir, &["config", "user.name", "nx-test"]);
    std::fs::write(dir.join("a.txt"), "a\n").unwrap();
    run_git(dir, &["add", "."]);
    run_git(dir, &["commit", "-qm", "init"]);
}

#[tokio::test]
async fn status_reports_branch_entries_and_staging_lifecycle() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path());
    let ops = GitCliOps::new();

    // 干净:branch=main,无条目
    let st = ops.status(dir.path()).await.unwrap();
    assert_eq!(st.branch.as_deref(), Some("main"));
    assert!(st.entries.is_empty(), "刚提交完应干净: {st:?}");
    assert!(!st.truncated);

    // 修改 + 未跟踪 + 重命名(中文文件名走 -z 原样路径)
    std::fs::write(dir.path().join("a.txt"), "a2\n").unwrap();
    std::fs::write(dir.path().join("新 文件.txt"), "n\n").unwrap();
    std::fs::write(dir.path().join("b.txt"), "b\n").unwrap();
    run_git(dir.path(), &["add", "b.txt"]);
    run_git(dir.path(), &["commit", "-qm", "b"]);
    run_git(dir.path(), &["mv", "b.txt", "b-renamed.txt"]);

    let st = ops.status(dir.path()).await.unwrap();
    let find = |p: &str| {
        st.entries
            .iter()
            .find(|e| e.path == Path::new(p))
            .unwrap_or_else(|| panic!("缺 {p}: {:?}", st.entries))
    };
    let a = find("a.txt");
    assert_eq!(a.index, None);
    assert_eq!(a.worktree, Some(FileStatus::Modified));
    let un = find("新 文件.txt");
    assert_eq!(un.worktree, Some(FileStatus::Untracked));
    let rn = find("b-renamed.txt");
    assert_eq!(rn.index, Some(FileStatus::Renamed));
    assert_eq!(rn.orig_path.as_deref(), Some(Path::new("b.txt")));

    // 暂存全部 → a 变为 index 侧 Modified;提交 → 干净
    ops.stage(dir.path(), None).await.unwrap();
    let st = ops.status(dir.path()).await.unwrap();
    assert_eq!(find_status(&st, "a.txt").index, Some(FileStatus::Modified));
    ops.commit(dir.path(), "更新 a 与新文件").await.unwrap();
    let st = ops.status(dir.path()).await.unwrap();
    assert!(st.entries.is_empty(), "提交后应干净: {st:?}");
}

fn find_status<'a>(
    st: &'a nexus_core::gitx::ops::GitStatus,
    p: &str,
) -> &'a nexus_core::gitx::ops::GitStatusEntry {
    st.entries.iter().find(|e| e.path == Path::new(p)).unwrap()
}

#[tokio::test]
async fn status_ahead_behind_from_branch_header() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path());
    let origin = tempfile::tempdir().unwrap();
    run_git(origin.path(), &["init", "-q", "--bare"]);
    run_git(
        dir.path(),
        &["remote", "add", "origin", origin.path().to_str().unwrap()],
    );
    run_git(dir.path(), &["push", "-q", "-u", "origin", "main"]);
    // 本地再提交一笔 → ahead 1 / behind 0
    std::fs::write(dir.path().join("c.txt"), "c\n").unwrap();
    run_git(dir.path(), &["add", "."]);
    run_git(dir.path(), &["commit", "-qm", "c"]);

    let ops = GitCliOps::new();
    let st = ops.status(dir.path()).await.unwrap();
    assert_eq!(st.ahead, 1);
    assert_eq!(st.behind, 0);
}

#[tokio::test]
async fn commit_rejects_empty_message() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path());
    let ops = GitCliOps::new();
    assert!(
        ops.commit(dir.path(), "  ").await.is_err(),
        "空白提交信息应拒绝"
    );
}

#[tokio::test]
async fn stage_selected_paths_only() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path());
    std::fs::write(dir.path().join("x.txt"), "x\n").unwrap();
    std::fs::write(dir.path().join("y.txt"), "y\n").unwrap();
    let ops = GitCliOps::new();
    ops.stage(dir.path(), Some(&[Path::new("x.txt").to_path_buf()]))
        .await
        .unwrap();
    let st = ops.status(dir.path()).await.unwrap();
    assert_eq!(find_status(&st, "x.txt").index, Some(FileStatus::Added));
    assert_eq!(find_status(&st, "y.txt").index, None);
}
