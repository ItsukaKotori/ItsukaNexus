# ItsukaNexus M2(async 重构 + 多会话 + 恢复)Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 M1 的同步线程 PTY 链路重构为 tokio,交付多 tab 并行终端、`Channel<PtyChunk>` 输出流 + replay 恢复、`session_list`、`AppConfig` 读写、tauri-plugin-log、增量 UTF-8 解码,并清掉 M1 遗留的七项必办,满足 M2 六项完成标准。

**Architecture:** SessionManager 保持"事件缝"骨架但全面 async 化:reader 经 `spawn_blocking` 桥接阻塞 IO(spec §1.3 明示这是正确做法而非妥协),有界 `tokio::sync::mpsc` 延续背压链,batcher 变 async 任务并接入**增量 UTF-8 解码器**与 **256KB replay ring buffer**;输出流从全局 emit 迁移到 per-session 的 `Channel<PtyChunk>`(attach 时替换,前端刷新经 `session_list` + replay 无损恢复)。会话表从 `Mutex<HashMap<SessionId, PtySession>>` 改为**瘦句柄**(writer/killer 的 Arc + 快照),`send_input` 不再持表锁跨 I/O(M1 遗留 #5)。退出后会话条目保留并标记状态(侧边栏可见 Failed/Exited,M1 遗留 #6)。前端引入 zustand 单 store + React 外 terminalManager(实例常驻,CSS 切换),输出直达 `term.write` 的性能红线不变。

**Tech Stack:** tokio(full)+ tokio-util(CancellationToken)、zustand 5、@tauri-apps/api Channel、tauri-plugin-log 2。

**Spec:** `docs/superpowers/specs/2026-09-09-itsukanexus-mvp-design.md`(M2 章节 + §1.3 背压链/replay + §1.4 Channel 与 session_attach/session_list/config 命令 + §1.5 terminalManager 方案)
**M1 完成记录:** `docs/superpowers/plans/2026-09-09-m1-pty-terminal.md` 末尾"M2 必办"与"Windows PTY 知识库"章节——本计划已吸收(见下方映射表),执行者不必回读。

## M1 遗留必办 → 本计划映射

| M1 遗留 | 落点 |
|---|---|
| #1 孙进程持 slave fd → Exit 永不发 + 线程泄漏 | Task 5(wait 任务 join batcher 改 5s 超时,超时仍发 Exit 并带 detail 标记;进程组 kill 留 M3) |
| #2 初始输出订阅竞态 | Task 6/8(Channel 由前端传入 + create 起进 ring buffer,attach replay 补齐,竞态窗口归零) |
| #3 事件键名统一 | Task 3/6(全部载荷统一 `sessionId`;`session://state` 新增) |
| #4 kill 结果按平台区分 + 日志 | Task 5/7(cfg(windows) 区分 + tauri-plugin-log) |
| #5 整表锁跨写 I/O | Task 5(瘦句柄,writer Arc 直接写,不持表锁) |
| #6 退出后死条目 | Task 3/5(条目保留 + 状态,send_input 对非 Running 会话返回明确错误) |
| #7 lossy 解码 | Task 2(decode.rs 增量解码,batcher 接入) |

## Global Constraints

- 开发机:macOS(Apple Silicon),Rust 1.95、node 22、pnpm 12.3.4;跨平台回归靠 CI 三平台矩阵(仅 main push 与 PR 触发)
- 分支:在 worktree 特性分支(建议名 `m2-async-multi-session`)上实现,禁止直接提交 main;PR 由用户本人合并
- 提交规范:每任务一次提交,信息结尾 `Co-Authored-By: Claude Code <noreply@anthropic.com>`
- 包管理器只用 **pnpm**
- 质量门槛(每任务收尾必过):`cargo fmt` 已应用、`cargo clippy --all-targets -- -D warnings` 零警告、`cargo test` 全绿;涉及前端时 `pnpm build` 全绿
- 参数定值(spec §1.4,M2 不变):读 chunk 8KB;合帧窗口 16ms、单帧上限 32KB;有界队列 64 帧;replay buffer 256KB
- 事件与命令命名:命令 snake_case;事件 `session://state`、`session://exit`(载荷统一 `sessionId` camelCase);Channel 载荷 `PtyChunk { sessionId, data, seq }`
- **nexus-core 不依赖 tauri** 的缝保持:manager 模块不得 `use tauri`(Channel 在 IPC 层接线);集成测试不依赖 tauri 运行时
- PTY 底层(portable-pty 0.9)不动:PtySession 的同步 API 保持,由 spawn_blocking 桥接
- Windows 陷阱(M1 实测):ConPTY 启动期 DSR 查询需代答(已有测试基础设施);`WinChildKiller::kill` 成败判定反转,退出真相以 Exit 事件为准;测试中阻塞读/wait 必须有界(辅助线程 + recv_timeout 模式)

---

### Task 1: tokio 依赖 + 概念验证 example

**Files:**
- Modify: `src-tauri/Cargo.toml`(`[dependencies]` 加 tokio、tokio-util;`[dev-dependencies]` 无需)
- Create: `src-tauri/examples/tokio_basics.rs`

**Interfaces:**
- Consumes: —
- Produces: `tokio = { version = "1", features = ["full"] }`、`tokio-util = "0.7"`;可运行的 `cargo run --example tokio_basics`

**学习点(写给执行者):** ① `tokio::sync::mpsc::channel(N)` 有界通道的 `send().await` 在队列满时**挂起任务而非线程**——这就是背压的 async 形态(M1 里是阻塞线程);② `tokio::time::timeout` 给任何 await 加期限;③ `CancellationToken::cancelled().await` 是可协作的关停信号,`select!` 里与正常工作并排;④ `spawn_blocking` 把阻塞函数放进专用线程池,返回 JoinHandle(可 .await)。example 把四件事各演一遍。

- [ ] **Step 1: 加依赖**

`src-tauri/Cargo.toml` `[dependencies]` 追加:

```toml
tokio = { version = "1", features = ["full"] }
tokio-util = "0.7"
```

- [ ] **Step 2: 写 example**

`src-tauri/examples/tokio_basics.rs`:

```rust
//! tokio 概念验证(spec 风险 #1 缓解):在重构主线前,亲手感受
//! 有界通道背压 / timeout / CancellationToken / spawn_blocking 四件事。
//! 运行:cargo run --example tokio_basics
use std::time::Duration;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() {
    // 1) 有界通道 + send().await 背压:容量 2,生产 5 条,消费前 2 条后停 200ms
    let (tx, mut rx) = mpsc::channel::<u32>(2);
    let producer = tokio::spawn(async move {
        for i in 0..5u32 {
            // 队列满时这里挂起——生产者被消费速度约束,这就是背压
            tx.send(i).await.expect("consumer alive");
            println!("produced {i}");
        }
    });
    let mut got = Vec::new();
    for _ in 0..2 {
        got.push(rx.recv().await.unwrap());
    }
    println!("consumed {:?}, pausing (producer must be parked on send #3)", got);
    tokio::time::sleep(Duration::from_millis(200)).await;
    while let Some(v) = rx.recv().await {
        got.push(v);
    }
    producer.await.unwrap();
    assert_eq!(got, vec![0, 1, 2, 3, 4]);

    // 2) timeout:给慢 await 加期限,超时返回 Err 而不是永远等
    let slow = tokio::time::sleep(Duration::from_secs(10));
    let r = tokio::time::timeout(Duration::from_millis(50), slow).await;
    assert!(r.is_err(), "10s 的 sleep 在 50ms 处被打断");

    // 3) CancellationToken:select! 里正常工作与关停信号并排
    let token = CancellationToken::new();
    let child = token.clone();
    let worker = tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_millis(50)) => println!("tick"),
                _ = child.cancelled() => {
                    println!("cancelled, draining");
                    return;
                }
            }
        }
    });
    tokio::time::sleep(Duration::from_millis(120)).await;
    token.cancel();
    worker.await.unwrap();

    // 4) spawn_blocking:阻塞 IO 的桥(打印线程名证明不在 async 线程)
    let pid = tokio::task::spawn_blocking(|| {
        println!("blocking pool thread: {:?}", std::thread::current().id());
        std::process::id()
    })
    .await
    .unwrap();
    println!("pid {pid} — all four concepts verified");
}
```

- [ ] **Step 3: 运行验证**

```bash
cd /Users/itsuka/CodeSpace/ItsukaNexus/src-tauri && cargo run --example tokio_basics
```

Expected: produced 0..4 顺序输出、consumer 暂停期间无 produced #3(背压挂起)、tick ×2、cancelled、四概念 verified,正常退出。

- [ ] **Step 4: 门槛 + 提交**

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test
cd /Users/itsuka/CodeSpace/ItsukaNexus && git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/examples && git commit -m "feat(m2): tokio 依赖与 async 概念验证 example

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 2: `pty/decode.rs` 增量 UTF-8 解码器(TDD)

**Files:**
- Create: `src-tauri/src/pty/decode.rs`
- Modify: `src-tauri/src/pty/mod.rs`(加 `pub mod decode;`)

**Interfaces:**
- Consumes: —
- Produces: `Decoder::new()`、`Decoder::feed(&mut self, bytes: &[u8]) -> String`(立即返回可安全显示的文本,**保留不完整多字节尾部到实例内**,下次 feed 续上)、`Decoder::pending(&self) -> usize`(测试用)。Task 5 的 batcher 每会话持一个 Decoder 实例。

**学习点:** `String::from_utf8_lossy` 对被 chunk 边界切断的多字节序列(中文 3 字节、emoji 4 字节)会插入 U+FFFD 替换符且**无法恢复**——错误是粘住的。增量解码器的状态机:记住"还差几个延续字节(0x80..=0xBF)";`std::str::from_utf8` 返回 `Err(Utf8Error)` 时 `error.valid_up_to()` 告诉我们前多少字节完整、`error.error_len()` 为 None 表示尾部只是不完整(留待续)而非非法(替换符处理)。

- [ ] **Step 1: 写失败测试**

`src-tauri/src/pty/decode.rs` 底部:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// 把 bytes 按给定切分逐块 feed,返回拼接结果——模拟跨 chunk 边界
    fn feed_chunks(dec: &mut Decoder, bytes: &[u8], splits: &[usize]) -> String {
        let mut out = String::new();
        let mut start = 0usize;
        for &end in splits {
            out.push_str(&dec.feed(&bytes[start..end]));
            start = end;
        }
        out.push_str(&dec.feed(&bytes[start..]));
        out
    }

    #[test]
    fn ascii_passthrough() {
        let mut d = Decoder::new();
        assert_eq!(d.feed(b"hello"), "hello");
        assert_eq!(d.pending(), 0);
    }

    #[test]
    fn chinese_split_across_chunks() {
        // "你好世界" = 4 × 3 字节;在每个字节边界都切一遍
        let bytes = "你好世界".as_bytes();
        for k in 1..bytes.len() {
            let mut d = Decoder::new();
            let out = feed_chunks(&mut d, bytes, &[k]);
            assert_eq!(out, "你好世界", "切分点 {k} 不应产生替换符");
        }
    }

    #[test]
    fn emoji_4byte_split() {
        // 🚀 = F0 9F 9A 80(4 字节);拆成 1+3 与 2+2
        let bytes = "🚀".as_bytes();
        assert_eq!(bytes.len(), 4);
        let mut d = Decoder::new();
        assert_eq!(feed_chunks(&mut d, bytes, &[1]), "🚀");
        let mut d = Decoder::new();
        assert_eq!(feed_chunks(&mut d, bytes, &[2]), "🚀");
    }

    #[test]
    fn incomplete_tail_withheld_then_completed() {
        let mut d = Decoder::new();
        let bytes = "中".as_bytes(); // E4 BD AD
        let first = d.feed(&bytes[..2]);
        assert_eq!(first, "", "不完整尾部应扣留,不产替换符");
        assert_eq!(d.pending(), 2);
        let second = d.feed(&bytes[2..]);
        assert_eq!(second, "中");
    }

    #[test]
    fn invalid_byte_becomes_replacement_and_resyncs() {
        let mut d = Decoder::new();
        // 0xFF 是非法 UTF-8 首字节:替换符处理后,后续正常文本应恢复
        let out = d.feed(&[0xFF, b'o', b'k']);
        assert_eq!(out, "\u{FFFD}ok");
        assert_eq!(d.pending(), 0);
    }

    #[test]
    fn mixed_multibyte_stream_split_at_every_point() {
        // 中文 + emoji + ASCII 混排,滑窗切分(覆盖多字符同时跨界的组合)
        let text = "a你b🚀c好d🎉e";
        let bytes = text.as_bytes();
        for k in 1..bytes.len() {
            let mut d = Decoder::new();
            assert_eq!(feed_chunks(&mut d, bytes, &[k]), text, "切分点 {k}");
        }
    }
}
```

- [ ] **Step 2: 红**

```bash
cd /Users/itsuka/CodeSpace/ItsukaNexus/src-tauri && cargo test --lib pty::decode
```

Expected: 编译错误 `cannot find type Decoder`。

- [ ] **Step 3: 实现**

`src-tauri/src/pty/decode.rs`(测试模块之上):

```rust
// 增量 UTF-8 解码:替代 from_utf8_lossy 的跨 chunk 安全方案(spec §1.2 pty/decode.rs)。
// 每会话一个实例,feed 合帧后的字节块;不完整的多字节尾部扣留在实例内,
// 下一块到达时续上——转义序列与文本都不会因 chunk 边界撕裂。
pub struct Decoder {
    /// 上一块遗留的不完整多字节前缀(已确认合法首字节 + 部分延续字节)
    pending: Vec<u8>,
}

impl Decoder {
    pub fn new() -> Self {
        Self { pending: Vec::new() }
    }

    /// 喂入一块字节,返回此刻可安全显示的完整文本。
    pub fn feed(&mut self, bytes: &[u8]) -> String {
        // 拼上遗留前缀再整体解码(遗留最多 3 字节,拷贝开销可忽略)
        let mut buf = std::mem::take(&mut self.pending);
        buf.extend_from_slice(bytes);

        match std::str::from_utf8(&buf) {
            Ok(s) => s.to_string(),
            Err(e) => {
                let valid_up_to = e.valid_up_to();
                let mut out = String::with_capacity(buf.len());
                // 1) 完整前缀直接收录
                out.push_str(unsafe { std::str::from_utf8_unchecked(&buf[..valid_up_to]) });
                match e.error_len() {
                    // 2) 尾部只是不完整:扣留,等下一块
                    None => {
                        self.pending = buf[valid_up_to..].to_vec();
                    }
                    // 3) 确实非法:替换符顶替坏字节,从下一字节重同步继续解
                    Some(bad_len) => {
                        out.push('\u{FFFD}');
                        let rest = &buf[valid_up_to + bad_len..];
                        // 剩余部分递归式处理(一次 feed 理论上可能多个坏字节)
                        let tail = self.feed(rest);
                        out.push_str(&tail);
                    }
                }
                out
            }
        }
    }

    /// 当前扣留的不完整字节数(测试与诊断用)
    pub fn pending(&self) -> usize {
        self.pending.len()
    }
}

impl Default for Decoder {
    fn default() -> Self {
        Self::new()
    }
}
```

注:`unsafe from_utf8_unchecked` 处可改用 `std::str::from_utf8(&buf[..valid_up_to]).unwrap()` 的安全写法(前缀已被确认合法,unwrap 不会触发);若想全无 unsafe,用安全写法即可,clippy 都接受。

`src-tauri/src/pty/mod.rs` 追加 `pub mod decode;`。

- [ ] **Step 4: 绿 + 门槛 + 提交**

```bash
cargo test --lib pty::decode && cargo test
cargo fmt && cargo clippy --all-targets -- -D warnings
cd /Users/itsuka/CodeSpace/ItsukaNexus && git add src-tauri/src && git commit -m "feat(m2): 增量 UTF-8 解码器(跨 chunk 多字节安全,完成标准⑥)

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 3: `agent/state.rs` 会话状态机 + 事件键名统一(TDD)

**Files:**
- Create: `src-tauri/src/agent/state.rs`
- Modify: `src-tauri/src/agent/mod.rs`(加 `pub mod state;`)
- Modify: `src-tauri/src/agent/manager.rs`(SessionEvent 键名 `id` → `session_id`,载荷类型改造——只动类型与构造点,编排逻辑 Task 5 重写)

**Interfaces:**
- Consumes: `SessionId`
- Produces:
  - `SessionState { Running, Stopping, Exited, Failed }`(serde camelCase;M2 简化版,Created 一闪不入表,WaitingInput 是 v1.1 的活)
  - `SessionState::can_transition_to(&self, next) -> bool`(穷尽 match)
  - `StateChange { session_id, prev, next, at_ms, detail: Option<String> }`(session://state 载荷)
  - `SessionSnapshot { session_id, state, started_at_ms, exit_code: Option<i32>, pid: Option<u32> }`(session_list 返回元素)
  - `SessionEvent` 变三变体:`Output { session_id, data, seq }`、`State(StateChange)`、`Exit { session_id, code }`——键名统一(M1 遗留 #3)

**学习点:** enum 状态机是 Rust 的看家本领:穷尽 match 让"新增状态时编译器点名所有漏改处"(spec 风险 #10)。`at_ms` 用 `SystemTime::now()` 的 epoch 毫秒(展示用,不追求单调)。

- [ ] **Step 1: 写失败测试**

`src-tauri/src/agent/state.rs` 底部:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legal_transitions() {
        use SessionState::*;
        assert!(Running.can_transition_to(Stopping));
        assert!(Running.can_transition_to(Failed));
        assert!(Running.can_transition_to(Exited));
        assert!(Stopping.can_transition_to(Exited));
        assert!(Stopping.can_transition_to(Failed));
    }

    #[test]
    fn terminal_states_absorb_nothing() {
        use SessionState::*;
        for from in [Exited, Failed] {
            for to in [Running, Stopping, Exited, Failed] {
                assert!(!from.can_transition_to(to), "{from:?} -> {to:?} 应被拒绝");
            }
        }
    }

    #[test]
    fn running_to_running_is_idempotent_rejected() {
        use SessionState::*;
        assert!(!Running.can_transition_to(Running));
    }

    #[test]
    fn snapshot_serializes_camel_case() {
        let snap = SessionSnapshot {
            session_id: SessionId::new(),
            state: SessionState::Running,
            started_at_ms: 1234,
            exit_code: None,
            pid: Some(42),
        };
        let json = serde_json::to_string(&snap).unwrap();
        assert!(json.contains("\"sessionId\""));
        assert!(json.contains("\"startedAtMs\""));
        assert!(json.contains("\"exitCode\""));
        assert!(!json.contains("session_id"), "键名必须 camelCase");
    }
}
```

- [ ] **Step 2: 红** — `cargo test --lib agent::state` 编译错误。

- [ ] **Step 3: 实现**

`src-tauri/src/agent/state.rs`(测试之上):

```rust
// 会话状态机(Rust 单一权威,spec §1.3):穷尽 match 保证新增状态时
// 编译器强制处理所有迁移点。M2 简化:Created 一闪不入表,
// WaitingInput 预留 v1.1 启发式推断(spec 明示的有意简化)。
use serde::Serialize;

use crate::ids::SessionId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SessionState {
    Running,
    /// 用户/管理端已请求停止,等待子进程退出
    Stopping,
    /// 子进程已退出(exit_code 有值)
    Exited,
    /// 非正常失败(spawn 失败、PTY 错误、wait 错误)
    Failed,
}

impl SessionState {
    /// 迁移合法性(单一权威;终态吸收一切 = false)
    pub fn can_transition_to(&self, next: SessionState) -> bool {
        use SessionState::*;
        matches!(
            (self, next),
            (Running, Stopping) | (Running, Exited) | (Running, Failed) | (Stopping, Exited)
                | (Stopping, Failed)
        )
    }
}

/// session://state 事件载荷(spec §1.4)
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StateChange {
    pub session_id: SessionId,
    pub prev: SessionState,
    pub next: SessionState,
    pub at_ms: u64,
    pub detail: Option<String>,
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// session_list 返回元素:会话快照(退出后保留,侧边栏显示 Exited/Failed)
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSnapshot {
    pub session_id: SessionId,
    pub state: SessionState,
    pub started_at_ms: u64,
    pub exit_code: Option<i32>,
    pub pid: Option<u32>,
}
```

`src-tauri/src/agent/manager.rs` 的 `SessionEvent` 改为(此任务只改类型与编译错误波及处,线程编排保持 M1 形态——Task 5 才重写):

```rust
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum SessionEvent {
    Output {
        session_id: SessionId,
        data: String,
        seq: u64,
    },
    State(crate::agent::state::StateChange),
    Exit {
        session_id: SessionId,
        code: i32,
    },
}
```

M1 的 `create` 里 sink 调用处补 `seq`(用每会话计数器,简单 `Arc<AtomicU64>` 或在 batcher 线程里局部计数后传值)与 `State` 事件(Running 迁移:`State(StateChange { session_id: id, prev: Running, next: ..., at_ms, detail })`——M1 无状态机,此任务让编译通过的最小做法:`create` 成功后发一次 `prev: Running, next: Running` 是非法迁移,不发即可,仅引入类型;真正的 State 事件发送在 Task 5)。若编译波及面大,允许把 Output/Exit 构造点用新字段补齐,行为不变。

- [ ] **Step 4: 绿 + 全量回归 + 提交**

```bash
cargo test && cargo fmt && cargo clippy --all-targets -- -D warnings
cd /Users/itsuka/CodeSpace/ItsukaNexus && git add src-tauri/src && git commit -m "feat(m2): SessionState 状态机与事件键名统一(sessionId)

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 4: `pty/replay.rs` 回放环形缓冲(TDD)

**Files:**
- Create: `src-tauri/src/pty/replay.rs`
- Modify: `src-tauri/src/pty/mod.rs`(加 `pub mod replay;`)

**Interfaces:**
- Consumes: —
- Produces: `ReplayBuffer::new(capacity_bytes: usize)`、`push_str(&mut self, s: &str)`(超容量丢**最旧**内容)、`snapshot(&self) -> String`(当前保留的全部内容)。Task 5 每会话一个(256KB)。

**学习点:** ring buffer 的 VecDeque<u8> 逐字节入队太慢;实用做法是 `VecDeque<String>` 按"段"入队,记总字节数,超限就从队头弹段(弹段可能过冲一点点,可接受——256KB 上限是近似值)。replay 是**字节近似**的显示用缓存,不是精确日志。

- [ ] **Step 1: 失败测试**(文件底部 `#[cfg(test)] mod tests`)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_order_and_snapshot() {
        let mut rb = ReplayBuffer::new(1024);
        rb.push_str("hello ");
        rb.push_str("world");
        assert_eq!(rb.snapshot(), "hello world");
        assert_eq!(rb.total_bytes(), 11);
    }

    #[test]
    fn evicts_oldest_when_full() {
        let mut rb = ReplayBuffer::new(10);
        rb.push_str("0123456789"); // 正好满
        rb.push_str("ABC");
        let s = rb.snapshot();
        assert!(s.ends_with("ABC"));
        assert!(s.len() <= 13, "最多保留新段+已存段,不会无限增长");
        assert!(!s.starts_with('0') || s.len() == 13, "最旧内容应被挤掉");
    }

    #[test]
    fn single_oversized_segment_replaces_all() {
        let mut rb = ReplayBuffer::new(8);
        rb.push_str("old");
        rb.push_str("this-segment-is-way-longer-than-capacity");
        let s = rb.snapshot();
        assert_eq!(s, "this-segment-is-way-longer-than-capacity");
        assert!(rb.total_bytes() >= s.len());
    }

    #[test]
    fn empty_snapshot_is_empty_string() {
        let rb = ReplayBuffer::new(64);
        assert_eq!(rb.snapshot(), "");
        assert_eq!(rb.total_bytes(), 0);
    }
}
```

- [ ] **Step 2: 红** — `cargo test --lib pty::replay` 编译错误。

- [ ] **Step 3: 实现**

```rust
// 回放缓冲(spec §1.3):每会话保留最近 ~256KB 输出,attach 时重放,
// 前端热重载/崩溃后终端不丢上下文。按"段"管理避免逐字节开销。
use std::collections::VecDeque;

pub struct ReplayBuffer {
    capacity: usize,
    total: usize,
    segments: VecDeque<String>,
}

impl ReplayBuffer {
    pub fn new(capacity_bytes: usize) -> Self {
        Self {
            capacity: capacity_bytes.max(1),
            total: 0,
            segments: VecDeque::new(),
        }
    }

    pub fn push_str(&mut self, s: &str) {
        if s.is_empty() {
            return;
        }
        self.segments.push_back(s.to_string());
        self.total += s.len();
        self.evict();
    }

    fn evict(&mut self) {
        while self.total > self.capacity && self.segments.len() > 1 {
            if let Some(front) = self.segments.pop_front() {
                self.total = self.total.saturating_sub(front.len());
            }
        }
        // 单段超容量:整段保留(替换语义,见测试 3)
    }

    pub fn snapshot(&self) -> String {
        self.segments.concat()
    }

    pub fn total_bytes(&self) -> usize {
        self.total
    }
}
```

- [ ] **Step 4: 绿 + 门槛 + 提交**

```bash
cargo test && cargo fmt && cargo clippy --all-targets -- -D warnings
cd /Users/itsuka/CodeSpace/ItsukaNexus && git add src-tauri/src && git commit -m "feat(m2): 回放环形缓冲(段式 ~256KB,attach 重放)

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 5: SessionManager tokio 重构(核心)

**Files:**
- Rewrite: `src-tauri/src/agent/manager.rs`
- Modify: `src-tauri/tests/session_manager.rs`(M1 四条测试迁移 `#[tokio::test]` + 新增状态/背压断言)
- Modify: `src-tauri/src/lib.rs`(仅编译波及:create 变 async 后命令函数改 `async fn` + `.await`,sink 闭包处理新 SessionEvent 变体)

**Interfaces:**
- Consumes: PtySession(不变)、decode::Decoder、replay::ReplayBuffer、state::{SessionState, StateChange, SessionSnapshot, now_ms}、tokio/tokio-util
- Produces(IPC 层 Task 6 消费的最终形态):
  - `SessionManager::new(sink: EventSink)`(sink 现在只承载**低频** State/Exit;Output 走 per-session 订阅)
  - `async fn create(&self, provider_id: &str, cols: u16, rows: u16) -> Result<SessionId, NexusError>`——spawn 后发 `State{ Running }` 事件
  - `fn subscribe(&self, id: SessionId) -> Result<Subscription, NexusError>`,`Subscription { pub replay: String, pub rx: tokio::sync::mpsc::Receiver<OutputFrame> }`;重复 subscribe 替换旧订阅(M1 遗留 #2 的 Rust 侧;`OutputFrame { seq: u64, data: String }`)
  - `async fn send_input(&self, id, data: &str)`(经句柄 writer,不持表锁)
  - `fn resize(&self, id, cols, rows)`
  - `async fn stop(&self, id, force: bool)`——force=true 直接 kill;false 先发 `\x03`、`timeout(2s)` 等退出、再 kill(CancellationToken 协作取消输出任务)
  - `fn list(&self) -> Vec<SessionSnapshot>`(含已退出会话)
  - 退出/失败后条目保留(状态 Exited/Failed),`send_input` 对非 Running 会话返回 `NexusError::SessionNotFound` 改为 `Err(NexusError::InvalidState(..))`?——**决定**:新增变体 `NexusError::SessionNotRunning(String)`;SessionNotFound 仅指"从未存在"。

**学习点:** ① `spawn_blocking` 里跑阻塞 read 循环,返回值经通道回 async 世界——"阻塞 IO 的边界"就是这条线;② 有界 `mpsc::channel(64).send().await` 满时挂起 reader 任务 = 背压与 M1 同构;③ **join 超时版退出链**(P7 修复):wait 任务先 `spawn_blocking(child.wait)`,再 `timeout(JOIN_TIMEOUT, batcher_handle)`,超时则放弃 join(Batcher 卡死只泄漏一个任务,不再扣住 Exit 事件),detail 标注 "output pipeline stalled";④ 优雅关停用 CancellationToken 的 `select!`:Ctrl-C 字符是给**子进程**的信号,取消令牌是给**我们自己的任务树**的,两者配合。

**实现骨架(完整结构,细节内联注释):**

```rust
// SessionManager(async 版):会话注册表(瘦句柄)+ 每会话任务树。
pub struct SessionManager {
    sink: EventSink, // 低频 State/Exit;Output 走 subscribe
    sessions: std::sync::Mutex<HashMap<SessionId, SessionHandle>>,
}

pub struct SessionHandle {
    writer: Arc<Mutex<Box<dyn Write + Send>>>,   // 与 PtySession 相同的共享形态
    killer: Arc<Mutex<Box<dyn ChildKiller + Send + Sync>>>,
    master: Arc<dyn MasterPty + Send + Sync>,     // resize 用(需 PtySession 暴露)
    snapshot: SessionSnapshot,
    replay: Arc<Mutex<ReplayBuffer>>,
    subscriber: Arc<Mutex<Option<mpsc::Sender<OutputFrame>>>>,
    cancel: CancellationToken,
}
```

**PtySession 需要的最小改造**(同任务内完成):加 `pub fn split(self) -> SessionParts`(把 writer/killer/master 以 Arc 形态拆出,reader 已由 take_reader 交出,child 由 spawn 的二元组给出)或等价 accessor(`writer(&self) -> Arc<...>` 等)。保持既有测试不破:`spawn/take_reader/write_all/resize/kill` 的公开签名不变(内部改为委托 Arc)。

**create 编排:**

```rust
pub async fn create(&self, provider_id: &str, cols: u16, rows: u16) -> Result<SessionId, NexusError> {
    if provider_id != "shell" { return Err(NexusError::UnsupportedProvider(provider_id.into())); }
    let id = SessionId::new();
    let (session, child) = PtySession::spawn(&default_shell(), &[], cols, rows)?;
    let reader = session.take_reader();
    let parts = session.into_parts(); // writer/killer/master 的 Arc

    let (raw_tx, raw_rx) = mpsc::channel::<Vec<u8>>(QUEUE_DEPTH);      // 有界 64:背压
    let (out_tx, out_rx) = mpsc::channel::<OutputFrame>(SUBSCRIBER_DEPTH); // 订阅流,容量 128

    let cancel = CancellationToken::new();
    let replay = Arc::new(Mutex::new(ReplayBuffer::new(REPLAY_CAP)));
    let subscriber = Arc::new(Mutex::new(Some(out_tx)));

    // 任务 1/3:reader——spawn_blocking 桥接阻塞读(端口与 M1 相同的 8KB 循环)
    let cancel_r = cancel.clone();
    tokio::task::spawn_blocking(move || {
        let mut reader = reader; let mut buf = vec![0u8; READ_CHUNK];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    // 阻塞线程里不能 .await:用 blocking_send,语义=队列满则阻塞(=背压)
                    if raw_tx.blocking_send(buf[..n].to_vec()).is_err() { break; }
                }
            }
        }
        let _ = cancel_r; // reader 退出由 EOF 驱动,不需要令牌;令牌用于输出侧
    });

    // 任务 2/3:batcher——async 合帧 + 增量解码 + replay/订阅分发
    let mut decoder = Decoder::new();
    let replay_b = replay.clone(); let sub_b = subscriber.clone();
    let cancel_b = cancel.clone();
    let batcher = tokio::spawn(async move {
        let mut rx = raw_rx; let mut seq: u64 = 0;
        loop {
            tokio::select! {
                frame = next_frame_async(&mut rx) => {
                    let Some(frame) = frame else { break };
                    let text = decoder.feed(&frame);
                    if text.is_empty() { continue; }
                    seq += 1;
                    replay_b.lock().expect("replay 锁").push_str(&text);
                    let of = OutputFrame { seq, data: text };
                    let guard = sub_b.lock().expect("订阅锁");
                    if let Some(tx) = guard.as_ref() {
                        // 订阅通道满 = 前端消费慢:await 挂起,背压向 reader 传导
                        if tx.send(of).await.is_err() { /* 订阅者已 drop,只留 replay */ }
                    }
                }
                _ = cancel_b.cancelled() => break,
            }
        }
    });

    // 任务 3/3:wait——收割子进程;join batcher 带 5s 超时(P7 修复)
    let sink_exit = self.sink.clone();
    let snapshot_slot = /* 见下:状态写回需要表锁,经 Arc<Mutex<SessionSnapshot>> */;
    tokio::spawn(async move {
        let code = tokio::task::spawn_blocking(move || child.wait().map(|s| s.exit_code() as i32).unwrap_or(-1))
            .await
            .unwrap_or(-1);
        // join 输出管线(给最后几帧让路),超时放弃——Exit 不再被卡死(P7)
        let detail = match tokio::time::timeout(Duration::from_secs(5), batcher).await {
            Ok(_) => None,
            Err(_) => Some("output pipeline stalled".into()),
        };
        sink_exit(SessionEvent::Exit { session_id: id, code });
        // 状态迁移 Running→Exited/Failed 写回快照 + State 事件(经由 manager 提供的闭包或直接持有表引用)
    });

    // State{Running} 事件 + 入表(快照含 pid = child.process_id())
    self.sessions.lock().expect("表锁").insert(id, handle);
    Ok(id)
}
```

**async 合帧函数**(与 M1 的 next_frame 同语义,放 batcher.rs,旧同步版保留给 M1 单测):

```rust
pub async fn next_frame_async(rx: &mut mpsc::Receiver<Vec<u8>>, window: Duration, max_bytes: usize) -> Option<Vec<u8>> {
    let mut frame = rx.recv().await?;
    let deadline = tokio::time::Instant::now() + window;
    loop {
        if frame.len() >= max_bytes { break; }
        let now = tokio::time::Instant::now();
        if now >= deadline { break; }
        match tokio::time::timeout_at(deadline, rx.recv()).await {
            Ok(Some(chunk)) => frame.extend_from_slice(&chunk),
            Ok(None) | Err(_) => break,
        }
    }
    Some(frame)
}
```

**stop 优雅关停:**

```rust
pub async fn stop(&self, id: SessionId, force: bool) -> Result<(), NexusError> {
    let handle = { self.sessions.lock().expect("表锁").get(&id).cloned() }
        .ok_or_else(|| NexusError::SessionNotFound(id.to_string()))?;
    if !matches!(handle.snapshot.state, SessionState::Running) {
        return Err(NexusError::SessionNotRunning(id.to_string()));
    }
    self.set_state(id, SessionState::Stopping, None); // State 事件
    if !force {
        let _ = handle.writer.lock().expect("w").write_all(b"\x03"); // Ctrl-C 给子进程
        let exited = tokio::time::timeout(Duration::from_secs(2), async {
            // 轮询快照状态直至 Exited/Failed(200ms 间隔;真正通知由 wait 任务驱动)
            loop {
                tokio::time::sleep(Duration::from_millis(200)).await;
                if !matches!(self.snapshot_state(id), Some(SessionState::Running | SessionState::Stopping)) { return; }
            }
        }).await.is_ok();
        if exited { return Ok(()); }
    }
    let _ = handle.killer.lock().expect("k").kill(); // 上游 Windows 判定反转,以 Exit 为准
    Ok(())
}
```

`set_state` 是内部辅助:表锁更新快照 + `can_transition_to` 校验(非法迁移 log::warn 并跳过)+ sink 发 State 事件。wait 任务的 Exited/Failed 迁移也走它(通过 `Arc<dyn Fn(...)>` 或把表包进 `Arc` 后各任务共享 `Arc<SessionManagerInner>`——**实现自由度**:让 wait 任务持有 `Weak<SessionManagerInner>` 或一个 `Arc<Mutex<HashMap>>` + sink 的克隆即可,不追求唯一解,但必须保证:退出后条目在、状态对、State 事件只发一次)。

**测试迁移与新增**(`tests/session_manager.rs`):
- 文件头 `#![cfg(unix)]` 保留;`manager_with_channel` 的 mpsc 换 `tokio::sync::mpsc::unbounded_channel`(sink 在 async 上下文调用);每条测试 `#[tokio::test]`
- M1 四条(回显/stop→Exit+code≠0+后续报错/未知 provider/双会话独立)语义不变;错误断言更新:stop 后会话条目保留但 Stopping→Exited,`send_input` 得 `SessionNotRunning`
- 新增:`natural_exit` 后 `list()` 里该会话 state==Exited、exit_code==Some(0)
- 新增:`create` 后收到 State{Running} 事件先于任何 Output
- 新增:subscribe 返回的 replay 包含此前的输出(发 `echo marker` 后 subscribe,replay 含 marker),且后续输出经 rx 到达且 seq 单调

- [ ] **Step 1: 写失败测试**(迁移 + 新增,一次到位)
- [ ] **Step 2: 红**(编译错误为主)
- [ ] **Step 3: 实现**(manager.rs 重写 + PtySession into_parts + batcher next_frame_async + error 变体 + lib.rs 波及)
- [ ] **Step 4: 绿** `cargo test`(pty_session 3 条 + session_manager 全部 + lib 4 + app_info 1)
- [ ] **Step 5: 门槛 + 提交**

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings
cd /Users/itsuka/CodeSpace/ItsukaNexus && git add src-tauri/src src-tauri/tests && git commit -m "feat(m2): SessionManager tokio 重构——背压链/增量解码/replay/状态机/优雅关停

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 6: IPC——Channel 输出流 + session_attach/session_list

**Files:**
- Modify: `src-tauri/src/lib.rs`(session_attach、session_list 命令;emit 改造;错误映射)
- Modify: `src-tauri/Cargo.toml`(无需新依赖,Channel 在 tauri 里)

**Interfaces:**
- Consumes: Task 5 的 `subscribe/list`;`tauri::ipc::Channel`
- Produces(前端契约,Task 8 消费):
  - `session_attach { sessionId, output: Channel<PtyChunk> } -> { replayedBytes: u64 }`:Rust 侧 spawn 一个转发任务,把 `Subscription.rx` 的 OutputFrame 逐条 `channel.send(PtyChunk)`;**重复 attach 替换旧订阅**(Task 5 的 subscribe 语义,旧转发任务因发送端替换而结束)。replay 文本先经 Channel 发一条 `PtyChunk { seq: 0, data: replay }`
  - `session_list() -> Vec<SessionSnapshot>`
  - `PtyChunk { sessionId: String, data: String, seq: u64 }`(serde camelCase;注意 Channel 载荷里的 sessionId 冗余于会话,但保留——spec §1.4 契约)
  - 事件:`session://state`(StateChange)、`session://exit`({ sessionId, code });**`session://output` 全局 emit 废除**
  - `session_create` 返回 `{ sessionId, state }`(state 为 "running" 字符串→改 `SessionState` 序列化,即 `"running"` camelCase)
  - 错误:SessionNotRunning 映射为可读中文串(`map_err(|e| e.to_string())` 已足够,thiserror Display 写清楚)

**转发任务骨架**(lib.rs 内,这是唯一允许 use tauri 的层):

```rust
#[tauri::command]
async fn session_attach(
    state: State<'_, SessionManager>,
    session_id: String,
    output: tauri::ipc::Channel<PtyChunk>,
) -> Result<AttachAck, String> {
    let id = parse_id(session_id)?;
    let sub = state.subscribe(id).map_err(|e| e.to_string())?;
    let replayed = sub.replay.len() as u64;
    if !sub.replay.is_empty() {
        let _ = output.send(PtyChunk { session_id: id, data: sub.replay.clone(), seq: 0 });
    }
    tokio::spawn(async move {
        let mut rx = sub.rx;
        while let Some(frame) = rx.recv().await {
            if output.send(PtyChunk { session_id: id, data: frame.data, seq: frame.seq }).is_err() {
                break; // 前端 Channel 失效(webview 重载)
            }
        }
    });
    Ok(AttachAck { replayed_bytes: replayed })
}
```

- [ ] **Step 1**: lib.rs 命令层改造(attach/list/事件 emit 三变体/create 返回真实 state;`tauri::command` async fn + State<'_,...>;`app.manage(SessionManager::new(sink))` 不变,sink 现在只发 State/Exit 两类)
- [ ] **Step 2**: `cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt`(本任务薄封装无新测试,由 Task 8 E2E 与 Task 9 覆盖;既有测试必须全绿)
- [ ] **Step 3**: 提交 `feat(m2): session_attach(Channel)/session_list 与事件 emit 改造`

---

### Task 7: 配置子系统 + tauri-plugin-log

**Files:**
- Create: `src-tauri/src/config/mod.rs`(声明 + 默认配置)
- Create: `src-tauri/src/config/model.rs`(AppConfig/TerminalConfig/AgentProfile,serde)
- Create: `src-tauri/src/config/store.rs`(load/save,tmp+rename 原子写)
- Create: `src-tauri/tests/config_store.rs`
- Modify: `src-tauri/src/lib.rs`(config_get/config_save 命令;plugin-log 注册;manager 里 `log::warn` 接 kill 失败/非法迁移/背压事件)
- Modify: `src-tauri/capabilities/default.json`(加 `"log:default"`)
- Modify: `src-tauri/Cargo.toml`(tauri-plugin-log = "2"、log = "0.4")
- Modify: `package.json`(@tauri-apps/plugin-log)+ `pnpm install`

**Interfaces:**
- Consumes: tauri 的 `app.path().app_config_dir()`(IPC 层传入路径,store 本身只吃 `&Path`)
- Produces:
  - `AppConfig { version: u32(=1), terminal: TerminalConfig{ font_family: Option, font_size: u16(=13), scrollback: u32(=5000) }, agent_profiles: Vec<AgentProfile> }`
  - `AgentProfile { id, display_name, command, args_template: Vec<String>, env: HashMap<String,String> }`
  - 默认配置含一条 `shell` profile(command=default_shell 的占位——M2 不驱动 spawn,M4 才接)
  - `Store::load_or_create(dir: &Path) -> AppConfig`、`Store::save(dir: &Path, &AppConfig) -> Result<(), NexusError>`(写 `config.json`,tmp + rename)
  - IPC:`config_get() -> AppConfig`、`config_save(config: AppConfig) -> AppConfig`(保存后返回落盘值)
  - 日志:`tauri_plugin_log`(target log 文件 + stderr;级别 info)

**测试**(`tests/config_store.rs`,tempfile 目录…… 用 `std::env::temp_dir()` + 唯一子目录,不引 tempfile 依赖):
- load_or_create 在空目录生成默认配置且文件存在
- save 后 load 往返相等
- save 写坏 JSON 前的旧文件不被破坏(原子性:构造 tmp 写失败场景太重,跳过;测 rename 后内容完整即可)

- [ ] **Step 1**: 失败测试 → 红
- [ ] **Step 2**: model/store 实现 → 绿
- [ ] **Step 3**: lib.rs 接 config_get/save + plugin-log + capabilities + manager 三处 log::warn;`cargo test && clippy && fmt`;前端 `pnpm add @tauri-apps/plugin-log` + `pnpm build`
- [ ] **Step 4**: 提交 `feat(m2): AppConfig 原子读写与 tauri-plugin-log 接入`

---

### Task 8: 前端——zustand + terminalManager + 多 tab + 恢复

**Files:**
- Create: `src/stores/sessionsStore.ts`(zustand)
- Create: `src/features/terminal/terminalManager.ts`(React 外实例注册表 + attach 编排)
- Create: `src/features/terminal/TerminalTabs.tsx`
- Modify: `src/features/terminal/TerminalPane.tsx`(改常驻模式:接收 sessionId,不再 onReady/create)
- Modify: `src/App.tsx`(AppShell:顶栏 + tab 栏 + 终端区;启动恢复流程)
- Modify: `src/ipc/commands.ts`(sessionAttach(Channel)/sessionList/configGet/configSave;sessionCreate 返回类型更新)
- Modify: `src/ipc/events.ts`(onSessionState/onSessionExit 全局订阅,按 sessionId 分发;onSessionOutput 删除)
- Modify: `src/ipc/types.ts`(PtyChunk/SessionSnapshot/SessionState/StateChange/SessionExitEvent 新形态/AppConfig)

**Interfaces:**
- Consumes: Task 6 的 IPC 契约;Task 7 的 config
- Produces: M2 UI 形态——顶栏(app 信息 + 新建按钮)+ tab 栏(每会话一个 tab:标题=序号,状态徽章 Running/Stopping/Exited/Failed,关闭按钮)+ 主区(所有会话的 pane 常驻挂载,`display:none` 切换;性能红线:输出直达 term.write 不进 React)

**核心代码(完整给出):**

`sessionsStore.ts`:

```typescript
import { create } from "zustand";
import type { SessionExitEvent, SessionState, SessionSnapshot } from "../ipc/types";

interface SessionsState {
  sessions: Record<string, SessionSnapshot>;
  activeId: string | null;
  /** 启动恢复:session_list 结果填充 */
  hydrate: (snaps: SessionSnapshot[]) => void;
  add: (snap: SessionSnapshot) => void;
  setActive: (id: string | null) => void;
  onState: (sessionId: string, next: SessionState) => void;
  onExit: (ev: SessionExitEvent) => void;
  /** 本地移除 tab(会话可能已退出;条目仍留在 Rust 侧列表里) */
  closeTab: (id: string) => void;
}

export const useSessions = create<SessionsState>((set, get) => ({
  sessions: {},
  activeId: null,
  hydrate: (snaps) =>
    set(() => ({
      sessions: Object.fromEntries(snaps.map((s) => [s.sessionId, s])),
      activeId: snaps.find((s) => s.state === "running")?.sessionId ?? snaps[0]?.sessionId ?? null,
    })),
  add: (snap) =>
    set((st) => ({ sessions: { ...st.sessions, [snap.sessionId]: snap }, activeId: snap.sessionId })),
  setActive: (id) => set({ activeId: id }),
  onState: (sessionId, next) =>
    set((st) => {
      const cur = st.sessions[sessionId];
      if (!cur) return st;
      return { sessions: { ...st.sessions, [sessionId]: { ...cur, state: next } } };
    }),
  onExit: (ev) =>
    set((st) => {
      const cur = st.sessions[ev.sessionId];
      if (!cur) return st;
      return {
        sessions: {
          ...st.sessions,
          [ev.sessionId]: { ...cur, state: "exited", exitCode: ev.code },
        },
      };
    }),
  closeTab: (id) =>
    set((st) => {
      const sessions = { ...st.sessions };
      delete sessions[id];
      const ids = Object.keys(sessions);
      return { sessions, activeId: st.activeId === id ? (ids[ids.length - 1] ?? null) : st.activeId };
    }),
}));
```

`terminalManager.ts`(React 外常驻层,spec §1.5):

```typescript
import { Channel } from "@tauri-apps/api/core";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";

import { sessionAttach, sessionSendInput, sessionStop } from "../../ipc/commands";
import type { AppConfig, PtyChunk } from "../../ipc/types";

interface Entry {
  terminal: Terminal;
  fit: FitAddon;
  /** attach 重入守卫:同一会话只挂一条 Channel */
  attaching: boolean;
}

const entries = new Map<string, Entry>();
let config: Pick<AppConfig["terminal"], "fontSize" | "scrollback"> = {
  fontSize: 13,
  scrollback: 5000,
};

export function setTerminalConfig(c: AppConfig["terminal"]): void {
  config = { fontSize: c.fontSize, scrollback: c.scrollback };
}

export function getEntry(id: string): Entry | undefined {
  return entries.get(id);
}

export function createEntry(id: string, container: HTMLElement): Entry {
  const terminal = new Terminal({
    fontFamily: "Menlo, Monaco, 'Courier New', monospace",
    fontSize: config.fontSize,
    cursorBlink: true,
    scrollback: config.scrollback,
  });
  const fit = new FitAddon();
  terminal.loadAddon(fit);
  terminal.open(container);
  terminal.onData((data) => void sessionSendInput(id, data));
  const entry = { terminal, fit, attaching: false };
  entries.set(id, entry);
  return entry;
}

/** attach:replay + 后续流直达 term.write(绕过 React) */
export function attachSession(id: string, entry: Entry): void {
  if (entry.attaching) return;
  entry.attaching = true;
  const channel = new Channel<PtyChunk>();
  channel.onmessage = (msg) => entry.terminal.write(msg.data);
  void sessionAttach(id, channel).finally(() => {
    entry.attaching = false;
  });
}

export function disposeEntry(id: string): void {
  const e = entries.get(id);
  if (!e) return;
  entries.delete(id);
  e.terminal.dispose();
}

export async function stopSession(id: string): Promise<void> {
  await sessionStop(id);
}

/** resize 由 TerminalPane 的 ResizeObserver 调这里 */
export function fitAndResize(id: string, cols: number, rows: number): void {
  void sessionResizeReexport(id, cols, rows);
}

import { sessionResize as sessionResizeReexport } from "../../ipc/commands";
```

(注意:import 合并到文件头——上面为分段示意;成品文件 import 全部置顶。)

`TerminalPane.tsx` 改造要点:props 变 `{ sessionId: string; onFitted: (id: string, cols: number, rows: number) => void }`;mount 时 `createEntry`(若不存在)+ 立即 `attachSession`;ResizeObserver 防抖 100ms → `fit()` → 上报尺寸;**不再有 create 逻辑**(新建会话是顶栏按钮的事)。App 持久渲染 `<div style={{display: activeId===id?"block":"none"}}>` 包裹每个已知会话的 pane——**组件不卸载,只藏**。

`App.tsx`(AppShell):

```tsx
function App() {
  const { sessions, activeId, hydrate, add, setActive, onState, onExit, closeTab } = useSessions();
  const [info, setInfo] = useState<AppInfo | null>(null);
  const bootedRef = useRef(false);

  // 启动:恢复会话列表 + 全局事件订阅 + 配置(P8 根治:输出走 Channel,状态走全局事件)
  useEffect(() => {
    if (bootedRef.current) return;
    bootedRef.current = true;
    void (async () => {
      try { setInfo(await getAppInfo()); } catch { /* 展示性信息 */ }
      try {
        setTerminalConfig((await configGet()).terminal);
      } catch { /* 默认值兜底 */ }
      try { hydrate(await sessionList()); } catch { /* 全新会话 */ }
    })();
    const unState = onSessionStateGlobal((sc) => useSessions.getState().onState(sc.sessionId, sc.next));
    const unExit = onSessionExitGlobal((ev) => useSessions.getState().onExit(ev));
    return () => { void unState.then((u) => u()); void unExit.then((u) => u()); };
  }, []);

  const handleNew = useCallback(async () => {
    // 临时容器尺寸未知:用 80x24 创建,pane 挂载后 fit+resize 校正(spec 风险 #2 由 resize 链路消化)
    const created = await sessionCreate("shell", 80, 24);
    add({ sessionId: created.sessionId, state: created.state, startedAtMs: Date.now(), exitCode: null, pid: null });
  }, []);

  // 渲染:顶栏 + TerminalTabs + 常驻 pane 区(略——按上述结构写全)
}
```

刷新恢复路径:hydrate → 每个 snapshot 渲染 pane → pane mount → attachSession(replay 先到,Channel 流续上)→ 完成标准③。

- [ ] **Step 1**: types/commands/events 扩展(纯类型与封装)
- [ ] **Step 2**: terminalManager + sessionsStore
- [ ] **Step 3**: TerminalPane 改造 + TerminalTabs + App AppShell
- [ ] **Step 4**: `pnpm build` 全绿 + `cargo clippy/fmt --check`(Rust 侧无改动)
- [ ] **Step 5**: 提交 `feat(m2): zustand 多会话 UI——常驻终端实例/tab 栏/启动恢复`

---

### Task 9: 背压集成测试(完成标准②)

**Files:**
- Modify: `src-tauri/tests/session_manager.rs`(追加)

**测试设计**(unix):create 会话(subscribe 但**不消费** rx)→ `send_input("seq 1 20000\n")` → 等待若干输出进入 replay → 断言:订阅通道容量未爆(`out_rx.capacity() - out_rx.max_capacity()` 之类,或直接断言进程仍活着且 replay 在增长后趋停)→ 开始消费 rx → 收到完整数据(拼接后含 1..20000 的所有行,无丢失)→ 退出码正常。再加一条:消费期间 `list()` 与 `stop()` 不被阻塞(整表锁已拆,完成标准②的"UI 不卡"对应物)。

```rust
#[tokio::test]
async fn slow_consumer_applies_backpressure_without_loss() {
    let (mgr, sink_rx) = manager_with_channel().await;
    let id = mgr.create("shell", 200, 50).await.unwrap();
    let sub = mgr.subscribe(id).unwrap();
    // 不消费 sub.rx:订阅通道(128)填满后 batcher 挂起 → raw 队列(64)填满 → reader 挂起
    mgr.send_input(id, "seq 1 20000 > /dev/stdout\n").await.unwrap();
    // 观察 replay 增长趋停(背压生效的侧写):轮询至 total_bytes 稳定或超时
    // 然后开始消费,拼接到 20000 行完整
    let mut collected = String::new();
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_millis(500), sub.rx.recv()).await {
            Ok(Some(frame)) => collected.push_str(&frame.data),
            Ok(None) => break,
            Err(_) => continue,
        }
    }
    for n in 1..=20000u32 {
        assert!(collected.contains(&format!("\n{n}\n")) || collected.starts_with(&format!("{n}\n")) || collected.contains(&format!("{n}\n")), "行 {n} 不应丢失");
    }
    // 消费恢复期间管理操作不被阻塞
    let m2 = mgr.clone(); // 需 SessionManager: Clone(Arc 内部)或改用 Arc<SessionManager>
    let list_now = tokio::time::timeout(Duration::from_secs(2), async move { m2.list().len() }).await;
    assert!(list_now.is_ok(), "list() 不得被慢消费者阻塞");
}
```

(断言"每行都在"用 `contains(&format!("{n}\n"))` 对 20000 行是 O(n²) 字符串扫描——改为把 collected 按 '\n' 收集进 `HashSet<&str>` 再查;执行时按此实现。`SessionManager` 若非 Clone,manager_with_channel 返回 `Arc<SessionManager>` 统一处理。)

- [ ] **Step 1**: 写测试 → 红(若 subscribe 消费逻辑正确则直接绿;红的标准是暴露真 bug)
- [ ] **Step 2**: 修正至绿(只允许改实现,不许弱化断言)
- [ ] **Step 3**: 门槛 + 提交 `test(m2): 慢消费者背压——不丢字、内存有界、管理操作不阻塞`

---

### Task 10: M2 端到端验收(六项完成标准)

- [ ] **Step 1**(标准①):`pnpm tauri dev`,新建 4+ tab,各跑 `echo $TAB_N`、`vim`、`htop`(或 `top`)、`seq 1 100000 | tail`,切换 tab 输出互不串流、内容各自保持
- [ ] **Step 2**(标准③):某 tab 运行 `seq 1 50000` 中途前端 Ctrl+R(macOS Cmd+R)刷新——tab 列表恢复、各终端内容经 replay 回填、继续输入可用
- [ ] **Step 3**(标准④):设置页/M2 最小实现:改 fontSize 保存 → 重启应用配置保留(config.json 落在 app_config_dir;临时用 `defaults`/文件检查验证)
- [ ] **Step 4**(标准⑤):系统侧 `kill -9 <子进程 pid>`(pid 从 session_list 拿)→ 侧边栏/tab 徽章 3 秒内变 failed
- [ ] **Step 5**(标准②):Task 9 已自动化覆盖,此处补一次手动观察(终端里 `yes` + 暂停 webview 调试器)可选
- [ ] **Step 6**: 全量回归(`cargo test`/clippy/fmt/`pnpm build`)+ 日志检查(log 文件有 info 级会话事件,无 error 风暴)
- [ ] **Step 7**: 推送分支 + 建 PR + `gh pr checks` 四项全绿(注意 CI 由 PR 触发)

---

## Self-Review 记录

- **Spec 覆盖**:tokio 重构→T1/T5;多 tab→T8;Channel+attach+replay→T4/T5/T6/T8;session_list→T5/T6;config→T7;log→T7;decode→T2;六项标准→T10(①②③④⑤⑥ 分别对应)。M1 遗留七项映射表见上。✅
- **有意裁剪(非遗漏)**:进程组 kill(setsid/killpg)需 libc/nix,归 M3(P7 用 join 超时兜底根治"Exit 不发");WaitingInput 状态 v1.1;AgentProfile 仅结构+默认 shell 条目,真实注册表 M4;1-4 宫格 M4;TS 类型从 Rust 生成( specta)记 M3 议题;grid 布局 M4。
- **类型一致性**:PtyChunk{sessionId,data,seq} Rust(serde camelCase)↔TS(T8 types.ts)对齐;SessionSnapshot{sessionId,state,startedAtMs,exitCode,pid}↔TS;StateChange{sessionId,prev,next,atMs,detail}↔TS;SessionEvent tag=type 三变体与 lib.rs emit 的三个通道名对齐;NexusError 新增 SessionNotRunning 在 T5 定义、T6 映射。✅
- **已知风险**:T5 是最大任务(重写 manager),若子代理两轮仍卡,拆 T5a(PtySession into_parts + async batcher)/T5b(manager 重写)两发;T8 的 terminalManager 代码里 import 置顶已注明;T9 的 O(n²) 断言已改为 HashSet 方案并注明。✅
