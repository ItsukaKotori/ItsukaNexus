// SessionManager 集成测试:真实 spawn 默认 shell。
// M2 编排重写后的事件缝:sink 只承载低频 State/Exit;Output 走 per-session
// subscribe(带 replay)。交互回显断言只在 unix 可靠(windows 的 powershell
// 交互编码是 M2 议题),windows CI 覆盖靠 Task 2 的 pty_session 一次性测试。
#![cfg(unix)]

use std::sync::Arc;
use std::time::Duration;

use itsukanexus_lib::agent::manager::{OutputFrame, SessionEvent, SessionManager};
use itsukanexus_lib::agent::state::SessionState;
use itsukanexus_lib::error::NexusError;
use itsukanexus_lib::ids::SessionId;

/// 单条测试的整体 deadline:真实 spawn shell,就绪时间不定,超时即失败
const TEST_TIMEOUT: Duration = Duration::from_secs(10);

type EventRx = tokio::sync::mpsc::UnboundedReceiver<SessionEvent>;

fn manager_with_channel() -> (SessionManager, EventRx) {
    // unbounded:sink 闭包在异步任务上下文被同步调用,不能 .await
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let mgr = SessionManager::new(Arc::new(move |ev| {
        let _ = tx.send(ev);
    }));
    (mgr, rx)
}

fn deadline() -> tokio::time::Instant {
    tokio::time::Instant::now() + TEST_TIMEOUT
}

/// deadline 内收一条事件;超时/通道关闭即 panic
async fn next_event(rx: &mut EventRx, dl: tokio::time::Instant) -> SessionEvent {
    let now = tokio::time::Instant::now();
    assert!(now < dl, "10 秒内未等到事件");
    tokio::time::timeout(dl - now, rx.recv())
        .await
        .expect("10 秒内未等到事件")
        .expect("事件通道意外关闭")
}

/// 等 id 的 Exit 事件(跳过途中的 State 事件),返回退出码
async fn wait_exit(rx: &mut EventRx, id: SessionId) -> i32 {
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
async fn wait_frame_contains(
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

#[tokio::test]
async fn create_shell_send_input_sees_echo() {
    let (mgr, _rx) = manager_with_channel();
    let id = mgr.create("shell", 80, 24).await.expect("create 失败");
    let sub = mgr.subscribe(id).expect("subscribe 失败");
    let mut frames = sub.rx;

    // shell 就绪时间不定:输入停在 PTY 缓冲,shell 起来后照常读到并回显
    mgr.send_input(id, "echo marker-manager-42\n")
        .await
        .expect("send 失败");
    wait_frame_contains(&mut frames, "marker-manager-42").await;
}

#[tokio::test]
async fn stop_kills_session_and_emits_exit() {
    let (mgr, mut rx) = manager_with_channel();
    let id = mgr.create("shell", 80, 24).await.expect("create 失败");
    mgr.stop(id, true).await.expect("stop 失败");

    let code = wait_exit(&mut rx, id).await;
    assert_ne!(code, 0, "被 kill 的 shell 退出码应非 0");

    // M2:退出后条目保留;状态写回先于 Exit 事件,收到 Exit 即可见终态
    let snap = mgr
        .list()
        .into_iter()
        .find(|s| s.session_id == id)
        .expect("退出后条目必须保留");
    assert_eq!(snap.state, SessionState::Exited);

    // 条目还在但不在 Running:明确报"未在运行",而非"不存在"
    let err = mgr.send_input(id, "should fail\n").await.unwrap_err();
    assert!(matches!(err, NexusError::SessionNotRunning(_)));
}

#[tokio::test]
async fn natural_exit_emits_zero_code_and_snapshot() {
    // 自然退出:钉住"join 输出管线后再发 Exit"的不变量 + 退出后快照可查
    let (mgr, mut rx) = manager_with_channel();
    let id = mgr.create("shell", 80, 24).await.expect("create 失败");

    // shell 尚未就绪也没关系:输入停在 PTY 缓冲,shell 起来后照常读到
    mgr.send_input(id, "exit\n").await.expect("send 失败");

    let code = wait_exit(&mut rx, id).await;
    assert_eq!(code, 0, "自然退出的 shell 退出码应为 0");

    let snap = mgr
        .list()
        .into_iter()
        .find(|s| s.session_id == id)
        .expect("退出后条目必须保留");
    assert_eq!(snap.state, SessionState::Exited);
    assert_eq!(snap.exit_code, Some(0));
}

#[tokio::test]
async fn create_rejects_unknown_provider() {
    let (mgr, _rx) = manager_with_channel();
    let err = mgr.create("claude", 80, 24).await.unwrap_err();
    assert!(matches!(err, NexusError::UnsupportedProvider(_)));
}

#[tokio::test]
async fn two_sessions_are_independent() {
    let (mgr, _rx) = manager_with_channel();
    let a = mgr.create("shell", 80, 24).await.unwrap();
    let b = mgr.create("shell", 80, 24).await.unwrap();
    assert_ne!(a, b);

    let sub = mgr.subscribe(b).expect("subscribe b 失败");
    let mut frames = sub.rx;
    mgr.send_input(b, "echo only-in-b\n").await.unwrap();
    wait_frame_contains(&mut frames, "only-in-b").await;

    mgr.stop(a, true).await.expect("stop a 失败");
    mgr.stop(b, true).await.expect("stop b 失败");
}

#[tokio::test]
async fn create_emits_state_running_before_any_output() {
    let (mgr, mut rx) = manager_with_channel();
    let id = mgr.create("shell", 80, 24).await.expect("create 失败");

    // create 返回前 State{Running} 已发出:sink 队列的第一条必是它
    match next_event(&mut rx, deadline()).await {
        SessionEvent::State(change) => {
            assert_eq!(change.session_id, id);
            assert_eq!(change.next, SessionState::Running);
        }
        other => panic!("首个事件应是 State(Running),实际 {other:?}"),
    }

    // 之后的输出经订阅通道到达(Output 不走全局 sink)
    let sub = mgr.subscribe(id).expect("subscribe 失败");
    let mut frames = sub.rx;
    mgr.send_input(id, "echo after-state-1\n")
        .await
        .expect("send 失败");
    wait_frame_contains(&mut frames, "after-state-1").await;
}

#[tokio::test]
async fn subscribe_replays_history_and_keeps_seq_monotonic() {
    let (mgr, _rx) = manager_with_channel();
    let id = mgr.create("shell", 80, 24).await.expect("create 失败");

    // 先订阅、等历史输出落地,再换订阅验证 replay
    let first = mgr.subscribe(id).expect("subscribe 失败");
    let mut first_rx = first.rx;
    mgr.send_input(id, "echo marker-sub-77\n")
        .await
        .expect("send 失败");
    let seen = wait_frame_contains(&mut first_rx, "marker-sub-77").await;
    for pair in seen.windows(2) {
        assert!(pair[0].seq < pair[1].seq, "订阅内 seq 必须严格递增");
    }

    // 重复 subscribe 替换旧订阅:旧通道在存量耗尽后关闭
    let mut second = mgr.subscribe(id).expect("re-subscribe 失败");
    assert!(
        second.replay.contains("marker-sub-77"),
        "replay 应包含订阅前的输出,实际 {:?}",
        second.replay
    );
    // 旧通道 drain 至关闭:负载下 marker 之后的提示符帧可能仍在旧通道,
    // 单次 recv 断言 None 会 flake;deadline 内循环收完存量,通道终将关闭
    // (发送端已被替换,克隆随发送完成耗尽),存量帧内容不 assert
    let dl = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let now = tokio::time::Instant::now();
        assert!(now < dl, "5 秒内旧订阅通道未关闭");
        match tokio::time::timeout(dl - now, first_rx.recv()).await {
            Ok(Some(_leftover)) => {}
            Ok(None) => break, // 发送端已替换:通道关闭,断言成立
            Err(_) => panic!("5 秒内旧订阅通道未关闭"),
        }
    }

    // 后续输出经新通道到达,seq 跨订阅单调不回退
    mgr.send_input(id, "echo after-replay-88\n")
        .await
        .expect("send 失败");
    let tail = wait_frame_contains(&mut second.rx, "after-replay-88").await;
    for pair in tail.windows(2) {
        assert!(pair[0].seq < pair[1].seq, "新订阅内 seq 必须严格递增");
    }
    let last_before = seen.last().expect("至少一帧").seq;
    if let Some(f) = tail.first() {
        assert!(f.seq >= last_before, "seq 必须跨订阅单调不回退");
    }
}
