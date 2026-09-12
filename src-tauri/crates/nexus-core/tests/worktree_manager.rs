// WorktreeManager 全流程:真实 git(CI 三平台预装)。
// 覆盖完成标准⑥:含空格/中文的 repo 路径。
mod common;

use std::ffi::OsStr;
use std::process::Command;
use std::sync::Arc;
use std::sync::Mutex;

use common::init_repo_at;
use nexus_core::error::NexusError;
use nexus_core::gitx::cli::GitCliOps;
// 走 manager 编排,测试内不直接调 GitOps trait 方法(trait 无需导入)
use nexus_core::gitx::worktree::{WorktreeChange, WorktreeManager};
use nexus_core::ids::WorktreeName;

fn manager_with_events() -> (WorktreeManager, Arc<Mutex<Vec<WorktreeChange>>>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let s2 = seen.clone();
    let events = Arc::new(move |ev: nexus_core::gitx::worktree::WorktreeChanged| {
        s2.lock().unwrap().push(ev.change);
    });
    (
        WorktreeManager::new(Arc::new(GitCliOps::new()), events),
        seen,
    )
}

#[test]
fn worktree_name_format() {
    let n = WorktreeName::generate("shell", 1_800_000_000); // 2027-01-15 附近
    let s = n.as_str();
    assert!(s.starts_with("nexus/shell-"), "{s}");
    // nexus/shell-<yyMMdd-HHmmss>-<rand4>:rand4 是 4 hex
    let tail = s.rsplit('-').next().unwrap();
    assert_eq!(tail.len(), 4);
    assert!(tail.chars().all(|c| c.is_ascii_hexdigit()));
    // 时间段可解析回 6+6 数字
    let mid = s.trim_start_matches("nexus/shell-");
    let mid = &mid[..mid.len() - 5]; // 去掉 -rand4
    let (d, t) = mid.split_once('-').unwrap();
    assert_eq!(d.len(), 6);
    assert_eq!(t.len(), 6);
}

#[tokio::test]
async fn create_list_remove_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    init_repo_at(dir.path());
    let (mgr, events) = manager_with_events();

    let info = mgr.create(dir.path(), "shell", None).await.unwrap();
    // 路径基准:同一侧归一(macOS tempfile 在 /var → git/canonicalize 报 /private/var)
    let base = dir.path().join(".nx-worktrees");
    let base = base.canonicalize().unwrap_or(base);
    assert!(info.path.starts_with(base));
    assert!(info.path.is_dir(), "worktree 目录应存在");
    // 跨平台断言(Windows 断言不可写字面 contains):name 含 '/'
    // (nexus/shell-…),Windows 路径分隔符是 '\',字面子串必失败。改按
    // 结构比:目录最后一段 == name 尾段,且路径落在 .nx-worktrees 之下
    // (canonicalize 后可能是 /private/var 或 \\?\ 前缀形态,逐 component
    // 比较与分隔符无关,三平台同义)
    let leaf = info.name.as_str().rsplit('/').next().unwrap();
    assert_eq!(
        info.path.file_name().unwrap(),
        OsStr::new(leaf),
        "worktree 目录最后一段应与 name 尾段一致:{:?}",
        info.path
    );
    assert!(
        info.path
            .components()
            .any(|c| c.as_os_str() == OsStr::new(".nx-worktrees")),
        "worktree 应位于 .nx-worktrees 下:{:?}",
        info.path
    );
    // 分支存在(rev-parse --verify)
    let st = Command::new("git")
        .arg("-C")
        .arg(dir.path())
        .args([
            "rev-parse",
            "--verify",
            &format!("refs/heads/{}", info.name),
        ])
        .status()
        .unwrap();
    assert!(st.success(), "分支 {} 应存在", info.name);

    let list = mgr.list(dir.path()).await.unwrap();
    assert!(
        list.iter().any(|w| w.name == info.name),
        "list 应含新 worktree(且含主 worktree)"
    );

    // 在 worktree 里提交一笔(先 commit 保证干净,worktree remove 才接受)
    std::fs::write(info.path.join("wt.txt"), "x").unwrap();
    let run = |args: &[&str]| {
        Command::new("git")
            .arg("-C")
            .arg(&info.path)
            .args(args)
            .status()
            .unwrap()
    };
    assert!(run(&["add", "."]).success());
    assert!(run(&["commit", "-qm", "wt"]).success());

    mgr.remove(dir.path(), &info.name, true).await.unwrap();
    assert!(!info.path.exists(), "目录应删除");
    let list = mgr.list(dir.path()).await.unwrap();
    assert!(!list.iter().any(|w| w.name == info.name));
    // 分支也应消失(delete_branch = true;wt 提交未合并,-D 才删得掉)
    let st = Command::new("git")
        .arg("-C")
        .arg(dir.path())
        .args([
            "rev-parse",
            "--verify",
            &format!("refs/heads/{}", info.name),
        ])
        .status()
        .unwrap();
    assert!(!st.success(), "分支 {} 应已删除", info.name);
    // 事件顺序:Created → Removed
    let ev = events.lock().unwrap();
    assert!(matches!(
        ev.as_slice(),
        [WorktreeChange::Created, WorktreeChange::Removed]
    ));
}

/// 完成标准⑥:含空格与中文的 repo 路径(三平台 CI 都会跑这条)
#[tokio::test]
async fn paths_with_spaces_and_cjk_work() {
    let dir = tempfile::Builder::new()
        .prefix("nx repo 测 试")
        .tempdir()
        .unwrap();
    init_repo_at(dir.path());
    let (mgr, _events) = manager_with_events();
    let info = mgr.create(dir.path(), "shell", None).await.unwrap();
    assert!(info.path.is_dir());
    mgr.remove(dir.path(), &info.name, true).await.unwrap();
    assert!(!info.path.exists());
}

#[tokio::test]
async fn remove_unknown_name_errors() {
    let dir = tempfile::tempdir().unwrap();
    init_repo_at(dir.path());
    let (mgr, _events) = manager_with_events();
    match mgr
        .remove(dir.path(), "nexus/nope-000000-0000-0000", false)
        .await
    {
        Err(NexusError::GitCommand { .. }) => {}
        other => panic!("期望 GitCommand 错误,得到 {other:?}"),
    }
}
