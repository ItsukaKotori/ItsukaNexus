// PtySession 集成测试:真实 spawn 子进程,验证 spawn/读/写/kill 全链路。
// 交互回显(/bin/cat)只在 unix 存在,该测试 cfg(unix);
// 跨平台的一次性输出测试(sh/cmd)单独一条,保证 CI Windows 也有覆盖。
use std::io::Read;
use std::time::Duration;

use itsukanexus_lib::ids::SessionId;
use itsukanexus_lib::pty::session::PtySession;

fn recv_contains(reader: &mut dyn Read, needle: &str) -> String {
    // 带超时的读取循环:5 秒内没等到目标内容即失败,防测试挂死
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let mut acc = Vec::new();
    let mut buf = [0u8; 8192];
    while std::time::Instant::now() < deadline {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                acc.extend_from_slice(&buf[..n]);
                if String::from_utf8_lossy(&acc).contains(needle) {
                    return String::from_utf8_lossy(&acc).into_owned();
                }
            }
            Err(e) => panic!("read error: {e}"),
        }
    }
    panic!(
        "5 秒内未读到 {:?},实际收到: {:?}",
        needle,
        String::from_utf8_lossy(&acc)
    );
}

#[test]
fn spawn_run_and_exit_oneshot() {
    // 一次性命令:输出后自然退出,wait 拿到退出码
    let (prog, args): (&str, Vec<&str>) = if cfg!(windows) {
        ("cmd.exe", vec!["/c", "echo hello-pty"])
    } else {
        ("/bin/sh", vec!["-c", "echo hello-pty"])
    };
    let (sess, mut child) =
        PtySession::spawn(SessionId::new(), prog, &args, 80, 24).expect("spawn 失败");
    let mut reader = sess.take_reader();
    let output = recv_contains(&mut reader, "hello-pty");
    assert!(output.contains("hello-pty"));
    let status = child.wait().expect("wait 失败");
    assert_eq!(status.exit_code(), 0);
}

#[cfg(unix)]
#[test]
fn write_input_gets_echoed_by_cat() {
    // 交互回显:cat 把 stdin 原样吐回,验证 write_all → PTY → read 全链路
    let (sess, mut child) =
        PtySession::spawn(SessionId::new(), "/bin/cat", &[], 80, 24).expect("spawn 失败");
    let mut reader = sess.take_reader();

    sess.write_all(b"marker-xyz-9876\n").expect("write 失败");
    recv_contains(&mut reader, "marker-xyz-9876");

    // kill 后 wait 应能返回(子进程被信号杀死,退出码非 0 即可)
    sess.kill().expect("kill 失败");
    let status = child.wait().expect("kill 后 wait 失败");
    assert_ne!(status.exit_code(), 0);
}

#[test]
fn resize_does_not_error() {
    let (prog, args): (&str, Vec<&str>) = if cfg!(windows) {
        ("cmd.exe", vec!["/c", "exit", "0"])
    } else {
        ("/bin/sh", vec!["-c", "exit 0"])
    };
    let (sess, mut child) =
        PtySession::spawn(SessionId::new(), prog, &args, 80, 24).expect("spawn 失败");
    sess.resize(120, 40).expect("resize 失败");
    let _ = child.wait();
}
