// SessionManager 集成测试:真实 spawn 默认 shell,经事件 sink 观察输出。
// 交互回显断言只在 unix 可靠(windows 的 powershell 交互编码是 M2 议题),
// windows CI 覆盖靠 Task 2 的 pty_session 一次性测试。
#![cfg(unix)]

use std::sync::mpsc;
use std::time::{Duration, Instant};

use itsukanexus_lib::agent::manager::{SessionEvent, SessionManager};
use itsukanexus_lib::ids::SessionId;

fn manager_with_channel() -> (SessionManager, mpsc::Receiver<SessionEvent>) {
    let (tx, rx) = mpsc::channel();
    let mgr = SessionManager::new(std::sync::Arc::new(move |ev| {
        let _ = tx.send(ev);
    }));
    (mgr, rx)
}

fn wait_output_contains(rx: &mpsc::Receiver<SessionEvent>, id: SessionId, needle: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(500)) {
            Ok(SessionEvent::Output { id: ev_id, data }) if ev_id == id => {
                if data.contains(needle) {
                    return;
                }
            }
            Ok(SessionEvent::Exit { id: ev_id, code }) if ev_id == id => {
                panic!("会话提前退出(code={code}),还没等到 {needle:?}")
            }
            Ok(_) => {}
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => panic!("事件通道意外关闭"),
        }
    }
    panic!("10 秒内未收到包含 {needle:?} 的输出");
}

#[test]
fn create_shell_send_input_sees_echo() {
    let (mgr, rx) = manager_with_channel();
    let id = mgr.create("shell", 80, 24).expect("create 失败");

    // shell 就绪时间不定:提示符可能出现较晚,先等 shell 启动输出
    // (zsh/bash 启动一般会有提示符或至少空输出;直接发命令等回显)
    mgr.send_input(id, "echo marker-manager-42\n")
        .expect("send 失败");
    wait_output_contains(&rx, id, "marker-manager-42");
}

#[test]
fn stop_kills_session_and_emits_exit() {
    let (mgr, rx) = manager_with_channel();
    let id = mgr.create("shell", 80, 24).expect("create 失败");
    mgr.stop(id).expect("stop 失败");

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(Instant::now() < deadline, "10 秒内未收到 Exit 事件");
        match rx.recv_timeout(Duration::from_millis(500)) {
            Ok(SessionEvent::Exit { id: ev_id, code }) if ev_id == id => {
                assert_ne!(code, 0, "被 kill 的 shell 退出码应非 0");
                break;
            }
            Ok(_) => {}
            Err(_) => continue,
        }
    }
    // 停止后注册表里已无此会话
    let err = mgr.send_input(id, "should fail\n").unwrap_err();
    assert!(matches!(
        err,
        itsukanexus_lib::error::NexusError::SessionNotFound(_)
    ));
}

#[test]
fn create_rejects_unknown_provider() {
    let (mgr, _rx) = manager_with_channel();
    let err = mgr.create("claude", 80, 24).unwrap_err();
    assert!(matches!(
        err,
        itsukanexus_lib::error::NexusError::UnsupportedProvider(_)
    ));
}

#[test]
fn two_sessions_are_independent() {
    let (mgr, rx) = manager_with_channel();
    let a = mgr.create("shell", 80, 24).unwrap();
    let b = mgr.create("shell", 80, 24).unwrap();
    assert_ne!(a, b);

    mgr.send_input(b, "echo only-in-b\n").unwrap();
    wait_output_contains(&rx, b, "only-in-b");
    // a 没收到 b 的命令输出(只检查 b 的输出确实属于 b)
    mgr.stop(a).expect("stop a 失败");
    mgr.stop(b).expect("stop b 失败");
}
