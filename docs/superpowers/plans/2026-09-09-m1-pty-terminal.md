# ItsukaNexus M1(单终端:PTY 全链路)Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在应用内嵌一个真实终端(xterm.js ↔ PTY ↔ 默认 shell),打通 `session_create/send_input/resize/stop` 四个 IPC 命令,输出经 reader 线程 → 有界通道 → 合帧线程后 emit,满足 M1 五项完成标准。

**Architecture:** 同步线程实现(spec 设计原则 #1:先线程后 async,M2 才重构 tokio)。Rust 侧新增 `pty/`(portable-pty 封装 + 合帧)与 `agent/`(会话注册表 + 线程编排)两个领域模块,按未来 nexus-core 的布局摆放;`SessionManager` 通过注入的事件 sink 回调输出(而非直接持有 AppHandle),使编排逻辑可用纯 `cargo test` 集成测试覆盖——这也是 M2 EventBus 的前身。前端新增 `features/terminal/`(xterm.js 挂载)并扩展 `src/ipc/` 封装层(commands + events)。

**Tech Stack:** portable-pty 0.9、thiserror 2、uuid 1、@xterm/xterm 5 + @xterm/addon-fit、std::thread + std::sync::mpsc。

**Spec:** `docs/superpowers/specs/2026-09-09-itsukanexus-mvp-design.md`(M1 章节 + §1.4 IPC 命令清单 + §1.3 背压链 + §1.5 xterm 集成要点)

## Global Constraints

- 开发机:macOS(Apple Silicon),Rust 1.95、node 22、pnpm 12.3.4 已装好(无需 PATH 处理);跨平台回归靠 GitHub Actions 三平台矩阵(`.github/workflows/ci.yml` 已存在,推送自动跑)
- 分支:在 `m1-pty-terminal` 特性分支上实现,禁止直接提交 main
- 提交规范:每任务一次提交,信息结尾 `Co-Authored-By: Claude Code <noreply@anthropic.com>`
- 包管理器只用 **pnpm**(禁 npm/yarn install)
- 质量门槛(每任务收尾必须过):`cargo fmt` 已应用、`cargo clippy --all-targets -- -D warnings` 零警告、`cargo test` 全绿;涉及前端时 `pnpm build` 全绿
- 命名统一:模块 `pty`/`agent`;IPC 命令 snake_case(`session_create` 等);事件 `session://output`、`session://exit`(载荷 serde camelCase)
- **M1 禁止引入 tokio/async**(spec 原则 #1);UTF-8 解码用 `from_utf8_lossy`(已知跨 chunk 多字节缺陷是 M2 的活,spec 明示)
- 环境注入:每个 PTY 会话设 `TERM=xterm-256color`、`COLORTERM=truecolor`
- 参数定值(spec §1.4):读 chunk 8KB;合帧窗口 16ms、单帧上限 32KB;有界队列 64 帧

---

### Task 1: PTY 概念验证(example)+ 新依赖

**Files:**
- Create: `src-tauri/examples/pty_echo.rs`
- Modify: `src-tauri/Cargo.toml`(dev 无需,主依赖加 portable-pty / thiserror / uuid)

**Interfaces:**
- Consumes: —
- Produces: `portable-pty = "0.9"`、`thiserror = "2"`、`uuid = { version = "1", features = ["v4", "serde"] }` 三个依赖(Task 2-4 使用);可运行的 `cargo run --example pty_echo`

**学习点(写给执行者):** portable-pty 的核心对象模型——`PtySystem`(工厂)→ `openpty(PtySize)` 得 `PtyPair { master, slave }`;`slave.spawn_command(CommandBuilder)` 得 `Box<dyn Child>`;`master.take_writer()` 写、`master.try_clone_reader()` 读。**slave 在 spawn 后即被消耗**(它的使命就是孵化进程),读写都走 master。这是与 `std::process::Command` + pipe 最大的形态差异:PTY 的读写端是终端设备,不是裸管道。

- [ ] **Step 1: 加依赖**

`src-tauri/Cargo.toml` 的 `[dependencies]` 追加:

```toml
portable-pty = "0.9"
thiserror = "2"
uuid = { version = "1", features = ["v4", "serde"] }
```

- [ ] **Step 2: 写 example**

`src-tauri/examples/pty_echo.rs`:

```rust
//! PTY 概念验证(spec 风险 #1 缓解措施):50 行独立小例,不进应用主线。
//! 在碰正式代码前,先用手感确认 portable-pty 的对象模型与阻塞读行为。
//! 运行:cargo run --example pty_echo
use std::io::{Read, Write};
use std::thread;
use std::time::Duration;

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
    cmd.args(["-c", r#"echo hello-from-pty; printf '\x1b[31mred-text\x1b[0m\n'; sleep 1"#]);
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
```

- [ ] **Step 3: 运行验证**

```bash
cd /Users/itsuka/CodeSpace/ItsukaNexus/src-tauri && cargo run --example pty_echo
```

Expected: 打印 `hello-from-pty`、红色 `red-text`(终端里可见颜色)、`exit code: 0`,进程正常结束。

- [ ] **Step 4: 质量门槛 + 提交**

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings
cd /Users/itsuka/CodeSpace/ItsukaNexus && git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/examples && git commit -m "feat(m1): portable-pty 依赖与 PTY 概念验证 example

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 2: `error.rs` + `ids.rs` + `pty/session.rs`(TDD)

**Files:**
- Create: `src-tauri/src/error.rs`
- Create: `src-tauri/src/ids.rs`
- Create: `src-tauri/src/pty/mod.rs`
- Create: `src-tauri/src/pty/session.rs`
- Create: `src-tauri/tests/pty_session.rs`
- Modify: `src-tauri/src/lib.rs`(模块声明)

**Interfaces:**
- Consumes: Task 1 的依赖
- Produces:
  - `NexusError`(`Pty(#[from] io::Error)` / `SessionNotFound(String)` / `UnsupportedProvider(String)`)
  - `SessionId`(uuid newtype,`SessionId::new()`,Serialize 为字符串,Display)
  - `PtySession::spawn(session_id: SessionId, program: &str, args: &[&str], cols: u16, rows: u16) -> Result<(PtySession, Box<dyn Child + Send>), NexusError>`——**返回二元组**:Child 单独交出去给 wait 线程独占(见学习点)
  - `PtySession::{take_reader, write_all, resize, kill}(&self)`(全部 `&self`,可放进 `Mutex<HashMap>` 直接用)

**学习点(写给执行者):** ① `Box<dyn Child>::wait(&mut self)` 需要 `&mut`,而 `kill` 语义上谁都能调——portable-pty 用 `child.clone_killer()` 解决:spawn 后立刻克隆出 `Box<dyn ChildKiller + Send + Sync>` 留在会话里,`Box<dyn Child>` move 给 wait 线程独占。这是"所有权即并发设计"的第一课。② `take_writer()` 拿到的 `Box<dyn Write + Send>` 要被 IPC 命令线程和会话本身共享,所以包 `Arc<Mutex<…>>`;而 reader 天然单线程独占,不需要锁。③ 集成测试只能测 lib crate(`itsukanexus_lib`),这就是 `pub mod` 的原因。

- [ ] **Step 1: 写失败测试**

`src-tauri/tests/pty_session.rs`:

```rust
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
    let (sess, mut child) = PtySession::spawn(
        SessionId::new(),
        "/bin/sh",
        &["-c", "exit 0"],
        80,
        24,
    )
    .expect("spawn 失败");
    sess.resize(120, 40).expect("resize 失败");
    let _ = child.wait();
}
```

注意:`resize_does_not_error` 与 unix 分支里 `/bin/sh` 在 Windows CI 不执行(`#[cfg(unix)]`);`spawn_run_and_exit_oneshot` 在 Windows 走 `cmd.exe` 分支。

- [ ] **Step 2: 运行验证失败(红)**

```bash
cd /Users/itsuka/CodeSpace/ItsukaNexus/src-tauri && cargo test --test pty_session
```

Expected: 编译错误 `unresolved module ids`(红=测试有效)。

- [ ] **Step 3: 最小实现**

`src-tauri/src/error.rs`:

```rust
// 领域错误:M1 只需要三个变体;随里程碑推进再分层(spec M4 提到 Config/Git/Pty/Spawn 分层)
use thiserror::Error;

#[derive(Debug, Error)]
pub enum NexusError {
    /// PTY/IO 层错误(spawn 失败、write 失败等,std::io::Error 自动转换)
    #[error("PTY 错误: {0}")]
    Pty(#[from] std::io::Error),
    /// 会话 id 查不到(已停止或从未创建)
    #[error("会话不存在: {0}")]
    SessionNotFound(String),
    /// provider_id 不被支持(M1 只认 "shell")
    #[error("不支持的 provider: {0}")]
    UnsupportedProvider(String),
}
```

`src-tauri/src/ids.rs`:

```rust
// newtype 防字符串滥用(spec M3 学习主题的提前落地):
// SessionId 只能通过 new() 诞生,不可能手写一个假的。
use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(uuid::Uuid);

impl SessionId {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4())
    }
}

impl Default for SessionId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}
```

`src-tauri/src/pty/mod.rs`:

```rust
// PTY 子系统:对 portable-pty 的封装。
// 模块边界即未来 crate 边界(spec §1.2 布局):M3 拆分时整体搬入 nexus-core。
pub mod session;
```

`src-tauri/src/pty/session.rs`:

```rust
// PtySession:一个受管 PTY 会话的读写/控制面。
// 并发模型(M1,同步线程):
//   - reader:单线程独占(take_reader 交出去,本结构不再持有)
//   - writer:Arc<Mutex<>> 共享(IPC 命令线程写,会话表持有)
//   - child:spawn 后立刻拆走——wait 独占给 wait 线程,kill 用 clone_killer 留在这
use std::io::Write;
use std::sync::{Arc, Mutex};

use portable_pty::{Child, ChildKiller, CommandBuilder, Master, NativePtySystem, PtySize, PtySystem};

use crate::error::NexusError;
use crate::ids::SessionId;

pub struct PtySession {
    #[allow(dead_code)] // M1 尚未读 session_id;manager(Task 4)会用
    session_id: SessionId,
    master: Box<dyn Master + Send>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    killer: Box<dyn ChildKiller + Send + Sync>,
}

impl PtySession {
    /// spawn 一个跑在 PTY 里的进程。
    /// 返回 (会话句柄, child):child 必须立刻交给专属线程调 wait(),
    /// 否则进程退出后无人收割(Windows 上即"僵尸句柄",spec M1 完成标准③)。
    pub fn spawn(
        session_id: SessionId,
        program: &str,
        args: &[&str],
        cols: u16,
        rows: u16,
    ) -> Result<(Self, Box<dyn Child + Send>), NexusError> {
        let pty_system = NativePtySystem::default();
        let pair = pty_system.openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        let mut cmd = CommandBuilder::new(program);
        cmd.args(args);
        // 颜色/终端能力注入(spec §1.3):让 TUI 程序输出真彩 ANSI
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");

        let mut child = pair.slave.spawn_command(cmd)?;
        let writer = pair.master.take_writer()?;
        let killer = child.clone_killer();

        let sess = Self {
            session_id,
            master: pair.master,
            writer: Arc::new(Mutex::new(writer)),
            killer,
        };
        Ok((sess, child))
    }

    /// 取走 reader 的克隆(master 允许多次 clone reader;M1 只需要一个)。
    /// 拿到它的线程独占进行阻塞读,直到 EOF/错误。
    pub fn take_reader(&self) -> Box<dyn std::io::Read + Send> {
        self.master
            .try_clone_reader()
            .expect("master 存活期内 clone reader 不会失败")
    }

    pub fn write_all(&self, bytes: &[u8]) -> Result<(), NexusError> {
        let mut w = self.writer.lock().map_err(|e| {
            NexusError::Pty(std::io::Error::other(format!("writer 被毒化: {e}")))
        })?;
        w.write_all(bytes)?;
        w.flush()?;
        Ok(())
    }

    pub fn resize(&self, cols: u16, rows: u16) -> Result<(), NexusError> {
        self.master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        Ok(())
    }

    /// 强杀子进程。M1 的 stop 一律走这里;
    /// force=false 的优雅关停(先 Ctrl-C、再等待、最后 kill)是 M2 CancellationToken 的活。
    pub fn kill(&self) -> Result<(), NexusError> {
        self.killer.kill()?;
        Ok(())
    }
}
```

`src-tauri/src/lib.rs` 顶部模块声明区(app.rs 声明旁)追加:

```rust
pub mod error;
pub mod ids;
pub mod pty;
```

- [ ] **Step 4: 运行验证通过(绿)**

```bash
cd /Users/itsuka/CodeSpace/ItsukaNexus/src-tauri && cargo test
```

Expected: `pty_session` 三个测试(Windows CI 上两个)全绿,原有 `app_info` 不受影响。

- [ ] **Step 5: fmt + clippy 门槛**

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings
```

Expected: 零警告。(若报 unused import,删除之。)

- [ ] **Step 6: 提交**

```bash
cd /Users/itsuka/CodeSpace/ItsukaNexus && git add src-tauri/src src-tauri/tests && git commit -m "feat(m1): NexusError/SessionId 与 PtySession 封装(含跨平台集成测试)

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 3: `pty/batcher.rs` 合帧纯逻辑(TDD)

**Files:**
- Create: `src-tauri/src/pty/batcher.rs`
- Modify: `src-tauri/src/pty/mod.rs`(加 `pub mod batcher;`)

**Interfaces:**
- Consumes: —(纯函数,不依赖 Task 2)
- Produces: `next_frame(rx: &std::sync::mpsc::Receiver<Vec<u8>>, window: Duration, max_bytes: usize) -> Option<Vec<u8>>`;常量 `FRAME_WINDOW: Duration`(16ms)、`FRAME_MAX_BYTES: usize`(32 * 1024)。Task 4 的 batcher 线程在循环里调它,`None` 即通道关闭、线程退出。

**学习点(写给执行者):** 合帧是"把字节雨变成 UI 可消化的帧"的关键(spec §1.3 背压链中间级):`recv()` 阻塞等第一块(无数据时线程安静睡眠,不占 CPU),第一块到手后开时间窗,窗口内 `recv_timeout` 尽量多收,收满 `max_bytes` 或窗口到期就交出一帧。函数不拥有线程、不碰 Tauri——所以可以用单元测试把时间行为钉死。

- [ ] **Step 1: 写失败测试**

在 `src-tauri/src/pty/batcher.rs` 底部写 `#[cfg(test)] mod tests`(本模块测试放单元测试,因为要测的正是模块内部时间行为):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn coalesces_rapid_chunks_into_one_frame() {
        let (tx, rx) = mpsc::sync_channel::<Vec<u8>>(64);
        for i in 0..3 {
            tx.send(format!("chunk-{i}").into_bytes()).unwrap();
        }
        drop(tx); // 发完了
        let frame = next_frame(&rx, Duration::from_millis(16), 32 * 1024).unwrap();
        let text = String::from_utf8_lossy(&frame).into_owned();
        assert_eq!(text, "chunk-0chunk-1chunk-2");
    }

    #[test]
    fn returns_none_when_channel_closed_and_empty() {
        let (tx, rx) = mpsc::sync_channel::<Vec<u8>>(64);
        drop(tx);
        assert!(next_frame(&rx, Duration::from_millis(16), 32 * 1024).is_none());
    }

    #[test]
    fn stops_at_max_bytes() {
        let (tx, rx) = mpsc::sync_channel::<Vec<u8>>(64);
        let big = vec![b'x'; 40 * 1024]; // 40KB,单块即超 32KB 上限
        tx.send(big).unwrap();
        drop(tx);
        let frame = next_frame(&rx, Duration::from_millis(16), 32 * 1024).unwrap();
        assert!(frame.len() >= 40 * 1024, "超限块不截断数据(帧可略超上限,保流完整性)");
    }

    #[test]
    fn slow_producer_expires_window_with_what_it_has() {
        let (tx, rx) = mpsc::sync_channel::<Vec<u8>>(64);
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(50)); // 比窗口慢
            let _ = tx.send(b"late".to_vec());
        });
        let start = std::time::Instant::now();
        // 先塞一块"开场"数据,让 next_frame 进入窗口期
        let (tx2, rx2) = mpsc::sync_channel::<Vec<u8>>(64);
        tx2.send(b"first".to_vec()).unwrap();
        drop(tx2);
        let frame = next_frame(&rx2, Duration::from_millis(10), 32 * 1024).unwrap();
        assert_eq!(frame, b"first".to_vec());
        assert!(start.elapsed() < Duration::from_millis(45), "窗口到期要立刻返回,不能陪慢生产者等");
        drop(tx);
    }
}
```

- [ ] **Step 2: 运行验证失败(红)**

```bash
cd /Users/itsuka/CodeSpace/ItsukaNexus/src-tauri && cargo test --lib pty
```

Expected: 编译错误 `cannot find function next_frame`(红=测试有效)。

- [ ] **Step 3: 最小实现**

`src-tauri/src/pty/batcher.rs`(测试模块之外的部分):

```rust
// 合帧:reader 线程的 8KB 原始块 → UI 可消化的帧(spec §1.3 背压链中间级)。
// 纯函数设计:不拥有线程、不碰 Tauri,时间行为可被单元测试钉死。
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

/// 时间窗:第一块到手后,窗口内的后续块并进同一帧
pub const FRAME_WINDOW: Duration = Duration::from_millis(16);
/// 单帧大小上限:达到即交帧(帧可略超——数据流不可截断,见测试 stops_at_max_bytes)
pub const FRAME_MAX_BYTES: usize = 32 * 1024;

/// 从通道取一帧。阻塞等第一块;窗口内继续并块;通道关闭且无数据 → None(调用方线程退出)。
pub fn next_frame(
    rx: &Receiver<Vec<u8>>,
    window: Duration,
    max_bytes: usize,
) -> Option<Vec<u8>> {
    let mut frame = rx.recv().ok()?;
    let deadline = Instant::now() + window;
    loop {
        if frame.len() >= max_bytes {
            break;
        }
        let now = Instant::now();
        if now >= deadline {
            break;
        }
        match rx.recv_timeout(deadline - now) {
            Ok(chunk) => frame.extend_from_slice(&chunk),
            Err(_) => break, // 超时或通道关闭:把手头的交出去
        }
    }
    Some(frame)
}
```

`src-tauri/src/pty/mod.rs` 追加:

```rust
pub mod batcher;
```

- [ ] **Step 4: 运行验证通过(绿)**

```bash
cargo test --lib pty && cargo test
```

Expected: batcher 四个测试 + 全部既有测试绿。

- [ ] **Step 5: fmt + clippy + 提交**

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings
cd /Users/itsuka/CodeSpace/ItsukaNexus && git add src-tauri/src && git commit -m "feat(m1): 输出合帧器(16ms 时间窗 + 32KB 上限,单元测试覆盖)

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 4: `agent/manager.rs` 会话管理与线程编排(TDD)

**Files:**
- Create: `src-tauri/src/agent/mod.rs`
- Create: `src-tauri/src/agent/manager.rs`
- Create: `src-tauri/tests/session_manager.rs`
- Modify: `src-tauri/src/lib.rs`(加 `pub mod agent;`)

**Interfaces:**
- Consumes: Task 2 的 `PtySession`/`SessionId`/`NexusError`;Task 3 的 `next_frame`/`FRAME_WINDOW`/`FRAME_MAX_BYTES`
- Produces:
  - `SessionEvent` enum:`Output { id: SessionId, data: String }`、`Exit { id: SessionId, code: i32 }`(serde camelCase,Task 5 的 IPC 层直接 emit 它)
  - `EventSink = Arc<dyn Fn(SessionEvent) + Send + Sync>`(类型别名)
  - `SessionManager::new(sink: EventSink) -> Self`
  - `SessionManager::{create(provider_id: &str, cols: u16, rows: u16) -> Result<SessionId, NexusError>`(M1 只认 `"shell"`)、`send_input(&self, id: SessionId, data: &str)`、`resize(&self, id, cols, rows)`、`stop(&self, id)`——全部 `&self`,内部 `Mutex<HashMap>`(spec §3 "注册表锁 + 会话内部状态自有归属"的 M1 版)

**学习点(写给执行者):** ① **事件缝**:manager 不持有 AppHandle,而是构造时注入 `EventSink` 闭包——测试塞 mpsc,IPC 层(Task 5)塞 `app.emit(...)`。这让它可以纯 `cargo test` 集成测试,也正是 M2 EventBus 重构的接缝(spec §1.3)。② **线程拓扑**(每会话三线程):reader(8KB 循环读 → `sync_channel(64)` 有界——队列满则 `send` 阻塞,背压传导到 PTY 内核缓冲,子进程 write 变慢,这是 spec 设计的"不丢帧"背压链)→ batcher(调 `next_frame`,lossy 转 String,sink Output)→ 以及 wait 线程(独占 child,`wait()` 收割后 join batcher 线程再 sink Exit——保证退出事件不会抢在最后几帧输出之前)。③ 线程退出不用显式通知:kill/进程退出 → master read 得到 EOF/Err → reader 线程退 → `tx` drop → `next_frame` 返 None → batcher 退。所有权链即生命周期。

- [ ] **Step 1: 写失败测试**

`src-tauri/tests/session_manager.rs`:

```rust
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
    mgr.send_input(id, "echo marker-manager-42\n").expect("send 失败");
    wait_output_contains(&rx, id, "marker-manager-42");
}

#[test]
fn stop_kills_session_and_emits_exit() {
    let (mgr, rx) = manager_with_channel();
    let id = mgr.create("shell", 80, 24).expect("create 失败");
    mgr.stop(id);

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
    assert!(matches!(err, itsukanexus_lib::error::NexusError::SessionNotFound(_)));
}

#[test]
fn create_rejects_unknown_provider() {
    let (mgr, _rx) = manager_with_channel();
    let err = mgr.create("claude", 80, 24).unwrap_err();
    assert!(matches!(err, itsukanexus_lib::error::NexusError::UnsupportedProvider(_)));
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
    mgr.stop(a);
    mgr.stop(b);
}
```

- [ ] **Step 2: 运行验证失败(红)**

```bash
cd /Users/itsuka/CodeSpace/ItsukaNexus/src-tauri && cargo test --test session_manager
```

Expected: 编译错误 `unresolved module agent`(红=测试有效)。

- [ ] **Step 3: 最小实现**

`src-tauri/src/agent/mod.rs`:

```rust
// agent 子系统:M1 只有最小会话编排;provider 注册表/状态机是 M4 的形态。
pub mod manager;
```

`src-tauri/src/agent/manager.rs`:

```rust
// SessionManager:会话注册表 + 每会话三线程编排(reader/batcher/wait)。
// 事件缝:所有对外通知走构造注入的 EventSink,manager 不知道 tauri 的存在
// (spec §1.3:nexus-core 不依赖 tauri;M2 这里换成 EventBus)。
use std::collections::HashMap;
use std::io::Read;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use serde::Serialize;

use crate::error::NexusError;
use crate::ids::SessionId;
use crate::pty::batcher::{next_frame, FRAME_MAX_BYTES, FRAME_WINDOW};
use crate::pty::session::PtySession;

/// 读线程每次 read 的缓冲大小(spec §1.4)
const READ_CHUNK: usize = 8 * 1024;
/// 有界队列深度(spec §1.4):满则 reader 阻塞 = 背压
const QUEUE_DEPTH: usize = 64;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum SessionEvent {
    Output { id: SessionId, data: String },
    Exit { id: SessionId, code: i32 },
}

pub type EventSink = Arc<dyn Fn(SessionEvent) + Send + Sync>;

pub struct SessionManager {
    sink: EventSink,
    sessions: Mutex<HashMap<SessionId, PtySession>>,
}

/// 默认 shell:优先 $SHELL,逐级 fallback。
/// M4 由 AgentProfile/配置驱动,此函数退化为兜底默认值。
pub fn default_shell() -> String {
    if cfg!(windows) {
        "powershell.exe".into()
    } else {
        std::env::var("SHELL")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "/bin/zsh".into())
    }
}

impl SessionManager {
    pub fn new(sink: EventSink) -> Self {
        Self {
            sink,
            sessions: Mutex::new(HashMap::new()),
        }
    }

    pub fn create(&self, provider_id: &str, cols: u16, rows: u16) -> Result<SessionId, NexusError> {
        if provider_id != "shell" {
            return Err(NexusError::UnsupportedProvider(provider_id.to_string()));
        }
        let id = SessionId::new();
        let shell = default_shell();
        let program: String = shell;
        let (session, child) = PtySession::spawn(id, &program, &[], cols, rows)?;

        let reader = session.take_reader();
        let (tx, rx) = mpsc::sync_channel::<Vec<u8>>(QUEUE_DEPTH);

        // 线程 1/3:reader——阻塞读 master,8KB 一块塞进有界队列
        std::thread::spawn(move || {
            let mut reader = reader;
            let mut buf = vec![0u8; READ_CHUNK];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break, // EOF 或设备错误:会话输出结束
                    Ok(n) => {
                        if tx.send(buf[..n].to_vec()).is_err() {
                            break; // batcher 已退出,无人消费
                        }
                    }
                }
            }
        }); // tx 在此 drop → 通道关闭 → batcher 的 next_frame 返 None

        // 线程 2/3:batcher——合帧后转 String 经 sink 通知
        let sink_output = self.sink.clone();
        let batcher: JoinHandle<()> = std::thread::spawn(move || {
            while let Some(frame) = next_frame(&rx, FRAME_WINDOW, FRAME_MAX_BYTES) {
                // M1 临时方案(spec 明示):lossy 解码,跨 chunk 多字节字符可能出替换符,M2 换增量解码器
                let data = String::from_utf8_lossy(&frame).into_owned();
                sink_output(SessionEvent::Output { id, data });
            }
        });

        // 线程 3/3:wait——收割子进程;先 join batcher 再发 Exit,
        // 保证退出事件不抢在最后几帧输出之前
        let sink_exit = self.sink.clone();
        std::thread::spawn(move || {
            let code = match child.wait() {
                Ok(status) => status.exit_code(),
                Err(_) => -1,
            };
            let _ = batcher.join();
            sink_exit(SessionEvent::Exit { id, code });
        });

        self.sessions
            .lock()
            .expect("会话表锁被毒化")
            .insert(id, session);
        Ok(id)
    }

    pub fn send_input(&self, id: SessionId, data: &str) -> Result<(), NexusError> {
        self.lookup(id)?.write_all(data.as_bytes())
    }

    pub fn resize(&self, id: SessionId, cols: u16, rows: u16) -> Result<(), NexusError> {
        self.lookup(id)?.resize(cols, rows)
    }

    /// 停止会话:M1 一律强杀(killer.kill)。
    /// force=false 的优雅关停(先发 Ctrl-C、宽限、再杀)留给 M2 的 CancellationToken。
    pub fn stop(&self, id: SessionId) -> Result<(), NexusError> {
        let session = self
            .sessions
            .lock()
            .expect("会话表锁被毒化")
            .remove(&id);
        match session {
            Some(s) => s.kill(),
            None => Err(NexusError::SessionNotFound(id.to_string())),
        }
    }

    fn lookup(&self, id: SessionId) -> Result<std::sync::MutexGuard<'_, HashMap<SessionId, PtySession>>, NexusError> {
        let guard = self.sessions.lock().expect("会话表锁被毒化");
        if guard.contains_key(&id) {
            Ok(guard)
        } else {
            Err(NexusError::SessionNotFound(id.to_string()))
        }
    }
}
```

实现注意:
- `lookup` 返回 `MutexGuard` 后 `send_input` 里链式调用会撞借用检查(临时 guard 生命周期),如编译报错,拆成两行:`let guard = self.lookup(id)?; guard.write_all(...)`;`resize` 同理。
- 若 clippy 报 `PtySession` 未导出 Debug 之类的边角,以最小改动修(如给 `SessionEvent` 已有 Debug,`PtySession` 不需要)。

`src-tauri/src/lib.rs` 模块声明追加:

```rust
pub mod agent;
```

- [ ] **Step 4: 运行验证通过(绿)**

```bash
cargo test
```

Expected: `session_manager` 四个测试全绿;CI Windows 上整个文件被 `#![cfg(unix)]` 跳过(显示 0 tests,不算失败)。

- [ ] **Step 5: fmt + clippy + 提交**

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings
cd /Users/itsuka/CodeSpace/ItsukaNexus && git add src-tauri/src src-tauri/tests && git commit -m "feat(m1): SessionManager——注册表 + reader/batcher/wait 三线程编排(事件缝可测)

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 5: IPC 命令注册(lib.rs 薄封装)

**Files:**
- Modify: `src-tauri/src/lib.rs`

**Interfaces:**
- Consumes: Task 4 的 `SessionManager`/`SessionEvent`/`EventSink`
- Produces(前端可 invoke,参数 camelCase 自动映射):
  - `session_create(providerId: String, cols: Option<u16>, rows: Option<u16>) -> SessionCreated { sessionId, state }`
  - `session_send_input(sessionId: String, data: String) -> ()`
  - `session_resize(sessionId: String, cols: u16, rows: u16) -> ()`
  - `session_stop(sessionId: String, force: Option<bool>) -> ()`(M1 忽略 force,一律强杀)
  - 全局事件 `session://output`(载荷 `{ type: "output", id, data }`)与 `session://exit`(`{ type: "exit", id, code }`)——serde `tag = "type"` 的 enum 序列化形态;**注意 id 字段名是 `id`**(camelCase 化后不变),前端 types.ts 与此对齐

说明:错误跨 IPC 只能变字符串,统一 `map_err(|e| e.to_string())`(spec M1 学习主题:领域错误 thiserror、IPC 边界 String)。

- [ ] **Step 1: 改造 lib.rs**

`src-tauri/src/lib.rs` 整体替换为:

```rust
// 领域模块声明:命令变多后演进为 commands/ 目录。
pub mod agent;
pub mod app;
pub mod error;
pub mod ids;
pub mod pty;

use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use agent::manager::{SessionEvent, SessionManager};
use ids::SessionId;

// ---------- app_info(M0)----------

#[tauri::command]
fn app_info() -> app::AppInfo {
    app::app_info()
}

// ---------- session_*(M1)----------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionCreated {
    session_id: SessionId,
    state: &'static str, // M1 固定 "running";M2 起换真正的 SessionState
}

#[tauri::command]
fn session_create(
    state: State<SessionManager>,
    provider_id: String,
    cols: Option<u16>,
    rows: Option<u16>,
) -> Result<SessionCreated, String> {
    let id = state
        .create(&provider_id, cols.unwrap_or(80), rows.unwrap_or(24))
        .map_err(|e| e.to_string())?;
    Ok(SessionCreated {
        session_id: id,
        state: "running",
    })
}

#[tauri::command]
fn session_send_input(
    state: State<SessionManager>,
    session_id: String,
    data: String,
) -> Result<(), String> {
    let id: SessionId = parse_id(session_id)?;
    state.send_input(id, &data).map_err(|e| e.to_string())
}

#[tauri::command]
fn session_resize(
    state: State<SessionManager>,
    session_id: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    let id: SessionId = parse_id(session_id)?;
    state.resize(id, cols, rows).map_err(|e| e.to_string())
}

#[tauri::command]
fn session_stop(
    state: State<SessionManager>,
    session_id: String,
    force: Option<bool>, // M1 忽略:一律强杀,M2 实现优雅关停
) -> Result<(), String> {
    let _ = force;
    let id: SessionId = parse_id(session_id)?;
    state.stop(id).map_err(|e| e.to_string())
}

fn parse_id(s: String) -> Result<SessionId, String> {
    s.parse::<uuid::Uuid>()
        .map(SessionId::from)
        .map_err(|e| format!("非法 session id {s:?}: {e}"))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // 事件缝的 IPC 端:把 SessionEvent 转 emit
            // (M2 重构为 EventBus 订阅;M1 一条闭包足够)
            let handle: AppHandle = app.handle().clone();
            let sink = Arc::new(move |ev: SessionEvent| {
                use SessionEvent::*;
                let (event, payload) = match ev {
                    Output { .. } => ("session://output", ev.clone()),
                    Exit { .. } => ("session://exit", ev),
                };
                let _ = handle.emit(event, payload);
            });
            app.manage(SessionManager::new(sink));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_info,
            session_create,
            session_send_input,
            session_resize,
            session_stop
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

同时给 `src-tauri/src/ids.rs` 的 `SessionId` 补 `From<uuid::Uuid>` 实现(放在 `impl SessionId` 块或文件尾部):

```rust
impl From<uuid::Uuid> for SessionId {
    fn from(u: uuid::Uuid) -> Self {
        Self(u)
    }
}
```

- [ ] **Step 2: 编译 + 全量回归**

```bash
cd /Users/itsuka/CodeSpace/ItsukaNexus/src-tauri && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt
```

Expected: 全绿零警告。(本任务是薄封装无新测试——逻辑都在 Task 4 测过,IPC 通不通由 Task 7 E2E 验证。)

- [ ] **Step 3: 提交**

```bash
cd /Users/itsuka/CodeSpace/ItsukaNexus && git add src-tauri/src && git commit -m "feat(m1): session_create/send_input/resize/stop 四个 IPC 命令 + 事件 emit

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 6: 前端——xterm 终端组件 + IPC 封装 + App 改造

**Files:**
- Modify: `src/ipc/types.ts`(加会话相关类型)
- Modify: `src/ipc/commands.ts`(加四个命令封装)
- Create: `src/ipc/events.ts`(所有 listen 的封装唯一入口)
- Create: `src/features/terminal/TerminalPane.tsx`
- Modify: `src/App.tsx`(整体替换)

**Interfaces:**
- Consumes: Task 5 的 IPC 命令与 `session://output`、`session://exit` 事件
- Produces:
  - `commands.ts`: `sessionCreate(providerId: string, cols: number, rows: number): Promise<SessionCreated>`、`sessionSendInput(sessionId: string, data: string): Promise<void>`、`sessionResize(sessionId, cols, rows): Promise<void>`、`sessionStop(sessionId: string): Promise<void>`
  - `events.ts`: `onSessionOutput(sessionId: string, cb: (data: string) => void): Promise<UnlistenFn>`、`onSessionExit(sessionId: string, cb: (code: number) => void): Promise<UnlistenFn>`(内部过滤本会话 id)
  - `TerminalPane.tsx`: 默认导出组件,props `{ sessionId: string | null; onReady: (cols: number, rows: number) => void; onExit: (code: number) => void }`

设计要点(spec §1.5):输出数据**不进 React state**,`onmessage` 直达 `term.write`;`ResizeObserver` + fit addon,resize 防抖 100ms;React StrictMode 下 effect 双执行,靠 cleanup 与 ref 守卫保证幂等;初始 PTY 尺寸由 fit 实测后经 `onReady` 上报,再 create(避免 80x24 默认导致 TUI 重排抖动,spec 风险 #2)。

- [ ] **Step 1: 装前端依赖**

```bash
cd /Users/itsuka/CodeSpace/ItsukaNexus && pnpm add @xterm/xterm @xterm/addon-fit
```

- [ ] **Step 2: 扩展 IPC 封装层**

`src/ipc/types.ts` 追加(保留既有 AppInfo):

```typescript
/** session_create 的返回(与 Rust 侧 SessionCreated 对齐,serde camelCase) */
export interface SessionCreated {
  sessionId: string;
  state: string;
}

/** session://output 事件载荷(agent::manager::SessionEvent::Output,serde tag=type) */
export interface SessionOutputEvent {
  type: "output";
  id: string;
  data: string;
}

/** session://exit 事件载荷 */
export interface SessionExitEvent {
  type: "exit";
  id: string;
  code: number;
}
```

`src/ipc/commands.ts` 追加(保留既有 getAppInfo):

```typescript
import { invoke } from "@tauri-apps/api/core";
import type { AppInfo, SessionCreated } from "./types";

/** 所有 Tauri invoke 的类型安全封装——前端唯一入口 */

export function getAppInfo(): Promise<AppInfo> {
  return invoke<AppInfo>("app_info");
}

export function sessionCreate(
  providerId: string,
  cols: number,
  rows: number
): Promise<SessionCreated> {
  return invoke<SessionCreated>("session_create", { providerId, cols, rows });
}

export function sessionSendInput(
  sessionId: string,
  data: string
): Promise<void> {
  return invoke<void>("session_send_input", { sessionId, data });
}

export function sessionResize(
  sessionId: string,
  cols: number,
  rows: number
): Promise<void> {
  return invoke<void>("session_resize", { sessionId, cols, rows });
}

export function sessionStop(sessionId: string): Promise<void> {
  return invoke<void>("session_stop", { sessionId, force: true });
}
```

(整体替换该文件内容,合并后的 import 如上。)

`src/ipc/events.ts` 新建:

```typescript
/** 所有 Tauri listen 的封装——前端唯一入口。
 *  每个封装:订阅全局事件 + 按会话 id 过滤 + 返回 unlisten。 */
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type { SessionExitEvent, SessionOutputEvent } from "./types";

export function onSessionOutput(
  sessionId: string,
  cb: (data: string) => void
): Promise<UnlistenFn> {
  return listen<SessionOutputEvent>("session://output", (e) => {
    if (e.payload.id === sessionId) cb(e.payload.data);
  });
}

export function onSessionExit(
  sessionId: string,
  cb: (code: number) => void
): Promise<UnlistenFn> {
  return listen<SessionExitEvent>("session://exit", (e) => {
    if (e.payload.id === sessionId) cb(e.payload.code);
  });
}
```

- [ ] **Step 3: TerminalPane 组件**

`src/features/terminal/TerminalPane.tsx`:

```tsx
// 终端面板:容器 div + xterm 实例生命周期 + 输入/resize 双向流。
// 性能红线(spec §1.5):输出数据不经 React state,事件回调直达 term.write。
import { useEffect, useRef } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";

import { sessionResize, sessionSendInput } from "../../ipc/commands";
import { onSessionExit, onSessionOutput } from "../../ipc/events";

interface Props {
  sessionId: string | null;
  /** fit 实测出初始尺寸后回调(父组件此时才 session_create) */
  onReady: (cols: number, rows: number) => void;
  /** 会话进程退出回调 */
  onExit: (code: number) => void;
}

const RESIZE_DEBOUNCE_MS = 100;

export default function TerminalPane({ sessionId, onReady, onExit }: Props) {
  const containerRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  // 当前会话 id 的 ref 镜像:输入/resize 回调要读"最新"值,避免闭包过期
  const sessionIdRef = useRef<string | null>(null);
  sessionIdRef.current = sessionId;

  // effect 1:终端实例与容器尺寸观察(挂载一次;StrictMode 双执行靠 cleanup 配对)
  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    const term = new Terminal({
      fontFamily: "Menlo, Monaco, 'Courier New', monospace",
      fontSize: 13,
      cursorBlink: true,
      scrollback: 5000,
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(container);
    fit.fit();
    termRef.current = term;

    // 初始尺寸上报 → 父组件 session_create(避免 80x24 默认的重排抖动)
    onReady(term.cols, term.rows);

    term.onData((data) => {
      const id = sessionIdRef.current;
      if (id) void sessionSendInput(id, data);
    });

    let resizeTimer: ReturnType<typeof setTimeout> | undefined;
    const ro = new ResizeObserver(() => {
      clearTimeout(resizeTimer);
      resizeTimer = setTimeout(() => {
        fit.fit();
        const id = sessionIdRef.current;
        if (id) void sessionResize(id, term.cols, term.rows);
      }, RESIZE_DEBOUNCE_MS);
    });
    ro.observe(container);

    return () => {
      clearTimeout(resizeTimer);
      ro.disconnect();
      term.dispose();
      termRef.current = null;
    };
    // onReady 故意不进依赖:仅挂载时上报一次
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // effect 2:会话输出/退出订阅(sessionId 变化时重订)
  useEffect(() => {
    if (!sessionId) return;
    let alive = true;
    let unOutput: (() => void) | undefined;
    let unExit: (() => void) | undefined;

    void onSessionOutput(sessionId, (data) => {
      termRef.current?.write(data);
    }).then((u) => {
      if (alive) unOutput = u;
      else u();
    });

    void onSessionExit(sessionId, (code) => {
      termRef.current?.writeln(
        `\x1b[90m[进程已退出,退出码 ${code}]\x1b[0m`
      );
      onExit(code);
    }).then((u) => {
      if (alive) unExit = u;
      else u();
    });

    return () => {
      alive = false;
      unOutput?.();
      unExit?.();
    };
    // onExit 故意不进依赖:行为只依赖挂载时的 props 语义
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sessionId]);

  return (
    <div
      ref={containerRef}
      style={{ width: "100%", height: "100%", minWidth: 0, minHeight: 0 }}
    />
  );
}
```

- [ ] **Step 4: 改写 App.tsx**

`src/App.tsx` 整体替换:

```tsx
// M1 UI:顶栏(标题 + 应用信息 + 新建/停止)+ 全屏单终端。
// M2 引入 zustand 与多 tab 后,这里瘦身成 AppShell 布局。
import { useCallback, useRef, useState } from "react";

import TerminalPane from "./features/terminal/TerminalPane";
import { getAppInfo, sessionCreate, sessionStop } from "./ipc/commands";
import type { AppInfo } from "./ipc/types";

function App() {
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [sessionId, setSessionId] = useState<string | null>(null);
  const [status, setStatus] = useState<string>("无会话");
  const [error, setError] = useState<string | null>(null);
  // StrictMode 下 effect 双执行会让 onReady 触发两次;ref 守卫保证只 create 一次
  const creatingRef = useRef(false);

  const handleTerminalReady = useCallback(
    async (cols: number, rows: number) => {
      if (creatingRef.current) return;
      creatingRef.current = true;
      try {
        setError(null);
        const created = await sessionCreate("shell", cols, rows);
        setSessionId(created.sessionId);
        setStatus("运行中");
        if (!info) {
          try {
            setInfo(await getAppInfo());
          } catch {
            /* 顶栏信息拿不到不影响终端 */
          }
        }
      } catch (e) {
        setError(String(e));
        setStatus("创建失败");
      } finally {
        creatingRef.current = false;
      }
    },
    [info]
  );

  const handleStop = useCallback(async () => {
    if (!sessionId) return;
    try {
      await sessionStop(sessionId);
    } catch (e) {
      setError(String(e));
    }
    setSessionId(null);
    setStatus("无会话");
  }, [sessionId]);

  const handleExit = useCallback(() => {
    setSessionId(null);
    setStatus("已退出");
  }, []);

  return (
    <main
      style={{
        display: "flex",
        flexDirection: "column",
        height: "100vh",
        fontFamily: "system-ui",
      }}
    >
      <header
        style={{
          display: "flex",
          alignItems: "center",
          gap: 12,
          padding: "8px 16px",
          borderBottom: "1px solid #ddd",
        }}
      >
        <strong>ItsukaNexus</strong>
        {info && (
          <span style={{ color: "#888", fontSize: 13 }}>
            v{info.version} · {info.platform}
          </span>
        )}
        <span style={{ flex: 1 }} />
        <span style={{ fontSize: 13 }}>{status}</span>
        <button onClick={handleStop} disabled={!sessionId}>
          停止
        </button>
      </header>
      <div style={{ flex: 1, minHeight: 0, padding: 4 }}>
        <TerminalPane
          sessionId={sessionId}
          onReady={handleTerminalReady}
          onExit={handleExit}
        />
      </div>
      {error && (
        <footer style={{ color: "red", padding: "4px 16px", fontSize: 13 }}>
          {error}
        </footer>
      )}
    </main>
  );
}

export default App;
```

- [ ] **Step 5: 类型检查与构建**

```bash
cd /Users/itsuka/CodeSpace/ItsukaNexus && pnpm build
```

Expected: `tsc && vite build` 全绿。(模板遗留的 `src/App.css`/`src/assets/react.svg` 不再被引用,tsc 不报错即不删——保持改动最小。)

- [ ] **Step 6: Rust 门槛回归 + 提交**

```bash
cd src-tauri && cargo clippy --all-targets -- -D warnings && cargo fmt --check
cd /Users/itsuka/CodeSpace/ItsukaNexus && git add src package.json pnpm-lock.yaml && git commit -m "feat(m1): 前端 xterm 终端组件与 session IPC/事件封装

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 7: M1 端到端验收(完成标准逐项)

**Files:**
- Modify: 无(验证任务;若暴露问题,修复后回归本清单并在计划里记一笔)

**Interfaces:**
- Consumes: Task 1-6 全部产出
- Produces: M1 五项完成标准全绿的证据

- [ ] **Step 1: 启动并做基本验收(标准①部分)**

```bash
cd /Users/itsuka/CodeSpace/ItsukaNexus && pnpm tauri dev
```

窗口出现即自动创建 shell 会话(顶栏"运行中")。在终端里逐项输入验证:

```bash
claude --version     # 或任何 CLI;不行就换 git --version
git log              # 分页器:按 q 退出,应无花屏
vim /etc/hosts       # 全屏 TUI:方向键、:q 退出,应无残影错位
printf '\x1b[31mred\x1b[0m\n'   # ANSI 颜色
exit                 # 自然退出:顶栏状态变"已退出",系统无残留 shell 进程(最终修复补了自动化测试)
```

Expected: 输入回显正常、颜色正常、分页器与 vim 渲染无花屏、scrollback 可滚动;`exit` 自然退出后状态变"已退出"且无残留进程。

- [ ] **Step 2: resize 验证(标准②)**

拖拽改变窗口大小。

Expected: 终端内容跟随重排(防抖 ~100ms 后),vim 内 `:set columns?` 应与视觉列数一致(可开 vim 观察 statusline 刷新)。

- [ ] **Step 3: 停止按钮验证(标准③)**

点"停止"按钮,另开系统终端:

```bash
pgrep -fl zsh | grep -v pgrep   # 或 powershell 时用任务管理器
```

Expected: 应用内显示"无会话"、`[进程已退出...]` 字样;系统里无残留 shell 进程;再点不到 5 秒。

- [ ] **Step 4: 大输出不卡(标准④)**

在应用终端里:

```bash
seq 1 20000 > /tmp/big.txt && cat /tmp/big.txt
```

Expected: UI 不冻结、不白屏;滚动回看 1..20000 完整;`echo done` 之后输入响应正常。(合帧生效的证据:cat 期间 UI 线程仍可交互。)

- [ ] **Step 5: 关闭无 panic(标准⑤)**

保持会话运行中,直接关闭应用窗口。

Expected: 进程退出,`tauri dev` 终端输出里无 `panicked at` 字样。

- [ ] **Step 6: 全量回归 + 收尾提交**

```bash
cd /Users/itsuka/CodeSpace/ItsukaNexus/src-tauri && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check
cd /Users/itsuka/CodeSpace/ItsukaNexus && pnpm build
git status --short
```

(`git status` 为空则跳过提交;有残留(如验收中临时改动)则酌情提交或还原后汇报。)

- [ ] **Step 7: 推送并确认 CI**

```bash
git push -u origin m1-pty-terminal
gh run watch   # 或 gh run list 轮询
```

> 注:CI 由 `pull_request` 触发(特性分支的 push 不触发,`push` 仅限 main),`gh run watch` 看不到运行;实操是建 PR 后用 `gh pr checks` 观察。

Expected: 三平台 Rust 矩阵 + 前端构建全绿(Windows 上 session_manager 测试显示 skipped/0 属预期,`#![cfg(unix)]`)。

---

## Self-Review 记录

- **Spec 覆盖**:M1 交付物四命令 → Task 5;`std::thread + mpsc + 16ms 合帧 + 全局 emit` → Task 3/4;`from_utf8_lossy` 临时方案 → Task 4(含注释指向 M2);xterm+fit → Task 6;TERM/COLORTERM 注入 → Task 2;完成标准①-⑤ → Task 7 Step 1-5。默认 shell 跨平台(macOS $SHELL/Windows powershell)→ Task 4 `default_shell()`。✅
- **有意的范围裁剪(非遗漏)**:背压链的有界队列 M1 即有(sync_channel 64),但"慢消费者观察"是 M2 完成标准②,不在本计划;replay/Channel/utf8 增量解码均 M2;多会话 UI(zustand/terminalManager)M2——M1 单会话组件即 spec M1 交付物原文。
- **占位符扫描**:无 TBD/TODO;全部代码步骤含完整代码;Task 4 已预判 `lookup` 借用检查问题并给出两行拆法。✅
- **类型一致性**:`SessionEvent` serde `rename_all=camelCase + tag="type"` → 序列化为 `{type, id, data}` / `{type, id, code}`,Task 6 `types.ts` 的 `SessionOutputEvent/SessionExitEvent` 字段 `type/id/data/code` 与之对齐;`SessionCreated.sessionId` ↔ TS `sessionId`;命令名 snake_case 五处对齐(`session_create` 等);`SessionId::from(uuid)` 在 Task 5 使用、Task 2 补齐定义——**注意 Task 2 的 ids.rs 代码块不含 From 实现,Task 5 Step 1 末尾单独给出,执行 Task 2 时可顺手一并写入**(放置位置已写明)。✅

---

## 完成记录(2026-09-10,PR #1 已合并)

全部 7 任务完成,最终全分支审查(With fixes)的三项 Important 已在合并前修复(视口 body margin / CI timeout-minutes / 自然退出 Exit{code:0} 测试),另修 Windows CI 两处 PTY 测试基础设施(阻塞读有界化、ConPTY DSR 代答)。CI 四检查全绿。

### M2 必办(执行 M1 时的裁定与遗留,按优先级)

1. **孙进程持 slave fd 时 Exit 永不发出 + 每会话泄漏 3 线程**(P7,必修):用户在终端跑 nohup/后台任务即可触发。修法:进程组 kill(Unix setsid/killpg)或 wait 线程 join 超时后带序外标记发 Exit。
2. **初始输出订阅竞态**(P8):create 返回前 PTY 已产输出,前端 listen 完成前的事件被丢。M2 store 化时改为"先订阅占位、后 create"。
3. **事件键名统一**:M1 载荷用 `id`,spec §1.4 写 `session_id`;M2 引入 `session://state` 时统一键名,建议 TS 类型从 Rust 生成。
4. **kill 结果按平台区分并接日志**:上游 portable-pty 0.9.0 `WinChildKiller::kill` 成败判定反转(成功返 Err),M1 以 Exit 事件为真相绕过;Unix 侧 kill 错误也被吞,M2 接 tauri-plugin-log 后按 `#[cfg(windows)]` 区分。
5. **send_input/resize 持整表锁跨写 I/O**:单会话 writer 阻塞会卡全部会话的 create/stop;M2 拆 per-session 句柄(actor 化即根治)。
6. **自然退出后注册表留死条目**(后续 send_input 得 Pty(EIO) 而非 SessionNotFound):wait 线程清表。
7. 其他小项:pty/decode.rs 增量 UTF-8 解码替换 from_utf8_lossy(spec 既定)、events.ts listen 加 .catch、主动停止后晚到 Exit 覆盖状态、xterm >500kB chunk 可选代码分割。

### Windows PTY 知识库(踩坑记录)

- ConPTY 启动期发 `ESC[6n`(DSR 光标查询),**无应答则不产出任何子进程输出**;真实应用 xterm.js 自动应答,裸测试需代答 `ESC[1;1R`(见 tests/pty_session.rs 的 recv_contains)。
- portable-pty 0.9 的 `Child::wait` 在 ConPTY 上可能长时间阻塞,测试中须有界化(辅助线程 + recv_timeout)。
- portable-pty 0.9 实际 API 与常见记忆差异:`Master`→`MasterPty`、`exit_code()` 返回 u32、`kill` 需经 `clone_killer()` 且 `&mut self`。
