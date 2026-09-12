// 集成测试共享辅助(session_manager / session_worktree / worktree_manager)。
// 各函数自原文件原样提级(行为不变、断言不弱化);每个测试二进制只用到其中
// 一部分,未用到的以 allow(dead_code) 压制(测试模块不可达,死代码属预期)。
#![allow(dead_code)]

use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use nexus_core::agent::manager::{OutputFrame, SessionEvent, SessionManager};
use nexus_core::ids::SessionId;

/// 单条测试的整体 deadline:真实 spawn shell,就绪时间不定,超时即失败
pub const TEST_TIMEOUT: Duration = Duration::from_secs(10);

pub type EventRx = tokio::sync::mpsc::UnboundedReceiver<SessionEvent>;

pub fn manager_with_channel() -> (SessionManager, EventRx) {
    // unbounded:sink 闭包在异步任务上下文被同步调用,不能 .await
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let mgr = SessionManager::new(Arc::new(move |ev| {
        let _ = tx.send(ev);
    }));
    (mgr, rx)
}

pub fn deadline() -> tokio::time::Instant {
    tokio::time::Instant::now() + TEST_TIMEOUT
}

/// deadline 内收一条事件;超时/通道关闭即 panic
pub async fn next_event(rx: &mut EventRx, dl: tokio::time::Instant) -> SessionEvent {
    let now = tokio::time::Instant::now();
    assert!(now < dl, "10 秒内未等到事件");
    tokio::time::timeout(dl - now, rx.recv())
        .await
        .expect("10 秒内未等到事件")
        .expect("事件通道意外关闭")
}

/// 等 id 的 Exit 事件(跳过途中的 State 事件),返回退出码
pub async fn wait_exit(rx: &mut EventRx, id: SessionId) -> i32 {
    let dl = deadline();
    loop {
        match next_event(rx, dl).await {
            SessionEvent::Exit {
                session_id: ev_id,
                code,
            } if ev_id == id => return code,
            _ => {}
        }
    }
}

/// deadline 内等包含 needle 的输出帧;返回沿途收到的全部帧(供 seq 断言)
pub async fn wait_frame_contains(
    rx: &mut tokio::sync::mpsc::Receiver<OutputFrame>,
    needle: &str,
) -> Vec<OutputFrame> {
    let dl = deadline();
    let mut seen = Vec::new();
    loop {
        let now = tokio::time::Instant::now();
        assert!(now < dl, "10 秒内未等到包含 {needle:?} 的输出帧");
        let frame = tokio::time::timeout(dl - now, rx.recv())
            .await
            .expect("10 秒内未等到输出帧")
            .expect("订阅通道意外关闭");
        let hit = frame.data.contains(needle);
        seen.push(frame);
        if hit {
            return seen;
        }
    }
}

/// 真实 git 初始化临时仓库(worktree 测试的公共前置)
pub fn init_repo_at(dir: &Path) {
    let run = |args: &[&str]| {
        let st = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .status()
            .unwrap();
        assert!(st.success(), "git {args:?} 失败");
    };
    run(&["init", "-q"]);
    run(&["config", "user.email", "test@nx.local"]);
    run(&["config", "user.name", "nx-test"]);
    std::fs::write(dir.join("README.md"), "# t\n").unwrap();
    run(&["add", "."]);
    run(&["commit", "-qm", "init"]);
}
