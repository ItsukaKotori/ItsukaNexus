//! PTY 概念验证(spec 风险 #1 缓解措施):50 行独立小例,不进应用主线。
//! 在碰正式代码前,先用手感确认 portable-pty 的对象模型与阻塞读行为。
//! 运行:cargo run --example pty_echo
use std::io::{Read, Write};
use std::thread;

use portable_pty::{CommandBuilder, NativePtySystem, PtySize, PtySystem};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let pty_system = NativePtySystem::default();

    // 80x24 是 xterm 的经典默认尺寸;pixel 尺寸填 0 表示未知
    let pair = pty_system.openpty(PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    })?;

    // 受控命令而非交互 shell:跑完就退出,example 可自动结束
    let mut cmd = CommandBuilder::new("/bin/sh");
    cmd.args([
        "-c",
        r#"echo hello-from-pty; printf '\x1b[31mred-text\x1b[0m\n'; sleep 1"#,
    ]);
    cmd.env("TERM", "xterm-256color");

    let mut child = pair.slave.spawn_command(cmd)?;
    let mut reader = pair.master.try_clone_reader()?;
    let mut writer = pair.master.take_writer()?;

    // 写进去的东西 sh 不会回显(非交互),但 sleep 期间可以验证 write 不阻塞
    writer.write_all(b"# this line goes nowhere visible\n")?;
    writer.flush()?;

    // 读线程:读到 EOF(子进程退出、slave 关闭)为止
    let handle = thread::spawn(move || {
        let mut buf = [0u8; 8192];
        let mut out = Vec::new();
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => out.extend_from_slice(&buf[..n]),
            }
        }
        out
    });

    let output = handle.join().map_err(|_| "reader 线程 panic")?;
    let text = String::from_utf8_lossy(&output);
    print!("{text}");
    assert!(text.contains("hello-from-pty"), "应包含 echo 输出");
    assert!(text.contains("\x1b[31m"), "应包含 ANSI 颜色转义序列");

    let status = child.wait()?;
    println!("exit code: {}", status.exit_code());
    Ok(())
}
