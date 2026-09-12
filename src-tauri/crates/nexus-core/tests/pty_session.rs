// PtySession 集成测试:真实 spawn 子进程,验证 spawn/读/写/kill 全链路。
// 交互回显(/bin/cat)只在 unix 存在,该测试 cfg(unix);
// 跨平台的一次性输出测试(sh/cmd)单独一条,保证 CI Windows 也有覆盖。
//
// 所有阻塞操作(read/wait)一律放入辅助线程,测试线程只做 recv_timeout:
// Windows ConPTY 无数据时 read 不返回、wait 也可能阻塞,deadline 无法打断
// 阻塞中的 read,只能在两次 read 之间检查,超时形同虚设。辅助线程在测试
// 结束/超时后泄漏,随测试进程退出而回收,这是有意为之。
use std::io::Read;
use std::time::Duration;

use nexus_core::pty::session::PtySession;

/// 有界读取:阻塞 read 在辅助线程进行(泄漏随测试进程退出,可接受),
/// 测试线程只做 recv_timeout。超时时 panic 并带上已收到的内容,便于 CI 诊断。
///
/// Windows ConPTY 启动期会向终端发 DSR 光标查询(`ESC[6n`),等到应答
/// (`ESC[<row>;<col>R`)后才继续产出子进程输出;真实应用里 xterm.js 自动
/// 应答,裸测试读端经由 `sess` 代答 `ESC[1;1R`,否则超时且只收到查询本身。
fn recv_contains(reader: Box<dyn Read + Send>, sess: &PtySession, needle: &str) -> String {
    let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        let mut reader = reader;
        let mut buf = [0u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if tx.send(buf[..n].to_vec()).is_err() {
                        break; // 测试线程已超时退出
                    }
                }
            }
        }
    });
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let mut acc = Vec::new();
    // 已应答扫描位:该位置之前的字节均已检查过 DSR 并代答(可多次出现,每次都答)
    let mut dsr_replied: usize = 0;
    loop {
        let now = std::time::Instant::now();
        if now >= deadline {
            panic!(
                "10 秒内未读到 {:?},实际收到: {:?}",
                needle,
                String::from_utf8_lossy(&acc)
            );
        }
        match rx.recv_timeout(deadline - now) {
            Ok(chunk) => {
                acc.extend_from_slice(&chunk);
                // ConPTY 启动期发 ESC[6n 询问光标位置,无应答则不再产出后续输出;
                // 真实终端(xterm.js)会自动应答,裸测试在此代答
                while let Some(pos) = acc[dsr_replied..].windows(4).position(|w| w == b"\x1b[6n") {
                    let _ = sess.write_all(b"\x1b[1;1R");
                    dsr_replied += pos + 4;
                }
                if String::from_utf8_lossy(&acc).contains(needle) {
                    return String::from_utf8_lossy(&acc).into_owned();
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                panic!(
                    "读线程已结束(EOF/错误)仍未见到 {:?},实际收到: {:?}",
                    needle,
                    String::from_utf8_lossy(&acc)
                );
            }
        }
    }
}

#[test]
fn spawn_run_and_exit_oneshot() {
    // 一次性命令:输出后自然退出,wait 拿到退出码
    let (prog, args): (&str, Vec<&str>) = if cfg!(windows) {
        ("cmd.exe", vec!["/c", "echo hello-pty"])
    } else {
        ("/bin/sh", vec!["-c", "echo hello-pty"])
    };
    let (sess, mut child) = PtySession::spawn(prog, &args, 80, 24, None).expect("spawn 失败");
    let reader = sess.take_reader();
    let output = recv_contains(reader, &sess, "hello-pty");
    assert!(output.contains("hello-pty"));
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait());
    });
    let status = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("child.wait() 10 秒未返回(ConPTY wait 阻塞?)");
    let status = status.expect("wait 失败");
    assert_eq!(status.exit_code(), 0);
}

#[cfg(unix)]
#[test]
fn write_input_gets_echoed_by_cat() {
    // 交互回显:cat 把 stdin 原样吐回,验证 write_all → PTY → read 全链路
    let (sess, mut child) = PtySession::spawn("/bin/cat", &[], 80, 24, None).expect("spawn 失败");
    let reader = sess.take_reader();

    sess.write_all(b"marker-xyz-9876\n").expect("write 失败");
    recv_contains(reader, &sess, "marker-xyz-9876");

    // kill 后 wait 应能返回(子进程被信号杀死,退出码非 0 即可)
    sess.kill().expect("kill 失败");
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait());
    });
    let status = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("child.wait() 10 秒未返回(kill 后 wait 阻塞?)");
    let status = status.expect("kill 后 wait 失败");
    assert_ne!(status.exit_code(), 0);
}

#[test]
fn resize_does_not_error() {
    let (prog, args): (&str, Vec<&str>) = if cfg!(windows) {
        ("cmd.exe", vec!["/c", "exit", "0"])
    } else {
        ("/bin/sh", vec!["-c", "exit 0"])
    };
    let (sess, mut child) = PtySession::spawn(prog, &args, 80, 24, None).expect("spawn 失败");
    sess.resize(120, 40).expect("resize 失败");
    // wait 可能阻塞(Windows ConPTY 观测):清理性质,不阻塞测试线程;线程随进程退出
    std::thread::spawn(move || {
        let _ = child.wait();
    });
}
