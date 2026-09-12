// session × worktree 全链路:真实 git + 真实 shell(M3 Task 11)。
// 钉住 core 级闭环:建 worktree → 开会话落 worktree(pwd 断言)→ 快照带
// 落点 → 退出 dispose(worktree 仍在)→ 显式 worktree_remove 清理。
// shell 交互回显断言只在 unix 可靠(同 session_manager.rs 口径);Windows 的
// worktree 纯 git 部分已由 worktree_manager.rs 三平台覆盖。
#![cfg(unix)]

mod common;

use std::sync::Arc;

use common::{init_repo_at, manager_with_channel, wait_exit, wait_frame_contains};
use nexus_core::agent::manager::LaunchSpec;
use nexus_core::gitx::cli::GitCliOps;
use nexus_core::gitx::worktree::WorktreeManager;

#[tokio::test]
async fn session_runs_inside_created_worktree_and_cleans_up() {
    // 真实 git 仓库 + 真实 worktree(事件 sink 不检:闭环断言在会话/目录侧)
    let dir = tempfile::tempdir().unwrap();
    init_repo_at(dir.path());
    let wt_mgr = WorktreeManager::new(Arc::new(GitCliOps::new()), Arc::new(|_| {}));
    let wt = wt_mgr.create(dir.path(), "shell", None).await.unwrap();
    assert!(wt.path.is_dir(), "worktree 目录应存在");

    let (mgr, mut sink_rx) = manager_with_channel();
    let spec = LaunchSpec {
        cwd: wt.path.clone(),
        repo_path: Some(dir.path().to_string_lossy().into_owned()),
        worktree_name: Some(wt.name.clone()),
    };
    let id = mgr.create("shell", 100, 30, Some(&spec)).await.unwrap();
    let sub = mgr.subscribe(id).expect("subscribe 失败");
    let mut frames = sub.rx;

    // 快照带落点:repo_path/worktree_name 原样透传,前端据它标记 tab 归属
    let snap = mgr
        .list()
        .into_iter()
        .find(|s| s.session_id == id)
        .expect("会话应在表中");
    assert_eq!(
        snap.repo_path.as_deref(),
        Some(dir.path().to_string_lossy().as_ref()),
        "快照应带回 repo_path"
    );
    assert_eq!(
        snap.worktree_name.as_deref(),
        Some(wt.name.as_str()),
        "快照应带回 worktree_name"
    );

    // 终端确实在 worktree 里(完成标准①的 core 对应物)。shell 就绪时间不定:
    // 输入停在 PTY 缓冲,起来后照常读到;create 返回的 path 已 canonicalize,
    // pwd 侧再 canonicalize 归一(macOS /var ↔ /private/var 两侧同源)
    mgr.send_input(id, "pwd\n").await.expect("send 失败");
    let wt_canon = wt.path.canonicalize().unwrap();
    wait_frame_contains(&mut frames, &wt_canon.to_string_lossy()).await;

    // 退出 → dispose → worktree 仍在(删除是显式决策,不是自动)
    mgr.send_input(id, "exit\n").await.expect("send 失败");
    let code = wait_exit(&mut sink_rx, id).await;
    assert_eq!(code, 0, "自然退出的 shell 退出码应为 0");
    mgr.dispose(id).expect("终态会话应可 dispose");
    assert!(mgr.list().is_empty(), "dispose 后条目应消失");
    assert!(
        wt.path.is_dir(),
        "dispose 不删 worktree(清理走显式 worktree_remove)"
    );

    // 显式清理(完成标准③)
    wt_mgr
        .remove(dir.path(), &wt.name, true)
        .await
        .expect("worktree remove 失败");
    assert!(!wt.path.exists(), "显式清理后 worktree 目录应删除");
}
