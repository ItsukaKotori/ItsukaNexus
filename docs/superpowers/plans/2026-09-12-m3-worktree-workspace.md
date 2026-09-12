# ItsukaNexus M3(git worktree + workspace 拆分)Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 `src-tauri` 拆成 cargo workspace(`nexus-core` 脱离 tauri),清掉 M2 终审遗留的六条必办(进程组 kill/会话回收/管理小项/config 加固),交付 git worktree 全链路(GitOps trait + CLI 实现 + WorktreeManager + IPC + RepoPicker UI + session 落 worktree),满足 spec M3 六项完成标准。

**Architecture:** 方案 A(拆分先行):先机械搬运模块成 `crates/nexus-core`(不依赖 tauri,集成测试随模块走),随后所有必办与 gitx 新代码直接写在新 crate 里;git 全部走 CLI(tokio::process)+ `GitOps` trait 留缝,porcelain 稳定解析;会话回收是"关 tab 即删"(`session_dispose`);Unix force-kill 换进程组语义(killpg + drop master);前端引入 shadcn/ui + Tailwind v4 并移植 M2 手写 UI。

**Tech Stack:** cargo workspace、async-trait、libc(unix)、tokio::process、tempfile(dev)、tauri-plugin-dialog、Tailwind v4 + shadcn/ui、zustand。

**Spec:** `docs/superpowers/specs/2026-09-09-itsukanexus-mvp-design.md`(v1.1;M3 章节 + §1.1/§1.2 workspace 布局 + §1.3 kill 序列与会话回收 + §1.4 命令/事件 + §3 选型表)
**M2 完成记录:** `docs/superpowers/plans/2026-09-10-m2-async-multi-session.md` 末尾"M3 必办"章节——本计划已全部吸收(见下方映射表),执行者不必回读。
**M1 Windows PTY 知识库:** `docs/superpowers/plans/2026-09-09-m1-pty-terminal.md` 末尾(ConPTY DSR 代答、portable-pty 0.9 API 差异、阻塞读有界化)——涉及 PTY 测试的任务(T3/T4/T11)执行前先读。

## M2 完成记录必办 → 本计划映射

| M2 必办 | 落点 |
|---|---|
| #1 进程组 kill(kill 正忙 shell SIGHUP 不足 + 孙进程泄漏) | Task 3(Unix killpg + drop master;Windows 维持 killer + Exit 真相) |
| #2 会话回收(终态断订阅/条目清理) | Task 4(`session_dispose` 关 tab 即删 + 终态断订阅 + 终态 subscribe 返回已关闭流) |
| #3 list 排序 + send_input 写超时 | Task 4 |
| #4 config 三项(读错误不覆盖写/save clamp/命令 async 化) | Task 5 |
| #5 状态机穷尽 match + serde 钉住测试 + spec 措辞对齐 | Task 2(spec 措辞已在 v1.1 对齐) |
| #6 终端退出反馈 + 两处过时注释 | Task 10(遮罩 + 输入门禁 + 注释清理) |

## Global Constraints

- 开发机:macOS(Apple Silicon),Rust 1.95、node 22、pnpm 12.3.4;跨平台回归靠 CI 三平台矩阵(仅 main push 与 PR 触发)
- 分支:在 worktree 特性分支(建议名 `m3-worktree-workspace`)上实现,禁止直接提交 main;PR 由用户本人合并
- 提交规范:每任务一次提交,信息结尾 `Co-Authored-By: Claude Code <noreply@anthropic.com>`
- 包管理器只用 **pnpm**
- 质量门槛(每任务收尾必过):`cargo fmt` 已应用、`cargo clippy --all-targets -- -D warnings` 零警告、`cargo test` 全绿;涉及前端时 `pnpm build` 全绿
- **nexus-core 不依赖 tauri** 的缝保持:core 内不得 `use tauri`;Channel/emit 等 wire 类型只定义在 IPC 层(src-tauri/src)
- M2 参数定值不变:读 chunk 8KB、合帧窗口 16ms/32KB 上限、有界队列 64、订阅流 128、replay 256KB
- 事件与命令命名:命令 snake_case;新事件 `worktree://changed`(载荷 `{ repoPath, change }` camelCase);新命令 `session_dispose`
- worktree 命名规范(spec §1.3):分支与目录同名 `nexus/<provider>-<yyMMdd-HHmmss>-<rand4>`,目录集中 `<repo>/.nx-worktrees/`
- git 版本 gate:>= 2.20 才放行 worktree 功能(`GitCheckInfo.worktreeSupported`)
- 路径一律 `PathBuf` + canonicalize,不手拼字符串;git 调用一律 `git -C <path>` 或显式 cwd,防进程 cwd 漂移
- Windows 陷阱(M1/M2 实测,仍然有效):ConPTY 启动期 DSR 查询需代答;`WinChildKiller::kill` 成败判定反转,退出真相以 Exit 事件为准;测试中阻塞读/wait 必须有界(辅助线程 + recv_timeout 或 timeout 包裹)
- 前端红线:输出直达 `term.write` 不进 React;组件常驻只藏不卸

---

### Task 1: cargo workspace 拆分——nexus-core 机械搬运

**Files:**
- Create: `src-tauri/crates/nexus-core/Cargo.toml`
- Create: `src-tauri/crates/nexus-core/src/lib.rs`(模块声明与 re-export)
- Move: `src-tauri/src/{agent/, config/, pty/, error.rs, ids.rs}` → `src-tauri/crates/nexus-core/src/`(用 `git mv`,保留历史)
- Move: `src-tauri/tests/{session_manager.rs, pty_session.rs, config_store.rs}` → `src-tauri/crates/nexus-core/tests/`;`app_info.rs` 留在 `src-tauri/tests/`
- Move: `src-tauri/examples/{pty_echo.rs, tokio_basics.rs}` → `src-tauri/crates/nexus-core/examples/`
- Modify: `src-tauri/Cargo.toml`(workspace root + 依赖瘦身)
- Modify: `src-tauri/src/lib.rs`(改 `use nexus_core::...`;`parse_id` 改用 `SessionId::from_str`)
- Modify: `src-tauri/src/ids.rs`(搬运后位于 core;新增 `FromStr` 实现)

**Interfaces:**
- Consumes: 现有全部模块(签名零变化)
- Produces: crate `nexus_core`(lib name `nexus_core`),re-export `nexus_core::NexusError`;`SessionId` 新增 `impl FromStr`(Err = String);`src-tauri` 依赖 `nexus-core = { path = "crates/nexus-core" }`

**学习点:** cargo workspace:root package(src-tauri 自身)自动是 member,`members` 只需列子 crate;`path` 依赖不进 lockfile 版本;**同 crate 内部 `use crate::...` 路径在搬运后完全不变**——这就是 M0 起模块树按未来 crate 边界摆放的红利,搬运只动边界外的引用。

- [ ] **Step 1: 建 crate 骨架 + git mv**

```bash
cd /Users/itsuka/CodeSpace/ItsukaNexus/src-tauri
mkdir -p crates/nexus-core/src crates/nexus-core/tests crates/nexus-core/examples
git mv src/agent crates/nexus-core/src/agent
git mv src/config crates/nexus-core/src/config
git mv src/pty crates/nexus-core/src/pty
git mv src/error.rs crates/nexus-core/src/error.rs
git mv src/ids.rs crates/nexus-core/src/ids.rs
git mv tests/session_manager.rs crates/nexus-core/tests/
git mv tests/pty_session.rs crates/nexus-core/tests/
git mv tests/config_store.rs crates/nexus-core/tests/
git mv examples/pty_echo.rs crates/nexus-core/examples/
git mv examples/tokio_basics.rs crates/nexus-core/examples/
```

`crates/nexus-core/Cargo.toml`:

```toml
[package]
name = "nexus-core"
version = "0.1.0"
edition = "2021"

[dependencies]
portable-pty = "0.9"
tokio = { version = "1", features = ["full"] }
tokio-util = "0.7"
thiserror = "2"
uuid = { version = "1", features = ["v4", "serde"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
log = "0.4"
```

`crates/nexus-core/src/lib.rs`(新文件;原 src-tauri/src/lib.rs 的模块声明部分搬来):

```rust
// nexus-core:领域核心(会话/PTY/配置/gitx),不依赖 tauri——
// 可以脱离 GUI 用纯 cargo test 验证(spec §1.1 拆分动机)。
pub mod agent;
pub mod config;
pub mod error;
pub mod ids;
pub mod pty;

pub use error::NexusError;
```

- [ ] **Step 2: workspace root 与依赖瘦身**

`src-tauri/Cargo.toml`:在 `[package]` 之前加 workspace 段;`[dependencies]` 移除 `portable-pty`、`thiserror`、`uuid`、`tokio-util`(IPC 层不再直接用),新增 `nexus-core`;`tauri`/`tauri-plugin-*`/`serde`/`serde_json`/`log`/`tokio` 保留(命令层仍在用;若 `cargo build` 报 unused dependency 警告不存在——Rust 不报——以编译通过为准,不确定就先留):

```toml
[workspace]
members = ["crates/nexus-core"]
resolver = "2"

[package]
name = "itsukanexus"
# ...(其余不变)

[dependencies]
tauri = { version = "2", features = [] }
tauri-plugin-log = "2"
tauri-plugin-opener = "2"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
log = "0.4"
tokio = { version = "1", features = ["full"] }
nexus-core = { path = "crates/nexus-core" }
```

(`uuid` 从 src-tauri 移除的前提是 Step 3 把 `parse_id` 改成 `SessionId::from_str`。)

- [ ] **Step 3: core 侧新增 FromStr;IPC 层改引用**

`crates/nexus-core/src/ids.rs` 追加(与既有 `From<uuid::Uuid>` 并列):

```rust
impl std::str::FromStr for SessionId {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        uuid::Uuid::parse_str(s)
            .map(SessionId)
            .map_err(|e| format!("非法 session id {s:?}: {e}"))
    }
}
```

`src-tauri/src/lib.rs`:
- 头部模块声明改为只留 `pub mod app;`(app.rs 仍属 IPC 层,`app_info` 是展示命令)
- `use` 改:`use nexus_core::agent::manager::{SessionEvent, SessionManager, Subscription};`、`use nexus_core::agent::state::{SessionSnapshot, SessionState};`、`use nexus_core::config;`、`use nexus_core::ids::SessionId;`、`use nexus_core::NexusError;`(如用到)
- `parse_id` 改为:

```rust
fn parse_id(s: String) -> Result<SessionId, String> {
    s.parse::<SessionId>()
}
```

- 搬走的三个集成测试文件与两个 example:把 `itsukanexus_lib::` 全部替换为 `nexus_core::`(先 `grep -rn "itsukanexus_lib" crates/` 确认每一处)。

- [ ] **Step 4: 全量验证(行为零变化)**

```bash
cargo test && cargo fmt && cargo clippy --all-targets -- -D warnings
cargo test -p nexus-core   # spec 完成标准⑤:core 脱离 GUI 可独立测试
cd /Users/itsuka/CodeSpace/ItsukaNexus && pnpm build
```

Expected: 测试数与拆分前完全一致(35 个测试:core 34 + app_info 1——以实际计数为准,只多不少、零失败);clippy 零警告;前端构建无关不受影响。

- [ ] **Step 5: 提交**

```bash
cd /Users/itsuka/CodeSpace/ItsukaNexus && git add -A src-tauri && git commit -m "refactor(m3): cargo workspace 拆分——nexus-core 脱离 tauri(机械搬运)

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 2: 状态机穷尽 match + serde 钉住测试 + IPC 命令目录化

**Files:**
- Modify: `src-tauri/crates/nexus-core/src/agent/state.rs`(can_transition_to 穷进化 + 新测试)
- Create: `src-tauri/src/commands/mod.rs`、`src-tauri/src/commands/app.rs`、`src-tauri/src/commands/session.rs`、`src-tauri/src/commands/config.rs`
- Modify: `src-tauri/src/lib.rs`(命令全部移走,只剩 run() 与 State 注入体)

**Interfaces:**
- Consumes: Task 1 的 nexus-core
- Produces: `commands` 模块(`mod.rs` 内 `generate_handler!` 清单是 IPC 唯一注册点);`can_transition_to` 行为不变但穷尽 match(新增 SessionState 变体时编译器点名所有迁移分支)

**学习点:** `matches!` 宏对未覆盖的组合静默返回 false——新增状态时编译器不吭声;改写成穷尽 `match` 后,`(Exited, _)` 这类通配会被编译器在新增变体时标红,逼你看一眼每个落点。IPC 目录化:lib.rs 只做 Builder 组装,每个域一个文件,`generate_handler!` 清单集中在一处——M3 起 worktree 命令加进来不再膨胀单文件。

- [ ] **Step 1: state.rs 写失败测试(serde 钉住)**

在 `mod tests` 追加:

```rust
    #[test]
    fn session_state_serde_values_pinned() {
        // 前端 SessionState 联合类型按这些字面量对齐(types.ts),钉死防漂移
        assert_eq!(
            serde_json::to_string(&SessionState::Running).unwrap(),
            "\"running\""
        );
        assert_eq!(
            serde_json::to_string(&SessionState::Stopping).unwrap(),
            "\"stopping\""
        );
        assert_eq!(
            serde_json::to_string(&SessionState::Exited).unwrap(),
            "\"exited\""
        );
        assert_eq!(
            serde_json::to_string(&SessionState::Failed).unwrap(),
            "\"failed\""
        );
    }
```

- [ ] **Step 2: 红** — `cargo test -p nexus-core --lib state` 新测试编译失败(`serde_json` dev 依赖在 core 已有,应直接过;若直接过也行——它钉的是未来漂移)。
- [ ] **Step 3: can_transition_to 穷进化**

```rust
    /// 迁移合法性(单一权威;终态吸收一切 = false)。
    /// 穷尽 match:新增状态变体时,编译器在此分支点名所有漏改处
    /// (matches! 版本对未覆盖组合静默 false,这是 M2 终审必办 #5 的核心)。
    pub fn can_transition_to(&self, next: SessionState) -> bool {
        use SessionState::*;
        match (self, next) {
            (Running, Stopping) | (Running, Exited) | (Running, Failed) => true,
            (Stopping, Exited) | (Stopping, Failed) => true,
            (Running, Running)
            | (Stopping, Running)
            | (Stopping, Stopping)
            | (Exited, Running)
            | (Exited, Stopping)
            | (Exited, Exited)
            | (Exited, Failed)
            | (Failed, Running)
            | (Failed, Stopping)
            | (Failed, Exited)
            | (Failed, Failed) => false,
        }
    }
```

(注意:穷尽 match 不允许 `_` 通配——上面的展开就是全部 16 种组合。)

- [ ] **Step 4: 绿** — `cargo test -p nexus-core`(既有迁移测试全过,行为不变)。

- [ ] **Step 5: IPC 命令目录化(行为零变化)**

`src-tauri/src/commands/mod.rs`:

```rust
// IPC 命令层:每个域一个文件,generate_handler 清单是唯一注册点(spec §1.2)。
pub mod app;
pub mod config;
pub mod session;

pub use app::app_info;
pub use config::{config_get, config_save, ConfigDir};
pub use session::{
    parse_id, session_attach, session_create, session_list, session_resize,
    session_send_input, session_stop, AttachAck, PtyChunk, SessionCreated,
};
```

`commands/app.rs` / `commands/session.rs` / `commands/config.rs`:把 lib.rs 里对应命令函数、IPC wire 类型(`PtyChunk`/`AttachAck`/`SessionCreated`/`ConfigDir`)与 `parse_id` **原样搬过去**(代码不变,只改 `use` 为 `nexus_core::...`)。`src-tauri/src/lib.rs` 最终形态:

```rust
// IPC 层:tauri Builder 组装 + State 注入。命令在 commands/ 目录(spec §1.2)。
pub mod app; // M0 的 app_info 领域函数留这层(展示命令,无领域逻辑)
mod commands;

use std::path::PathBuf;
use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager};

use commands::ConfigDir;
use nexus_core::agent::manager::{SessionEvent, SessionManager};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .build(),
        )
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // 事件缝的 IPC 端:SessionEvent(State/Exit)→ emit,Output 走 Channel 不经这
            let handle: AppHandle = app.handle().clone();
            let sink = Arc::new(move |ev: SessionEvent| {
                let event = match &ev {
                    SessionEvent::Output { .. } => return,
                    SessionEvent::State(_) => "session://state",
                    SessionEvent::Exit { .. } => "session://exit",
                };
                let _ = handle.emit(event, ev);
            });
            app.manage(SessionManager::new(sink));
            app.manage(ConfigDir(
                app.path().app_config_dir().expect("解析应用配置目录失败"),
            ));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::session_create,
            commands::session_attach,
            commands::session_send_input,
            commands::session_resize,
            commands::session_stop,
            commands::session_list,
            commands::config_get,
            commands::config_save
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

(`PathBuf` import 若 ConfigDir 定义搬去 commands/config.rs 则不需要——以编译器为准清理。)

- [ ] **Step 6: 门槛 + 提交**

```bash
cargo test && cargo fmt && cargo clippy --all-targets -- -D warnings
cd /Users/itsuka/CodeSpace/ItsukaNexus && git add src-tauri && git commit -m "refactor(m3): 状态机穷尽 match + serde 钉住测试 + IPC 命令目录化

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 3: Unix 进程组 kill——killpg + drop master(必办 #1)

**Files:**
- Modify: `src-tauri/crates/nexus-core/Cargo.toml`(`[target.'cfg(unix)'.dependencies]` 加 libc)
- Modify: `src-tauri/crates/nexus-core/src/pty/session.rs`(master 变 `Option`,spawn 构造处同步)
- Modify: `src-tauri/crates/nexus-core/src/agent/manager.rs`(SessionHandle.master 变 Option;resize 容忍 None;stop 的 force 路径加 killpg + drop master)
- Modify: `src-tauri/crates/nexus-core/tests/session_manager.rs`(新增两条测试)

**Interfaces:**
- Consumes: `SessionSnapshot.pid: Option<u32>`(M2 已有)
- Produces: `PtySessionParts.master: Arc<Mutex<Option<Box<dyn MasterPty + Send>>>>`;`manager::resize` 对 master 已消费的会话静默成功;stop(force) 在 Unix 语义变为"杀进程组 + 关 PTY"

**学习点:** ① PTY 子进程打开 controlling terminal 时即成为 **session leader,pgid == pid**,所以 `libc::killpg(pid, SIGKILL)` 一发就杀掉 shell 与它的直接同组子进程;② 但"kill 正忙 shell"时前台 busy job 在**另一个进程组**,killpg 够不到——真正杀到它的是 **drop master**:master fd 关闭 → 内核向前台进程组投 SIGHUP → slave 全关 → reader 收 EOF(M2 实测 SIGHUP 不足的补全);③ `libc` 是 unsafe 的薄封印,这里 errno(ESRCH 等)一概忽略——kill 的真相以 wait 任务的 Exit 事件为准(M2 既定原则)。

- [ ] **Step 1: 写失败测试**

`tests/session_manager.rs` 追加(文件已有 `#![cfg(unix)]` 与 `manager_with_channel` 辅助;两条都复用 M2 的有界等待模式):

```rust
/// 必办#1-a:孙进程持 slave fd 时,force stop 仍应让 Exit 事件及时到达
/// (不靠 JOIN_TIMEOUT 超时兜底,detail 不得出现 stalled)。
#[tokio::test]
async fn force_stop_reaches_exit_with_grandchild_holding_slave() {
    let (mgr, mut sink_rx) = manager_with_channel().await;
    let id = mgr.create("shell", 120, 30).await.unwrap();
    // 后台孙进程:sleep 持有 slave fd,shell 退出后它还活着
    mgr.send_input(id, "sleep 1000 &\n").await.unwrap();
    tokio::time::sleep(Duration::from_millis(800)).await;
    mgr.stop(id, true).await.unwrap();
    // Exit 必须在 JOIN_TIMEOUT(5s) 的正常路径内到达:给 4s 上限,留余量
    let ev = recv_with_deadline(&mut sink_rx, Duration::from_secs(4))
        .await
        .expect("Exit 事件应及时到达");
    match ev {
        SessionEvent::Exit { session_id, code } => {
            assert_eq!(session_id, id);
            assert_ne!(code, 0, "强杀的退出码应非 0");
        }
        other => panic!("期望 Exit 事件,得到 {other:?}"),
    }
    // 状态应已迁移到终态(list 可见)
    let snap = mgr.list().into_iter().find(|s| s.session_id == id).unwrap();
    assert!(matches!(
        snap.state,
        nexus_core::agent::state::SessionState::Exited
            | nexus_core::agent::state::SessionState::Failed
    ));
}

/// 必办#1-b:kill 正忙 shell(前台死循环 job 在独立进程组)——M2 实测
/// SIGHUP 不足导致 Exit 永不到达;killpg + drop master 后必须收敛。
#[tokio::test]
async fn force_stop_kills_busy_shell_foreground_job() {
    let (mgr, mut sink_rx) = manager_with_channel().await;
    let id = mgr.create("shell", 120, 30).await.unwrap();
    mgr.send_input(id, "while :; do :; done\n").await.unwrap();
    tokio::time::sleep(Duration::from_millis(800)).await; // 等 job 进前台
    mgr.stop(id, true).await.unwrap();
    let ev = recv_with_deadline(&mut sink_rx, Duration::from_secs(4))
        .await
        .expect("busy shell 场景 Exit 也必须到达");
    assert!(matches!(ev, SessionEvent::Exit { .. }));
}
```

(`recv_with_deadline` 若文件里尚无等价辅助,按 M2 模式补:`loop { timeout(500ms, sink_rx.recv()) }` 聚合到总 deadline,收到 `State` 事件跳过继续等 `Exit`。)

- [ ] **Step 2: 红** — `cargo test -p nexus-core --test session_manager force_stop` — busy shell 用例超时失败(Exit 4s 内不到);grandchild 用例可能靠 stalled 兜底通过也可能失败,以 busy 用例红为准。

- [ ] **Step 3: 实现**

`Cargo.toml`(core)追加:

```toml
[target.'cfg(unix)'.dependencies]
libc = "0.2"
```

`pty/session.rs`:`PtySessionParts` 与 `PtySession` 的 master 字段改 `Arc<Mutex<Option<Box<dyn MasterPty + Send>>>>`;`spawn` 构造 `Arc::new(Mutex::new(Some(pair.master)))`;`take_reader`/`resize` 改为先 `guard.as_ref()` 判 None(master 已被 kill 消费时 `take_reader` 不可能再被调——create 时序保证;`resize` 返回 `Ok(())` 静默):

```rust
    pub fn resize(&self, cols: u16, rows: u16) -> Result<(), NexusError> {
        let master = self.master.lock().expect("master 锁被毒化");
        if let Some(m) = master.as_ref() {
            m.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
                .map_err(pty_err)?;
        }
        Ok(())
    }
```

`agent/manager.rs`:
- `SessionHandle.master` 类型同步改 `Arc<Mutex<Option<Box<dyn MasterPty + Send>>>>`;`create` 里 `master: parts.master`(spawn 已包好 Some);`resize` 命令路径同上判 None 静默。
- `stop` 的 kill 段(force 或宽限超时后共用的末段)改为:

```rust
        {
            // Unix 进程组语义(M3 必办#1):子进程开 controlling terminal 即
            // session leader(pgid == pid),killpg 一次杀掉 shell 组;
            // 前台 busy job 在另一进程组——由 drop master 触发内核 SIGHUP。
            #[cfg(unix)]
            if let Some(pid) = handle.snapshot.pid {
                // ESRCH(组已死)等 errno 一概忽略:退出真相以 Exit 事件为准
                unsafe {
                    libc::killpg(pid as libc::pid_t, libc::SIGKILL);
                }
            }
            // 上游 portable-pty 0.9.0 WinChildKiller::kill 成败判定反转:
            // Err 不代表失败。Windows 主路径;Unix 兜底(killpg 之外的保险)
            let mut killer = handle.killer.lock().expect("killer 锁被毒化");
            if let Err(e) = killer.kill() {
                #[cfg(windows)]
                log::debug!("kill 返回 Err session={id}(Windows 判定反转,以 Exit 事件为真相): {e}");
                #[cfg(not(windows))]
                log::warn!("kill 失败 session={id}: {e}");
            }
        }
        // drop master:内核向前台进程组发 SIGHUP(kill 正忙 shell 的关键一击),
        // slave 全关 → reader EOF;master 已被消费(None)则无事发生
        drop(handle.master.lock().expect("master 锁被毒化").take());
        // 令牌是给我们自己的任务树的:kill 后输出管线无需等 EOF
        handle.cancel.cancel();
        Ok(())
```

- [ ] **Step 4: 绿** — 两条新测试过 + 全量 `cargo test -p nexus-core` 无回归(M2 的 stop→Exit 测试语义不变)。
- [ ] **Step 5: 门槛 + 提交**

```bash
cargo test && cargo fmt && cargo clippy --all-targets -- -D warnings
cd /Users/itsuka/CodeSpace/ItsukaNexus && git add src-tauri && git commit -m "fix(m3): Unix 进程组 kill——killpg+drop master 根治 busy shell 与孙进程泄漏

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 4: 会话回收(session_dispose)+ list 排序 + send_input 写超时(必办 #2/#3)

**Files:**
- Modify: `src-tauri/crates/nexus-core/src/agent/manager.rs`(dispose/终态断订阅/终态 subscribe 返回已关闭流/list 排序/send_input 超时)
- Modify: `src-tauri/crates/nexus-core/tests/session_manager.rs`(新增测试)

**Interfaces:**
- Consumes: Task 3 的 manager 形态
- Produces:
  - `pub fn dispose(&self, id: SessionId) -> Result<(), NexusError>`:仅终态(Exited/Failed)可删;运行中 → `SessionNotRunning`,不存在 → `SessionNotFound`。删除 = 表里 `remove`(replay/subscriber/句柄/令柄随 Arc 释放)
  - `list()` 按 `started_at_ms` 升序
  - `send_input` 写路径 2s 超时:超时返回 `NexusError::Pty(io::Error::other("写超时..."))`,不再永久挂 invoke
  - `subscribe` 对终态会话:返回 `Subscription { replay, rx: 已关闭的 rx }`(replay 照常可用;转发任务 recv 立即 None 自然退出,不泄漏)

**学习点:** ① drop 一个 `SessionHandle` 就是释放它的全部 Arc 引用链——Rust 的所有权即资源管理(RAII)在这里替代了显式 close;② "终态 subscribe 返回已关闭流"的构造:造一对 channel 后立刻 drop 发送端,接收端 recv() 立即返回 None——这是 mpsc 的优雅关闭语义;③ spawn_blocking 超时后**线程仍在跑**(无法安全杀线程),持有 writer 锁直到写入最终完成/失败——所以超时只是"放弃等待并报错",锁竞争由 stop 路径自己的写超时兜底,注释写明。

- [ ] **Step 1: 写失败测试**

```rust
/// 必办#2:dispose 关 tab 即删——终态可删、运行中拒绝、删后 SessionNotFound
#[tokio::test]
async fn dispose_removes_terminal_session_only() {
    let (mgr, mut sink_rx) = manager_with_channel().await;
    let id = mgr.create("shell", 80, 24).await.unwrap();
    assert!(matches!(
        mgr.dispose(id),
        Err(NexusError::SessionNotRunning(_))
    ));
    // 自然退出
    mgr.send_input(id, "exit\n").await.unwrap();
    drain_until_exit(&mut sink_rx, id).await;
    mgr.dispose(id).unwrap();
    assert!(mgr.list().is_empty(), "dispose 后 list 应为空");
    assert!(matches!(
        mgr.send_input(id, "x").await,
        Err(NexusError::SessionNotFound(_))
    ));
    assert!(matches!(
        mgr.dispose(id),
        Err(NexusError::SessionNotFound(_))
    ));
}

/// 必办#3-a:list 按 startedAtMs 排序(刷新后 tab 序稳定)
#[tokio::test]
async fn list_is_sorted_by_started_at() {
    let (mgr, _sink_rx) = manager_with_channel().await;
    let _a = mgr.create("shell", 80, 24).await.unwrap();
    tokio::time::sleep(Duration::from_millis(5)).await;
    let b = mgr.create("shell", 80, 24).await.unwrap();
    tokio::time::sleep(Duration::from_millis(5)).await;
    let _c = mgr.create("shell", 80, 24).await.unwrap();
    let snaps = mgr.list();
    let times: Vec<u64> = snaps.iter().map(|s| s.started_at_ms).collect();
    let mut sorted = times.clone();
    sorted.sort_unstable();
    assert_eq!(times, sorted);
    assert_eq!(snaps.last().unwrap().session_id, b, "后建的在最后(此处 b 先于 c 创建,断言非末位)");
    // 修正断言:第三个建的是 _c,它应最后
}
```

(注:list 排序断言写成:`assert_eq!(snaps[2].session_id, c)`——三个会话按创建序;上面示例最后一行有笔误,执行时以 `snaps[0]=a, snaps[1]=b, snaps[2]=c` 的逐位断言为准。)

```rust
/// 必办#3-b:巨量输入写给不读输入的子进程,send_input 必须超时返回而非挂起
#[tokio::test]
async fn send_input_times_out_when_child_never_reads() {
    let (mgr, _sink_rx) = manager_with_channel().await;
    let id = mgr.create("shell", 80, 24).await.unwrap();
    mgr.send_input(id, "sleep 1000\n").await.unwrap(); // 前台 job 不读 stdin
    tokio::time::sleep(Duration::from_millis(800)).await;
    let big = "x".repeat(512 * 1024); // 512KB,远超 PTY 输入缓冲
    let start = std::time::Instant::now();
    let r = mgr.send_input(id, &big).await;
    assert!(start.elapsed() < Duration::from_secs(6), "必须超时返回,不能挂死");
    assert!(r.is_err(), "超时应返回错误");
}
```

- [ ] **Step 2: 红** — `dispose` 不存在(编译错);`send_input_times_out...` 无超时实现时挂起 6s+ 失败。

- [ ] **Step 3: 实现**

`manager.rs`:

```rust
/// send_input 的写超时(必办#3):大粘贴 + 子进程不读 → write 阻塞。
/// 与 stop 的 Ctrl-C 写超时同型:超时放弃等待,spawn_blocking 线程随写入
/// 最终完成/失败自然收场(最多泄漏一个,进程退出兜底)。
const SEND_INPUT_TIMEOUT: Duration = Duration::from_secs(2);
```

`send_input` 的 `spawn_blocking(...).await` 外再包一层:

```rust
        match tokio::time::timeout(
            SEND_INPUT_TIMEOUT,
            tokio::task::spawn_blocking(move || -> Result<(), NexusError> {
                let mut w = writer.lock().map_err(|e| {
                    NexusError::Pty(std::io::Error::other(format!("writer 被毒化: {e}")))
                })?;
                w.write_all(&bytes)?;
                w.flush()?;
                Ok(())
            }),
        )
        .await
        {
            Ok(Ok(res)) => res,
            Ok(Err(e)) => Err(NexusError::Pty(std::io::Error::other(format!(
                "写任务失败: {e}"
            )))),
            Err(_) => Err(NexusError::Pty(std::io::Error::other(
                "写超时:子进程 2s 内未消费输入(可能已停止读取)",
            ))),
        }
```

`dispose` / 排序 / 终态断订阅 / 终态 subscribe:

```rust
    /// 会话回收(必办#2,"关 tab 即删"):仅终态可删。
    /// drop 条目 = 释放 replay/订阅/句柄/取消令牌的全部 Arc(RAII)。
    pub fn dispose(&self, id: SessionId) -> Result<(), NexusError> {
        let mut guard = self.inner.sessions.lock().expect("会话表锁被毒化");
        let handle = guard
            .get(&id)
            .ok_or_else(|| NexusError::SessionNotFound(id.to_string()))?;
        if !matches!(
            handle.snapshot.state,
            SessionState::Exited | SessionState::Failed
        ) {
            return Err(NexusError::SessionNotRunning(id.to_string()));
        }
        guard.remove(&id);
        Ok(())
    }
```

`list()` 收尾加:

```rust
        let mut snaps: Vec<SessionSnapshot> = ...collect();
        snaps.sort_by_key(|s| s.started_at_ms); // 必办#3:刷新后 tab 序稳定
        snaps
```

`subscribe` 开头(handle 拿到后)加终态分支:

```rust
        // 终态:batcher 已退出,不会再有帧。返回已关闭的 rx(drop 发送端),
        // IPC 转发任务 recv 即 None 自然收尾——刷新恢复时 attach 已退出会话
        // 不再泄漏常驻转发任务;replay 照常重放历史。
        if matches!(
            handle.snapshot.state,
            SessionState::Exited | SessionState::Failed
        ) {
            let replay = handle.replay.lock().expect("replay 锁").snapshot();
            let (_tx, rx) = mpsc::channel(1);
            drop(_tx);
            return Ok(Subscription { replay, rx });
        }
```

wait 任务在 `set_state(Exited/Failed)` 之后、emit Exit 之前加(锁序:表锁 → subscriber,符合文件头约定):

```rust
            // 终态断订阅(必办#2):常驻转发任务在存量耗尽后自然结束,
            // 退出会话不再占订阅通道;条目与 replay 保留(刷新恢复仍可看)
            if let Some(h) = inner.sessions.lock().expect("会话表锁被毒化").get_mut(&id) {
                *h.subscriber.lock().expect("订阅锁") = None;
            }
```

- [ ] **Step 4: 绿** — 新测试全过 + M2 既有测试无回归(attach/背压/接缝测试不受影响:它们都在 Running 生命周期内)。
- [ ] **Step 5: 门槛 + 提交**

```bash
cargo test && cargo fmt && cargo clippy --all-targets -- -D warnings
cd /Users/itsuka/CodeSpace/ItsukaNexus && git add src-tauri && git commit -m "feat(m3): session_dispose 关tab即删 + 终态断订阅 + list 排序 + 写超时

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 5: config 三项加固(必办 #4)

**Files:**
- Modify: `src-tauri/crates/nexus-core/src/config/store.rs`(load_or_create 返回是否落盘 + 非 NotFound 读错误不写)
- Modify: `src-tauri/crates/nexus-core/src/config/model.rs`(clamped)
- Modify: `src-tauri/src/commands/config.rs`(async 化)
- Modify: `src-tauri/crates/nexus-core/tests/config_store.rs`(新增测试)

**Interfaces:**
- Consumes: —
- Produces: `load_or_create(dir) -> (AppConfig, bool)`(bool = 是否落盘了默认;IPC 层取 `.0`,行为不变);`AppConfig::clamped(self) -> Self`(save 前应用,fontSize 6..=72、scrollback 100..=100_000);`config_get/config_save` 为 `async fn`(内部 spawn_blocking,IO 不占执行器)

**学习点:** tauri 2 async 命令 + `State<'_>` 参数要求返回 `Result`(borrow 跨 await 的限制)——两条 config 命令本来就返回 Result,顺手 async 化零成本。`clamp` 落在 save 内部而非命令层:**磁盘上的值永远是合法值**(以磁盘为准语义),前端不需要重复防御。

- [ ] **Step 1: 写失败测试**

`tests/config_store.rs` 追加:

```rust
/// 必办#4-a:非 NotFound 的读错误(如 config.json 是目录)不得覆盖写默认
#[test]
fn load_returns_default_without_write_on_non_notfound_error() {
    let dir = std::env::temp_dir().join(format!("nx-cfg-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::create_dir(dir.join("config.json")).unwrap(); // 目录 → read 报错(非 NotFound)
    let (cfg, wrote) = nexus_core::config::store::load_or_create(&dir);
    assert_eq!(cfg, nexus_core::config::model::AppConfig::default());
    assert!(!wrote, "读失败(非 NotFound)时不应尝试写默认");
    assert!(dir.join("config.json").is_dir(), "原目录保持原样");
    std::fs::remove_dir_all(&dir).ok();
}

/// 必办#4-b:save 前入参 clamp,磁盘永远是合法值
#[test]
fn save_clamps_out_of_range_values() {
    let dir = std::env::temp_dir().join(format!("nx-cfg-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut cfg = nexus_core::config::model::AppConfig::default();
    cfg.terminal.font_size = 200;
    cfg.terminal.scrollback = 0;
    nexus_core::config::store::save(&dir, &cfg).unwrap();
    let (loaded, _) = nexus_core::config::store::load_or_create(&dir);
    assert_eq!(loaded.terminal.font_size, 72);
    assert_eq!(loaded.terminal.scrollback, 100);
    std::fs::remove_dir_all(&dir).ok();
}
```

- [ ] **Step 2: 红** — 编译错(load_or_create 返回元组不存在)。

- [ ] **Step 3: 实现**

`store.rs` 的 `load_or_create` 签名与读分支改为:

```rust
/// 读配置:文件不存在 → 写默认并返回 (默认, true);解析失败 → 旧文件改名
/// `.bak` 后写默认 (默认, true);其余读错误(权限/是目录等,非 NotFound)
/// → 返回 (默认, **false**),不写盘(必办#4:读不到≠可以覆盖写)。
pub fn load_or_create(dir: &Path) -> (AppConfig, bool) {
    let path = dir.join(CONFIG_FILE);
    match std::fs::read(&path) {
        Ok(bytes) => match serde_json::from_slice::<AppConfig>(&bytes) {
            Ok(cfg) => return (cfg, false),
            Err(e) => {
                log::warn!("配置解析失败({e}),备份为 {CONFIG_BAK} 并重写默认");
                if let Err(e) = std::fs::rename(&path, dir.join(CONFIG_BAK)) {
                    log::warn!("配置备份失败: {e}");
                }
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => {
            log::warn!("配置读取失败({e}),返回默认且不写盘");
            return (AppConfig::default(), false);
        }
    }
    let cfg = AppConfig::default();
    if let Err(e) = save(dir, &cfg) {
        log::warn!("默认配置写盘失败: {e}");
    }
    (cfg, true)
}
```

`model.rs` 追加:

```rust
/// 终端参数合法区间(save 前统一 clamp,磁盘永远是合法值——必办#4)
pub const FONT_MIN: u16 = 6;
pub const FONT_MAX: u16 = 72;
pub const SCROLLBACK_MIN: u32 = 100;
pub const SCROLLBACK_MAX: u32 = 100_000;
```

`TerminalConfig` 与 `AppConfig` 各加:

```rust
impl TerminalConfig {
    pub fn clamped(mut self) -> Self {
        self.font_size = self.font_size.clamp(FONT_MIN, FONT_MAX);
        self.scrollback = self.scrollback.clamp(SCROLLBACK_MIN, SCROLLBACK_MAX);
        self
    }
}

impl AppConfig {
    pub fn clamped(mut self) -> Self {
        self.terminal = self.terminal.clamped();
        self
    }
}
```

`store.rs` 的 `save` 开头:`let cfg = &cfg.clone().clamped();`(其后全部用这个引用)。

`commands/config.rs` async 化:

```rust
#[tauri::command]
async fn config_get(dir: State<'_, ConfigDir>) -> Result<AppConfig, String> {
    let d = dir.0.clone();
    tokio::task::spawn_blocking(move || nexus_core::config::store::load_or_create(&d).0)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn config_save(
    dir: State<'_, ConfigDir>,
    config: AppConfig,
) -> Result<AppConfig, String> {
    let d = dir.0.clone();
    tokio::task::spawn_blocking(move || {
        nexus_core::config::store::save(&d, &config)?;
        nexus_core::config::store::load_or_create(&d).0
    })
    .await
    .map_err(|e: nexus_core::NexusError| e.to_string())?
    .map_err(|e: nexus_core::NexusError| e.to_string())
}
```

(注:闭包内两层 Result——`save` 的 `NexusError` 与 join 的 `JoinError`;写清楚泛型让编译器引导,类型标注以实际编译为准。)

- [ ] **Step 4: 绿 + 既有测试适配** — `tests/config_store.rs` 既有三处 `load_or_create(...)` 调用点补 `.0`(行为断言不变);全量 `cargo test`。
- [ ] **Step 5: 门槛 + 提交**

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings
cd /Users/itsuka/CodeSpace/ItsukaNexus && git add src-tauri && git commit -m "fix(m3): config 三项加固——读错误不覆盖写/save clamp/命令 async 化

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 6: 前端基建——Tailwind v4 + shadcn/ui 接入 + M2 UI 移植

**Files:**
- Modify: `package.json`(tailwindcss、@tailwindcss/vite、clsx、tailwind-merge、class-variance-authority、lucide-react——经 shadcn CLI 或手动)
- Modify: `vite.config.ts`(tailwindcss 插件 + `@` alias)
- Modify: `tsconfig.json`(baseUrl/paths)
- Modify: `src/index.css`(Tailwind 入口 + 暗色主题变量)
- Create: `components.json`、`src/lib/utils.ts`(cn)、`src/components/ui/*`(shadcn 组件:button/dialog/alert-dialog/input/label/checkbox/select/badge)
- Modify: `src/App.tsx`、`src/features/terminal/TerminalTabs.tsx`、`src/features/terminal/TerminalPane.tsx`(内联 style 全部换 Tailwind 类/shadcn 组件)
- Delete: `src/App.css`(若存在且已被清空引用)

**Interfaces:**
- Consumes: —
- Produces: `cn(...classes)` 工具(`src/lib/utils.ts`);`@/` 路径别名;暗色为**唯一主题**的 shadcn 变量(不做 light/dark 切换——桌面终端工具,spec §1.5);UI 视觉行为与 M2 验收基线一致(多 tab/恢复/配置/状态徽章四项不回退)

**学习点:** Tailwind v4 用 `@tailwindcss/vite` 插件 + CSS-first 配置(`@import "tailwindcss"`),没有 tailwind.config.js;shadcn/ui 不是运行时库——`shadcn add` 把组件**源码**拷进 `src/components/ui/`(基于 Radix 无头组件 + CVA 变体),仓库自有这些代码,可自由改。暗色唯一主题的做法:把 shadcn 的 `.dark` 变量值直接写进 `:root`。

- [ ] **Step 1: 安装与配置**

```bash
cd /Users/itsuka/CodeSpace/ItsukaNexus
pnpm add -D tailwindcss @tailwindcss/vite
pnpm dlx shadcn@latest init -y -b neutral   # 生成 components.json + 依赖(clsx/tailwind-merge/cva/lucide-react)
pnpm dlx shadcn@latest add button dialog alert-dialog input label checkbox select badge -y
```

(shadcn CLI 若交互卡住/参数不被当前版本接受,退路:手写 `components.json`、`src/lib/utils.ts`(`cn` = `clsx` + `tailwind-merge`)与 `src/components/ui/` 各组件——从 https://ui.shadcn.com/docs/components 复制 button/dialog/alert-dialog/input/label/checkbox/select/badge 源码,依赖手动 `pnpm add`。)

`vite.config.ts`:

```ts
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { fileURLToPath, URL } from "node:url";
// @ts-expect-error type error without @types/node package
import process from "node:process";
const host = process.env.TAURI_DEV_HOST;

export default defineConfig(() => ({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: { "@": fileURLToPath(new URL("./src", import.meta.url)) },
  },
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: "ws", host, port: 1421 } : undefined,
    watch: { ignored: ["**/src-tauri/**"] },
  },
}));
```

`tsconfig.json` compilerOptions 追加:

```json
    "baseUrl": ".",
    "paths": { "@/*": ["./src/*"] }
```

- [ ] **Step 2: index.css——Tailwind 入口 + 暗色唯一主题**

`src/index.css` 重写(shadcn neutral 的暗色调色板;桌面终端工具不做亮色):

```css
@import "tailwindcss";

/* 暗色为唯一主题:shadcn 的 .dark 变量直接落 :root(桌面终端工具,无亮色切换) */
:root {
  --background: oklch(0.145 0 0);
  --foreground: oklch(0.985 0 0);
  --card: oklch(0.205 0 0);
  --card-foreground: oklch(0.985 0 0);
  --popover: oklch(0.205 0 0);
  --popover-foreground: oklch(0.985 0 0);
  --primary: oklch(0.922 0 0);
  --primary-foreground: oklch(0.205 0 0);
  --secondary: oklch(0.269 0 0);
  --secondary-foreground: oklch(0.985 0 0);
  --muted: oklch(0.269 0 0);
  --muted-foreground: oklch(0.708 0 0);
  --accent: oklch(0.269 0 0);
  --accent-foreground: oklch(0.985 0 0);
  --destructive: oklch(0.704 0.191 22.216);
  --border: oklch(1 0 0 / 10%);
  --input: oklch(1 0 0 / 15%);
  --ring: oklch(0.556 0 0);
  --radius: 0.625rem;
}

@theme inline {
  --color-background: var(--background);
  --color-foreground: var(--foreground);
  --color-card: var(--card);
  --color-card-foreground: var(--card-foreground);
  --color-popover: var(--popover);
  --color-popover-foreground: var(--popover-foreground);
  --color-primary: var(--primary);
  --color-primary-foreground: var(--primary-foreground);
  --color-secondary: var(--secondary);
  --color-secondary-foreground: var(--secondary-foreground);
  --color-muted: var(--muted);
  --color-muted-foreground: var(--muted-foreground);
  --color-accent: var(--accent);
  --color-accent-foreground: var(--accent-foreground);
  --color-destructive: var(--destructive);
  --color-border: var(--border);
  --color-input: var(--input);
  --color-ring: var(--ring);
  --radius-sm: calc(var(--radius) - 4px);
  --radius-md: calc(var(--radius) - 2px);
  --radius-lg: var(--radius);
  --radius-xl: calc(var(--radius) + 4px);
}

html, body, #root { height: 100%; }
body { @apply bg-background text-foreground; }
```

- [ ] **Step 3: 移植 M2 UI(视觉基线不回退)**

- `TerminalTabs.tsx`:内联 style → Tailwind 类(`flex items-center gap-1 px-2 border-b border-border overflow-x-auto` 等);状态徽章配色映射改为 `running → text-green-500`、`stopping → text-amber-500`、`exited → text-gray-400`、`failed → text-red-500`(点用 `bg-*` 同色系);激活 tab 用 `bg-secondary` + `border border-border`;关闭按钮保持原生 button + Tailwind。
- `App.tsx`:`main` 用 `flex h-screen flex-col`;header 用 `flex items-center gap-3 border-b px-4 py-2`;错误 footer 用 `bg-destructive/10 px-4 py-1 text-sm text-red-400`;「新建会话」按钮换 shadcn `Button`。空态提示用 `text-muted-foreground`。
- `TerminalPane.tsx`:容器保持 `h-full w-full min-h-0 min-w-0`(Tailwind 类);为 Task 10 的退出遮罩预留结构(本任务不做遮罩)。
- `main.tsx` 不变;`App.css` 删除(确认无引用)。
- **不动**:terminalManager.ts、stores、ipc(T6 纯 UI 层);xterm 自带 css 照旧在 TerminalPane import。

- [ ] **Step 4: 构建 + 目检**

```bash
pnpm build   # tsc + vite 全绿
pnpm tauri dev   # 人工目检(M2 四项基线:多 tab 并行/刷新恢复/配置生效/状态徽章)——正式验收在 Task 12,这里粗看无破版即可
```

- [ ] **Step 5: 提交**

```bash
git add -A && git commit -m "feat(m3): Tailwind v4 + shadcn/ui 接入,M2 手写 UI 移植(暗色唯一主题)

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 7: gitx 域(上)——GitOps trait + git_check + git_validate_repo(TDD)

**Files:**
- Modify: `src-tauri/crates/nexus-core/Cargo.toml`(async-trait;dev-dependencies 加 tempfile)
- Create: `src-tauri/crates/nexus-core/src/gitx/mod.rs`、`src-tauri/crates/nexus-core/src/gitx/ops.rs`(trait + wire 类型)、`src-tauri/crates/nexus-core/src/gitx/cli.rs`(GitCliOps)
- Modify: `src-tauri/crates/nexus-core/src/lib.rs`(`pub mod gitx;`)
- Modify: `src-tauri/crates/nexus-core/src/error.rs`(Git 域变体)
- Create: `src-tauri/crates/nexus-core/tests/git_ops.rs`

**Interfaces:**
- Consumes: —
- Produces(Task 8/9 依赖的最终形态):
  - `gitx::ops::GitOps` trait(async 方法,一次定型含 worktree 三方法,Task 8 实现剩余):

```rust
#[async_trait::async_trait]
pub trait GitOps: Send + Sync {
    async fn check(&self) -> GitCheckInfo;
    async fn validate_repo(&self, path: &Path) -> Result<RepoInfo, NexusError>;
    async fn worktree_list(&self, repo: &Path) -> Result<Vec<WorktreeInfo>, NexusError>;
    async fn worktree_create(
        &self,
        repo: &Path,
        name: &str,
        base_ref: Option<&str>,
    ) -> Result<(), NexusError>;
    async fn worktree_remove(&self, repo: &Path, path: &Path) -> Result<(), NexusError>;
}
```

  - `GitCheckInfo { available: bool, version: Option<String>, path: Option<String>, worktree_supported: bool }`、`RepoInfo { root: PathBuf, current_branch: Option<String>, is_clean: bool }`(均 serde camelCase)
  - `GitCliOps::new()`(git_bin = "git")与 `GitCliOps::with_bin(bin: &str)`(测试注入不存在的二进制名,模拟系统无 git——不碰全局 PATH)
  - `NexusError` 新变体:`GitUnavailable(String)`、`NotARepo(String)`、`GitCommand { cmd: String, stderr: String }`

**学习点:** ① Rust 1.75 的 async fn in trait 不支持 `dyn Trait`(对象安全),而我们要 `Arc<dyn GitOps>` 留缝——`async-trait` 宏把方法改写为返回 `Pin<Box<dyn Future>>`,这是生态标准解;② `git --version` 输出形如 `git version 2.39.5 (Apple Git-101)`,版本解析取前三个数字;③ 测试注入用 `with_bin` 而非改 PATH:进程级全局状态在并行测试里是竞态源头。

- [ ] **Step 1: 写失败测试**

`tests/git_ops.rs`:

```rust
// gitx CLI 实现的集成测试:真实调 git(CI 三平台均预装)。
// 临时 repo 用 tempfile::tempdir(自动清理;spec M3 学习主题点名)。
use std::path::Path;
use std::process::Command;

use nexus_core::error::NexusError;
use nexus_core::gitx::cli::GitCliOps;
use nexus_core::gitx::ops::GitOps;

/// 建一个有一次提交的临时 git repo,返回其根目录(tempdir 句柄由调用方持有)
fn init_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let run = |args: &[&str]| {
        let st = Command::new("git")
            .args(["-C", dir.path().to_str().unwrap()])
            .args(args)
            .status()
            .unwrap();
        assert!(st.success(), "git {args:?} 失败");
    };
    run(&["init", "-q"]);
    run(&["config", "user.email", "test@nx.local"]);
    run(&["config", "user.name", "nx-test"]);
    std::fs::write(dir.path().join("README.md"), "# t\n").unwrap();
    run(&["add", "."]);
    run(&["commit", "-qm", "init"]);
    dir
}

#[tokio::test]
async fn check_finds_git_and_version() {
    let ops = GitCliOps::new();
    let info = ops.check().await;
    assert!(info.available, "CI/开发机都应装有 git");
    let v = info.version.expect("版本应解析出来");
    assert!(v.starts_with(char::is_ascii_digit), "版本串形如 2.39.5: {v}");
    assert!(info.worktree_supported, ">= 2.20 才支持 worktree");
    assert!(info.path.is_some());
}

#[tokio::test]
async fn check_with_missing_binary_reports_unavailable() {
    let ops = GitCliOps::with_bin("definitely-not-git-nx");
    let info = ops.check().await;
    assert!(!info.available);
    assert!(info.version.is_none());
    assert!(!info.worktree_supported);
}

#[tokio::test]
async fn validate_repo_roundtrip() {
    let ops = GitCliOps::new();
    let dir = init_repo();
    let info = ops.validate_repo(dir.path()).await.unwrap();
    assert_eq!(info.root, dir.path().canonicalize().unwrap());
    assert!(info.current_branch.is_some(), "init 后应停在某分支上");
    assert!(info.is_clean, "刚 commit 完应干净");

    std::fs::write(dir.path().join("dirty.txt"), "x").unwrap();
    let info = ops.validate_repo(dir.path()).await.unwrap();
    assert!(!info.is_clean, "未跟踪文件应视为脏");
}

#[tokio::test]
async fn validate_rejects_non_repo() {
    let dir = tempfile::tempdir().unwrap();
    let ops = GitCliOps::new();
    match ops.validate_repo(dir.path()).await {
        Err(NexusError::NotARepo(_)) => {}
        other => panic!("期望 NotARepo,得到 {other:?}"),
    }
}
```

- [ ] **Step 2: 红** — `cargo test -p nexus-core --test git_ops` 编译错(模块不存在)。

- [ ] **Step 3: 实现**

`Cargo.toml`(core):`async-trait = "0.1"` 进 `[dependencies]`;`tempfile = "3"` 进 `[dev-dependencies]`。

`error.rs` 追加变体:

```rust
    /// 系统无 git 或版本低到不可用(check 已探测,功能入口应先 gate)
    #[error("git 不可用: {0}")]
    GitUnavailable(String),
    /// 给定路径不是 git 仓库(rev-parse 失败)
    #[error("不是 git 仓库: {0}")]
    NotARepo(String),
    /// git 命令执行失败(stderr 透传给 UI)
    #[error("git {cmd} 失败: {stderr}")]
    GitCommand { cmd: String, stderr: String },
```

`gitx/mod.rs`:

```rust
// gitx:git 集成域。选型(spec §3):CLI 子进程 + porcelain 解析,
// trait 缝留给未来的 git2 读实现。
pub mod cli;
pub mod ops;
pub mod worktree; // Task 8 落地;本任务先注释掉此行,Task 8 打开
```

`gitx/ops.rs`:

```rust
// GitOps trait(spec §1.3"一切外部能力都有 trait 缝"):v1 唯一实现 GitCliOps。
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::NexusError;

/// git --version 探测结果(worktree_supported = 版本 >= 2.20,spec 风险 #5)
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GitCheckInfo {
    pub available: bool,
    pub version: Option<String>,
    pub path: Option<String>,
    pub worktree_supported: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoInfo {
    pub root: PathBuf,
    pub current_branch: Option<String>,
    pub is_clean: bool,
}

#[async_trait::async_trait]
pub trait GitOps: Send + Sync {
    async fn check(&self) -> GitCheckInfo;
    async fn validate_repo(&self, path: &Path) -> Result<RepoInfo, NexusError>;
    async fn worktree_list(&self, repo: &Path) -> Result<Vec<WorktreeInfo>, NexusError>;
    async fn worktree_create(&self, repo: &Path, name: &str, base_ref: Option<&str>) -> Result<(), NexusError>;
    async fn worktree_remove(&self, repo: &Path, path: &Path) -> Result<(), NexusError>;
}

use super::worktree::WorktreeInfo; // Task 8 前先在 worktree.rs 占位定义
```

(`WorktreeInfo` 在本任务先建占位文件 `gitx/worktree.rs` 只含类型定义,Task 8 再填 WorktreeManager——或者把 `WorktreeInfo` 定义挪进 ops.rs,Task 8 引用之;二选一,推荐**后者**(ops.rs 定义,worktree.rs 只做业务),执行时保持 trait/类型集中在 ops.rs。)

`gitx/cli.rs`:

```rust
// GitCliOps:tokio::process 调 git CLI。一律 `git -C <path>`,不依赖进程 cwd。
use std::path::{Path, PathBuf};

use super::ops::{GitCheckInfo, GitOps, RepoInfo};
use crate::error::NexusError;

pub struct GitCliOps {
    git_bin: String,
}

impl GitCliOps {
    pub fn new() -> Self {
        Self { git_bin: "git".into() }
    }
    /// 测试注入:用不存在的二进制名模拟"系统无 git"(不碰全局 PATH)
    pub fn with_bin(bin: &str) -> Self {
        Self { git_bin: bin.to_string() }
    }

    async fn run(&self, repo: Option<&Path>, args: &[&str]) -> Result<String, NexusError> {
        let mut cmd = tokio::process::Command::new(&self.git_bin);
        if let Some(r) = repo {
            cmd.arg("-C").arg(r);
        }
        cmd.args(args);
        let cmd_repr = format!("{:?}", args);
        let out = cmd
            .output()
            .await
            .map_err(|e| NexusError::GitUnavailable(format!("无法执行 git({e})")))?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
            return Err(NexusError::GitCommand { cmd: cmd_repr, stderr });
        }
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    }
}

impl Default for GitCliOps {
    fn default() -> Self {
        Self::new()
    }
}

/// "git version 2.39.5 (Apple Git-101)" → "2.39.5"
fn parse_version(s: &str) -> Option<String> {
    let mut it = s.split_whitespace();
    while let Some(w) = it.next() {
        if w == "version" {
            let v = it.next()?;
            let dots = v.split('.').count();
            return if dots >= 2 { Some(v.to_string()) } else { None };
        }
    }
    None
}

/// 主次版本 >= 2.20(worktree porcelain 稳定期,spec 风险 #5)
fn supports_worktree(version: &str) -> bool {
    let mut it = version.split('.');
    let major: u32 = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let minor: u32 = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    (major, minor) >= (2, 20)
}

fn which_git() -> Option<String> {
    let (prog, flag) = if cfg!(windows) {
        ("where", "git")
    } else {
        ("sh", "-c 'command -v git'")
    };
    let out = std::process::Command::new(prog)
        .args(if cfg!(windows) { vec![flag] } else { vec![flag] })
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout);
    s.lines().next().map(|l| l.trim().to_string())
}

#[async_trait::async_trait]
impl GitOps for GitCliOps {
    async fn check(&self) -> GitCheckInfo {
        // 不走 run():git 缺失是"探测结果"而非错误
        match self.run(None, &["--version"]).await {
            Ok(out) => {
                let version = parse_version(&out);
                let worktree_supported = version.as_deref().map(supports_worktree).unwrap_or(false);
                GitCheckInfo {
                    available: true,
                    worktree_supported,
                    version,
                    path: which_git(),
                }
            }
            Err(_) => GitCheckInfo::default(),
        }
    }

    async fn validate_repo(&self, path: &Path) -> Result<RepoInfo, NexusError> {
        let root = match self.run(Some(path), &["rev-parse", "--show-toplevel"]).await {
            Ok(s) => PathBuf::from(s.trim()),
            Err(NexusError::GitCommand { stderr, .. }) => {
                return Err(NexusError::NotARepo(format!("{}: {stderr}", path.display())));
            }
            Err(e) => return Err(e),
        };
        let root = root.canonicalize().unwrap_or(root);
        let branch_out = self
            .run(Some(&root), &["branch", "--show-current"])
            .await?;
        let current_branch = {
            let b = branch_out.trim();
            if b.is_empty() { None } else { Some(b.to_string()) }
        };
        let status = self.run(Some(&root), &["status", "--porcelain"]).await?;
        Ok(RepoInfo {
            root,
            current_branch,
            is_clean: status.trim().is_empty(),
        })
    }

    // worktree 三方法:Task 8 实现;本任务先 todo!()/unimplemented 占位并在
    // 文档注释标明"Task 8 落地"——测试不触达,不影响绿
    async fn worktree_list(&self, _repo: &Path) -> Result<Vec<crate::gitx::ops::WorktreeInfo>, NexusError> {
        unimplemented!("Task 8")
    }
    async fn worktree_create(&self, _repo: &Path, _name: &str, _base_ref: Option<&str>) -> Result<(), NexusError> {
        unimplemented!("Task 8")
    }
    async fn worktree_remove(&self, _repo: &Path, _path: &Path) -> Result<(), NexusError> {
        unimplemented!("Task 8")
    }
}
```

(注意 `which_git` 的 unix 分支:用 `sh -c 'command -v git'`;上面示意把 flag 组合写清楚,执行时以能跑通为准——也可以简单起见两个平台都试 `git` 的 `--exec-path` 之外的方案,或干脆 unix 用 `/usr/bin/which git`。**保底方案**:若跨平台 which 太绕,`path` 字段允许返回 None(Option 本就允许),check 只保 available/version——spec 表格字段仍在,值可空。)

- [ ] **Step 4: 绿 + 门槛**

```bash
cargo test -p nexus-core --test git_ops && cargo test && cargo fmt && cargo clippy --all-targets -- -D warnings
```

- [ ] **Step 5: 提交**

```bash
cd /Users/itsuka/CodeSpace/ItsukaNexus && git add src-tauri && git commit -m "feat(m3): gitx 域(上)——GitOps trait + git_check/git_validate_repo(TDD)

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 8: gitx 域(下)——WorktreeManager:命名/增删查/事件(TDD)

**Files:**
- Modify: `src-tauri/crates/nexus-core/src/ids.rs`(`WorktreeName` newtype + 生成器)
- Modify: `src-tauri/crates/nexus-core/src/gitx/ops.rs`(`WorktreeInfo` 定义——若 Task 7 已占位则完善字段)
- Create: `src-tauri/crates/nexus-core/src/gitx/timefmt.rs`(epoch → `yyMMdd-HHmmss`,无依赖手写)
- Modify: `src-tauri/crates/nexus-core/src/gitx/cli.rs`(实现 worktree 三方法 + porcelain 解析)
- Create: `src-tauri/crates/nexus-core/src/gitx/worktree.rs`(WorktreeManager)
- Modify: `src-tauri/crates/nexus-core/src/gitx/mod.rs`(打开 `pub mod timefmt; pub mod worktree;`)
- Create: `src-tauri/crates/nexus-core/tests/worktree_manager.rs`

**Interfaces:**
- Consumes: Task 7 的 `GitOps` trait 与 `GitCliOps`
- Produces(Task 9/10/11 依赖):
  - `ids::WorktreeName(String)`:`WorktreeName::generate(provider: &str, epoch_secs: u64) -> WorktreeName`(格式 `nexus/<provider>-<yyMMdd-HHmmss>-<rand4>`,provider 先 sanitize 到 `[a-z0-9-]`)、`as_str()`、`FromStr`(校验 `nexus/` 前缀 + 段数)
  - `ops::WorktreeInfo { name: String, path: PathBuf, branch: Option<String> }`(serde camelCase)
  - `worktree::WorktreeChange { Created, Removed }`、`WorktreeChanged { repo_path: PathBuf, change: WorktreeChange }`(serde camelCase;`WorktreeEventSink = Arc<dyn Fn(WorktreeChanged) + Send + Sync>`)
  - `WorktreeManager::new(ops: Arc<dyn GitOps>, events: WorktreeEventSink)`:
    - `async fn create(&self, repo: &Path, provider: &str, base_ref: Option<&str>) -> Result<WorktreeInfo, NexusError>`——生成名、路径 `<repo>/.nx-worktrees/<name>`、`worktree add -b <name>`、emit Created
    - `async fn list(&self, repo: &Path) -> Result<Vec<WorktreeInfo>, NexusError>`
    - `async fn remove(&self, repo: &Path, name: &str, delete_branch: bool) -> Result<(), NexusError>`——按 name 查路径、`worktree remove`、可选 `branch -D`、`prune`、emit Removed

**学习点:** ① epoch → UTC 日期的手写算法是 Howard Hinnant 的 `civil_from_days`(chrono 级功能不值得为此引依赖):days = epoch/86400,按 400 年格里历周期算 y/m/d;② porcelain 解析纪律:git porcelain 输出是**稳定契约**(块以空行分隔,行首关键词),解析器只认关键词、不猜列位置,未知行跳过——这样 git 小版本变动不炸;③ worktree 的"名字"没有原生概念,我们约定 **name == 分支名 == 目录名**(spec §1.3),三者同源于生成器。

- [ ] **Step 1: 写失败测试**

`tests/worktree_manager.rs`:

```rust
// WorktreeManager 全流程:真实 git(CI 三平台预装)。
// 覆盖完成标准⑥:含空格/中文的 repo 路径。
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::sync::Mutex;

use nexus_core::error::NexusError;
use nexus_core::gitx::cli::GitCliOps;
use nexus_core::gitx::ops::GitOps;
use nexus_core::gitx::worktree::{WorktreeChange, WorktreeManager};
use nexus_core::ids::WorktreeName;

fn init_repo_at(dir: &Path) {
    let run = |args: &[&str]| {
        let st = Command::new("git").arg("-C").arg(dir).args(args).status().unwrap();
        assert!(st.success(), "git {args:?} 失败");
    };
    run(&["init", "-q"]);
    run(&["config", "user.email", "test@nx.local"]);
    run(&["config", "user.name", "nx-test"]);
    std::fs::write(dir.join("README.md"), "# t\n").unwrap();
    run(&["add", "."]);
    run(&["commit", "-qm", "init"]);
}

fn manager_with_events() -> (WorktreeManager, Arc<Mutex<Vec<WorktreeChange>>>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let s2 = seen.clone();
    let events = Arc::new(move |ev: nexus_core::gitx::worktree::WorktreeChanged| {
        s2.lock().unwrap().push(ev.change);
    });
    (WorktreeManager::new(Arc::new(GitCliOps::new()), events), seen)
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
    assert!(info.path.starts_with(dir.path().join(".nx-worktrees")));
    assert!(info.path.is_dir(), "worktree 目录应存在");
    assert!(info.path.to_string_lossy().contains(&info.name));
    // 分支存在(rev-parse --verify)
    let st = Command::new("git")
        .arg("-C").arg(dir.path())
        .args(["rev-parse", "--verify", &format!("refs/heads/{}", info.name)])
        .status().unwrap();
    assert!(st.success(), "分支 {} 应存在", info.name);

    let list = mgr.list(dir.path()).await.unwrap();
    assert!(list.iter().any(|w| w.name == info.name), "list 应含新 worktree(且含主 worktree)");

    // 在 worktree 里提交一笔
    std::fs::write(info.path.join("wt.txt"), "x").unwrap();
    let run = |args: &[&str]| Command::new("git").arg("-C").arg(&info.path).args(args).status().unwrap();
    assert!(run(&["add", "."]).success());
    assert!(run(&["commit", "-qm", "wt"]).success());

    mgr.remove(dir.path(), &info.name, true).await.unwrap();
    assert!(!info.path.exists(), "目录应删除");
    let list = mgr.list(dir.path()).await.unwrap();
    assert!(!list.iter().any(|w| w.name == info.name));
    // 事件顺序:Created → Removed
    let ev = events.lock().unwrap();
    assert!(matches!(ev.as_slice(), [WorktreeChange::Created, WorktreeChange::Removed]));
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
    match mgr.remove(dir.path(), "nexus/nope-000000-0000-0000", false).await {
        Err(NexusError::GitCommand { .. }) => {}
        other => panic!("期望 GitCommand 错误,得到 {other:?}"),
    }
}
```

- [ ] **Step 2: 红** — 编译错(WorktreeName/WorktreeManager 不存在)。

- [ ] **Step 3: 实现**

`ids.rs` 追加:

```rust
/// worktree 名 == 分支名 == 目录名(spec §1.3):`nexus/<provider>-<yyMMdd-HHmmss>-<rand4>`
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WorktreeName(String);

impl WorktreeName {
    pub fn generate(provider: &str, epoch_secs: u64) -> Self {
        let p: String = provider
            .chars()
            .map(|c| if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' { c } else { '-' })
            .collect();
        let ts = crate::gitx::timefmt::ymd_hms(epoch_secs);
        let rand: String = uuid::Uuid::new_v4().simple().to_string()[..4].to_string();
        Self(format!("nexus/{p}-{ts}-{rand}"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for WorktreeName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
```

`gitx/timefmt.rs`(Hinnant civil_from_days,UTC,无依赖):

```rust
/// epoch 秒 → "yyMMdd-HHmmss"(UTC)。手写格里历换算(Hinnant civil_from_days),
/// 避免为日期格式化引 chrono/time 依赖。
pub fn ymd_hms(epoch_secs: u64) -> String {
    let days = (epoch_secs / 86_400) as i64;
    let secs_of_day = epoch_secs % 86_400;
    let (y, m, d) = civil_from_days(days);
    let yy = y % 100;
    format!(
        "{:02}{:02}{:02}-{:02}{:02}{:02}",
        yy, m, d,
        secs_of_day / 3600,
        (secs_of_day % 3600) / 60,
        secs_of_day % 60
    )
}

/// 天数(自 1970-01-01)→ (年, 月, 日),格里历。算法:Howard Hinnant。
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}
```

(底部 `#[cfg(test)]` 补两条:已知时刻断言——如 `ymd_hms(1_700_000_000) == "231114-224048"`(2023-11-14T22:13:20Z,以权威计算为准,执行时用 `date -u -r 1700000000 +%y%m%d-%H%M%S` 核对后再钉死)与 `ymd_hms(0) == "700101-000000"`。)

`ops.rs` 完善/确认 `WorktreeInfo`:

```rust
/// worktree_list 元素:name = 分支名(我们的命名规范),外建 worktree 无分支时
/// 用目录名兜底;path 已 canonicalize。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeInfo {
    pub name: String,
    pub path: PathBuf,
    pub branch: Option<String>,
}
```

`cli.rs` 实现三方法:

```rust
    async fn worktree_list(&self, repo: &Path) -> Result<Vec<WorktreeInfo>, NexusError> {
        let out = self.run(Some(repo), &["worktree", "list", "--porcelain"]).await?;
        Ok(parse_worktree_porcelain(&out))
    }

    async fn worktree_create(&self, repo: &Path, name: &str, base_ref: Option<&str>) -> Result<(), NexusError> {
        let path = repo.join(".nx-worktrees").join(name);
        let mut args: Vec<String> = vec![
            "worktree".into(), "add".into(), "-b".into(), name.into(),
            path.to_string_lossy().into(),
        ];
        if let Some(b) = base_ref {
            args.push(b.to_string());
        }
        let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        self.run(Some(repo), &arg_refs).await?;
        Ok(())
    }

    async fn worktree_remove(&self, repo: &Path, path: &Path) -> Result<(), NexusError> {
        self.run(
            Some(repo),
            &["worktree", "remove", &path.to_string_lossy()],
        )
        .await?;
        Ok(())
    }
```

porcelain 解析(模块级函数,`#[cfg(test)]` 直测):

```rust
/// porcelain 契约:块以空行分隔;行关键词 worktree/HEAD/branch/bare/detached。
/// 只认关键词,未知行跳过(git 小版本变动不炸);bare 块跳过。
pub(crate) fn parse_worktree_porcelain(out: &str) -> Vec<WorktreeInfo> {
    let mut result = Vec::new();
    let mut path: Option<PathBuf> = None;
    let mut branch: Option<String> = None;
    let mut bare = false;
    let mut flush = |path: &mut Option<PathBuf>, branch: &mut Option<String>, bare: &mut bool, result: &mut Vec<WorktreeInfo>| {
        if let Some(p) = path.take() {
            if !*bare {
                let name = branch
                    .clone()
                    .map(|b| b.trim_start_matches("refs/heads/").to_string())
                    .filter(|b| !b.is_empty())
                    .or_else(|| p.file_name().map(|f| f.to_string_lossy().into_owned()));
                result.push(WorktreeInfo { name: name.unwrap_or_default(), path: p, branch: branch.take() });
            }
        }
        *branch = None;
        *bare = false;
    };
    for line in out.lines() {
        if line.is_empty() {
            flush(&mut path, &mut branch, &mut bare, &mut result);
            continue;
        }
        if let Some(p) = line.strip_prefix("worktree ") {
            flush(&mut path, &mut branch, &mut bare, &mut result);
            path = Some(PathBuf::from(p));
        } else if let Some(b) = line.strip_prefix("branch ") {
            branch = Some(b.to_string());
        } else if line == "bare" {
            bare = true;
        }
        // HEAD/detached/locked/prunable 等:本场景不需要,跳过
    }
    flush(&mut path, &mut branch, &mut bare, &mut result);
    result
}
```

`worktree.rs`:

```rust
// WorktreeManager:命名规范 + 路径决策 + 业务编排 + worktree://changed 事件。
// 事件缝与 SessionManager 同型:构造注入 sink,core 不知道 tauri。
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;

use super::ops::{GitOps, WorktreeInfo};
use crate::error::NexusError;

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum WorktreeChange {
    Created,
    Removed,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeChanged {
    pub repo_path: PathBuf,
    pub change: WorktreeChange,
}

pub type WorktreeEventSink = Arc<dyn Fn(WorktreeChanged) + Send + Sync>;

pub struct WorktreeManager {
    ops: Arc<dyn GitOps>,
    events: WorktreeEventSink,
}

impl WorktreeManager {
    pub fn new(ops: Arc<dyn GitOps>, events: WorktreeEventSink) -> Self {
        Self { ops, events }
    }

    /// 创建 nexus 命名规范的 worktree(分支 = 目录 = name)。
    pub async fn create(
        &self,
        repo: &Path,
        provider: &str,
        base_ref: Option<&str>,
    ) -> Result<WorktreeInfo, NexusError> {
        let name = crate::ids::WorktreeName::generate(provider, now_epoch_secs());
        self.ops.worktree_create(repo, name.as_str(), base_ref).await?;
        let path = repo.join(".nx-worktrees").join(name.as_str());
        let path = path.canonicalize().unwrap_or(path);
        (self.events)(WorktreeChanged {
            repo_path: repo.to_path_buf(),
            change: WorktreeChange::Created,
        });
        Ok(WorktreeInfo {
            name: name.to_string(),
            branch: Some(name.to_string()),
            path,
        })
    }

    pub async fn list(&self, repo: &Path) -> Result<Vec<WorktreeInfo>, NexusError> {
        self.ops.worktree_list(repo).await
    }

    /// 按 name 移除;delete_branch 同时删本地分支;末尾 prune 清残留元数据。
    pub async fn remove(
        &self,
        repo: &Path,
        name: &str,
        delete_branch: bool,
    ) -> Result<(), NexusError> {
        let list = self.ops.worktree_list(repo).await?;
        let target = list
            .into_iter()
            .find(|w| w.name == name)
            .ok_or_else(|| NexusError::GitCommand {
                cmd: "worktree remove".into(),
                stderr: format!("worktree {name:?} 不存在"),
            })?;
        self.ops.worktree_remove(repo, &target.path).await?;
        if delete_branch {
            if let Err(e) = self
                .ops
                .run_branch_delete(repo, name)
                .await
            {
                log::warn!("分支删除失败(可能已合并/不存在): {e}");
            }
        }
        // prune 吸收失败:残留元数据无害
        let _ = self.ops.run_prune(repo).await;
        (self.events)(WorktreeChanged {
            repo_path: repo.to_path_buf(),
            change: WorktreeChange::Removed,
        });
        Ok(())
    }
}

fn now_epoch_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
```

(上面 `run_branch_delete`/`run_prune` 若不想进 trait(它们是实现细节),改法:GitOps trait 不加;GitCliOps 上定义 `pub async fn branch_delete(&self, repo, name)` / `pub async fn prune(&self, repo)` 具体方法,WorktreeManager 持 `Arc<GitCliOps>`?——**不行**,manager 必须只依赖 trait(缝的意义)。**正解**:把这两个操作并入 trait 的 `worktree_remove` 语义——`worktree_remove(repo, path, delete_branch)`,CLI 实现内部串联 remove/branch -D/prune。执行时按此简化:trait 方法签名 `async fn worktree_remove(&self, repo: &Path, path: &Path, delete_branch: bool) -> Result<(), NexusError>`,Task 7 的 trait 定义同步这一参(此为计划内修正,以本任务签名为准),WorktreeManager::remove 只做"查路径 + 调它 + emit"。)

- [ ] **Step 4: 绿 + porcelain 单测** — `cargo test -p nexus-core --test worktree_manager`;给 `parse_worktree_porcelain` 补 3 条内联单测(主 worktree + 一个分支 worktree 的标准输出快照、bare 块、detached 无 branch 块)。
- [ ] **Step 5: 门槛 + 提交**

```bash
cargo test && cargo fmt && cargo clippy --all-targets -- -D warnings
cd /Users/itsuka/CodeSpace/ItsukaNexus && git add src-tauri && git commit -m "feat(m3): gitx 域(下)——WorktreeManager 命名/增删查/porcelain 解析(TDD)

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 9: IPC 层——git/worktree 命令 + session_dispose + session 落 worktree + worktree://changed

**Files:**
- Modify: `src-tauri/Cargo.toml`(tauri-plugin-dialog = "2")
- Modify: `src-tauri/capabilities/default.json`(`"dialog:default"`)
- Create: `src-tauri/src/commands/worktree.rs`
- Modify: `src-tauri/src/commands/mod.rs`(注册新命令)
- Modify: `src-tauri/src/commands/session.rs`(session_dispose;session_create 加 repo_path/worktree_name)
- Modify: `src-tauri/src/lib.rs`(注册 dialog 插件 + manage WorktreeManager + worktree 事件 sink)
- Modify: `src-tauri/crates/nexus-core/src/agent/manager.rs`(create 加 cwd 参数;SessionSnapshot 加 repo_path/worktree_name)
- Modify: `src-tauri/crates/nexus-core/src/pty/session.rs`(PtySession::spawn 加 cwd)
- Modify: `src-tauri/crates/nexus-core/src/error.rs`(`InvalidInput(String)` 变体)

**Interfaces:**
- Consumes: Task 4 的 `dispose`、Task 8 的 `WorktreeManager`
- Produces(前端契约,Task 10 消费):
  - 命令:`git_check() -> GitCheckInfo`;`git_validate_repo { repoPath } -> RepoInfo`;`worktree_list { repoPath } -> Vec<WorktreeInfo>`;`worktree_create { repoPath, provider?, baseRef? } -> WorktreeInfo`;`worktree_remove { repoPath, name, deleteBranch } -> ()`;`session_dispose { sessionId } -> ()`
  - `session_create` 参数扩:`{ providerId, cols?, rows?, repoPath?, worktreeName? }`——worktreeName 给了则 repoPath 必给且 worktree 目录必须存在(cwd 落 worktree);两者都缺省 = M2 行为(默认 shell,cwd 继承)
  - `SessionSnapshot` 增 `repo_path: Option<String>`、`worktree_name: Option<String>`(serde camelCase;所有构造点补 None)
  - 事件 `worktree://changed` 载荷 = `WorktreeChanged`(repoPath/change)

**学习点:** CommandBuilder 的 `cwd` 是 PTY 子进程的工作目录——worktree 集成的全部秘密就是这一行;`tauri-plugin-dialog` 是前端目录选择器的后端(Rust 侧只注册,API 由 JS 包调用,Task 10 接);async 命令里 `State<'_, WorktreeManager>` 直接可用(manager 方法是 async,天然合拍)。

- [ ] **Step 1: core 侧扩展(TDD 顺序:先 manager 测试)**

`error.rs` 追加:

```rust
    /// 命令参数组合非法(如 worktree_name 没配 repo_path、worktree 不存在)
    #[error("参数无效: {0}")]
    InvalidInput(String),
```

`pty/session.rs` 的 `spawn` 签名加 `cwd: Option<&Path>`:

```rust
    pub fn spawn(
        program: &str,
        args: &[&str],
        cols: u16,
        rows: u16,
        cwd: Option<&Path>,
    ) -> Result<(Self, Box<dyn Child + Send>), NexusError> {
        // ...openpty 不变...
        let mut cmd = CommandBuilder::new(program);
        cmd.args(args);
        if let Some(c) = cwd {
            cmd.cwd(c);
        }
        cmd.env("TERM", "xterm-256color");
        // ...其余不变...
    }
```

`manager.rs`:
- `create` 签名:`pub async fn create(&self, provider_id: &str, cols: u16, rows: u16, launch: Option<&LaunchSpec>) -> Result<SessionId, NexusError>`,其中(放 manager.rs,快照也带出去):

```rust
/// 会话落点(spec §1.4 session_create 的 repo_path/worktree_name 消费端)
#[derive(Debug, Clone)]
pub struct LaunchSpec {
    pub cwd: PathBuf,
    pub repo_path: Option<String>,
    pub worktree_name: Option<String>,
}
```

`create` 内部:`PtySession::spawn(&default_shell(), &[], cols, rows, launch.map(|l| l.cwd.as_path()))?`;SessionSnapshot 新增两字段从 launch 填(无则 None)。
- `state.rs` 的 `SessionSnapshot` 加 `pub repo_path: Option<String>`、`pub worktree_name: Option<String>`(camelCase;既有测试构造点补 `None, None`)。
- 既有 create 调用点(manager 测试、lib.rs)统一补 `None` 参数。

`tests/session_manager.rs` 追加:

```rust
#[tokio::test]
async fn create_with_cwd_spawns_in_worktree_like_dir() {
    let (mgr, _sink) = manager_with_channel().await;
    let dir = tempfile::tempdir().unwrap();
    let spec = LaunchSpec {
        cwd: dir.path().to_path_buf(),
        repo_path: None,
        worktree_name: None,
    };
    let id = mgr.create("shell", 80, 24, Some(&spec)).await.unwrap();
    mgr.send_input(id, "pwd\n").await.unwrap();
    let sub = mgr.subscribe(id).unwrap();
    // 收敛等输出含 tempdir 路径(有界:总 5s deadline)
    // ...(复用文件里既有的输出收集辅助,断言 collected 包含 dir.path().canonicalize() 的字符串形态)
    let snap = mgr.list().into_iter().find(|s| s.session_id == id).unwrap();
    assert!(snap.repo_path.is_none() && snap.worktree_name.is_none());
    let _ = sub;
}
```

- [ ] **Step 2: 红→绿** — 跑 `cargo test -p nexus-core`(新测试驱动 LaunchSpec/cwd 落地)。

- [ ] **Step 3: IPC 层接线**

`commands/session.rs`:
- `session_create` 参数加 `repo_path: Option<String>, worktree_name: Option<String>`;构造 LaunchSpec:

```rust
    let launch = match (&repo_path, &worktree_name) {
        (Some(repo), Some(name)) => {
            let wt_dir = std::path::PathBuf::from(repo)
                .join(".nx-worktrees")
                .join(name);
            let canonical = wt_dir
                .canonicalize()
                .map_err(|e| format!("worktree 不存在 {}: {e}", wt_dir.display()))?;
            Some(nexus_core::agent::manager::LaunchSpec {
                cwd: canonical,
                repo_path: Some(repo.clone()),
                worktree_name: Some(name.clone()),
            })
        }
        (None, Some(_)) => {
            return Err("参数无效: 指定 worktreeName 时必须同时指定 repoPath".into());
        }
        _ => None,
    };
    let id = state.create(&provider_id, cols.unwrap_or(80), rows.unwrap_or(24), launch.as_ref())
        .await
        .map_err(|e| e.to_string())?;
```

- 新增:

```rust
#[tauri::command]
async fn session_dispose(
    state: State<'_, SessionManager>,
    session_id: String,
) -> Result<(), String> {
    let id = parse_id(session_id)?;
    state.dispose(id).map_err(|e| e.to_string())
}
```

`commands/worktree.rs`(新):

```rust
// git/worktree 命令层:薄封装 + worktree://changed 事件的 IPC 端在 lib.rs。
use tauri::State;

use nexus_core::gitx::ops::{GitCheckInfo, RepoInfo, WorktreeInfo};
use nexus_core::gitx::worktree::WorktreeManager;

#[tauri::command]
pub async fn git_check(state: State<'_, WorktreeManager>) -> Result<GitCheckInfo, String> {
    Ok(state.check().await)
}

#[tauri::command]
pub async fn git_validate_repo(
    state: State<'_, WorktreeManager>,
    repo_path: String,
) -> Result<RepoInfo, String> {
    state
        .validate_repo(std::path::Path::new(&repo_path))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn worktree_list(
    state: State<'_, WorktreeManager>,
    repo_path: String,
) -> Result<Vec<WorktreeInfo>, String> {
    state
        .list(std::path::Path::new(&repo_path))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn worktree_create(
    state: State<'_, WorktreeManager>,
    repo_path: String,
    provider: Option<String>,
    base_ref: Option<String>,
) -> Result<WorktreeInfo, String> {
    state
        .create(
            std::path::Path::new(&repo_path),
            provider.as_deref().unwrap_or("shell"),
            base_ref.as_deref(),
        )
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn worktree_remove(
    state: State<'_, WorktreeManager>,
    repo_path: String,
    name: String,
    delete_branch: bool,
) -> Result<(), String> {
    state
        .remove(std::path::Path::new(&repo_path), &name, delete_branch)
        .await
        .map_err(|e| e.to_string())
}
```

(注:WorktreeManager 需要暴露 `check/validate_repo` 的转发方法——内部委托 `self.ops`;在 Task 8 的 worktree.rs 上补 `pub async fn check(&self)` 与 `pub async fn validate_repo(&self, path)`,各一行委托。)

`lib.rs` 的 setup 里追加(dialog 插件注册放 Builder 链):

```rust
        .plugin(tauri_plugin_dialog::init())
        // ...setup 内:
            // worktree 事件缝的 IPC 端:WorktreeChanged → emit
            let wt_handle: AppHandle = app.handle().clone();
            let wt_events = Arc::new(move |ev: WorktreeChanged| {
                let _ = wt_handle.emit("worktree://changed", ev);
            });
            let ops: Arc<dyn GitOps> = Arc::new(GitCliOps::new());
            app.manage(WorktreeManager::new(ops, wt_events));
```

`generate_handler!` 追加 `commands::git_check, commands::git_validate_repo, commands::worktree_list, commands::worktree_create, commands::worktree_remove, commands::session_dispose`。`capabilities/default.json` permissions 加 `"dialog:default"`。

- [ ] **Step 4: 全量验证 + 提交**

```bash
cargo test && cargo fmt && cargo clippy --all-targets -- -D warnings
cd /Users/itsuka/CodeSpace/ItsukaNexus && git add -A src-tauri && git commit -m "feat(m3): IPC——git/worktree 命令 + session_dispose + session 落 worktree + worktree://changed

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 10: 前端——NewSessionDialog + worktreeStore + 退出反馈 + dispose 接线

**Files:**
- Modify: `package.json`(`pnpm add @tauri-apps/plugin-dialog`)
- Modify: `src/ipc/types.ts`(GitCheckInfo/RepoInfo/WorktreeInfo/WorktreeChanged;SessionSnapshot 加可选 repoPath/worktreeName;PtyChunk 注释修正)
- Modify: `src/ipc/commands.ts`(gitCheck/gitValidateRepo/worktreeList/worktreeCreate/worktreeRemove/sessionDispose;sessionCreate 扩参)
- Modify: `src/ipc/events.ts`(onWorktreeChanged)
- Create: `src/stores/worktreeStore.ts`
- Create: `src/features/launch/NewSessionDialog.tsx`
- Modify: `src/App.tsx`(启动 git_check 状态条;handleNew 走 Dialog;handleClose 接 dispose/确认)
- Modify: `src/features/terminal/terminalManager.ts`(closed 门禁 + 头注释修正)
- Modify: `src/features/terminal/TerminalPane.tsx`(订阅状态 → 退出遮罩 + setClosed)
- Modify: `src/features/terminal/TerminalTabs.tsx`(tab 显示 worktree 短名)

**Interfaces:**
- Consumes: Task 6 的 shadcn 组件、Task 9 的 IPC 契约
- Produces: M3 UI 形态——NewSessionDialog(repo 手输 + dialog 目录浏览 + git 校验状态 + 基线 ref + "创建 worktree"开关)、无 git 引导条、关 tab 的 AlertDialog(运行中确认 + 可选删 worktree)、退出 pane 灰遮罩 + 输入门禁、tab 副标题显示 worktree 名

**学习点:** `@tauri-apps/plugin-dialog` 的 `open({ directory: true })` 是原生目录选择器(能力由 capabilities 的 `dialog:default` 放行);xterm.js 没有 readonly 属性——"只读终端"是 onData 门禁 + 视觉遮罩的组合,这是终端嵌入 UI 的通用做法。

- [ ] **Step 1: 类型与 IPC 封装**

`types.ts` 追加/修改:

```typescript
/** git_check 返回(gitx/ops.rs GitCheckInfo) */
export interface GitCheckInfo {
  available: boolean;
  version: string | null;
  path: string | null;
  worktreeSupported: boolean;
}

/** git_validate_repo 返回(gitx/ops.rs RepoInfo) */
export interface RepoInfo {
  root: string;
  currentBranch: string | null;
  isClean: boolean;
}

/** worktree_list / worktree_create 返回(gitx/ops.rs WorktreeInfo) */
export interface WorktreeInfo {
  name: string;
  path: string;
  branch: string | null;
}

/** worktree://changed 载荷(gitx/worktree.rs WorktreeChanged) */
export interface WorktreeChanged {
  repoPath: string;
  change: "created" | "removed";
}
```

`SessionSnapshot` 加 `repoPath?: string | null; worktreeName?: string | null;`(Rust 侧 `Option` 序列化 null)。`PtyChunk` 注释修正(过时措辞清理,必办 #6):

```typescript
/** session_attach 输出流帧(lib.rs PtyChunk)。
 *  seq = 0 为 replay 帧(每条订阅至多一条、先行);实时帧从 1 起按会话单调,
 *  跨订阅不回退。接缝经 Rust 侧临界段原子化(I-1)不重复不丢失;
 *  seq 过滤仅作防御性保留。 */
```

`commands.ts` 追加:

```typescript
export function gitCheck(): Promise<GitCheckInfo> {
  return invoke<GitCheckInfo>("git_check");
}
export function gitValidateRepo(repoPath: string): Promise<RepoInfo> {
  return invoke<RepoInfo>("git_validate_repo", { repoPath });
}
export function worktreeList(repoPath: string): Promise<WorktreeInfo[]> {
  return invoke<WorktreeInfo[]>("worktree_list", { repoPath });
}
export function worktreeCreate(
  repoPath: string,
  provider?: string,
  baseRef?: string
): Promise<WorktreeInfo> {
  return invoke<WorktreeInfo>("worktree_create", {
    repoPath,
    provider: provider ?? null,
    baseRef: baseRef ?? null,
  });
}
export function worktreeRemove(
  repoPath: string,
  name: string,
  deleteBranch: boolean
): Promise<void> {
  return invoke<void>("worktree_remove", { repoPath, name, deleteBranch });
}
export function sessionDispose(sessionId: string): Promise<void> {
  return invoke<void>("session_dispose", { sessionId });
}
```

`sessionCreate` 扩参(可选,向后兼容):

```typescript
export function sessionCreate(
  providerId: string,
  cols: number,
  rows: number,
  opts?: { repoPath?: string; worktreeName?: string }
): Promise<SessionCreated> {
  return invoke<SessionCreated>("session_create", {
    providerId,
    cols,
    rows,
    repoPath: opts?.repoPath ?? null,
    worktreeName: opts?.worktreeName ?? null,
  });
}
```

`events.ts` 追加:

```typescript
/** worktree://changed:增删联动(全局事件) */
export function onWorktreeChanged(
  cb: (ev: WorktreeChanged) => void
): Promise<UnlistenFn> {
  return safeListen<WorktreeChanged>("worktree://changed", cb);
}
```

- [ ] **Step 2: worktreeStore**

```typescript
// worktree 域 store(spec §1.2):repo 校验结果 + worktree 列表缓存,
// worktree://changed 事件驱动刷新(M3 最小形态:M4 fleet 再扩展)。
import { create } from "zustand";
import {
  gitValidateRepo,
  worktreeList,
  worktreeRemove,
} from "../ipc/commands";
import type { RepoInfo, WorktreeInfo } from "../ipc/types";

interface RepoEntry {
  info: RepoInfo | null;
  worktrees: WorktreeInfo[];
}

interface WorktreeState {
  repos: Record<string, RepoEntry>;
  /** 校验并加载一个 repo(目录选择/手输后调用) */
  loadRepo: (path: string) => Promise<void>;
  refresh: (path: string) => Promise<void>;
  /** 事件联动:同名 worktree 列表失效重拉 */
  applyChange: (ev: { repoPath: string }) => void;
  removeWorktree: (
    repoPath: string,
    name: string,
    deleteBranch: boolean
  ) => Promise<void>;
}

export const useWorktrees = create<WorktreeState>((set, get) => ({
  repos: {},
  loadRepo: async (path) => {
    try {
      const info = await gitValidateRepo(path);
      const worktrees = await worktreeList(path).catch(() => []);
      set((st) => ({ repos: { ...st.repos, [path]: { info, worktrees } } }));
    } catch (e) {
      console.error("[worktree] 校验失败", path, e);
      set((st) => ({ repos: { ...st.repos, [path]: { info: null, worktrees: [] } } }));
    }
  },
  refresh: async (path) => {
    const worktrees = await worktreeList(path).catch(() => []);
    set((st) => ({
      repos: { ...st.repos, [path]: { info: st.repos[path]?.info ?? null, worktrees } },
    }));
  },
  applyChange: (ev) => {
    if (get().repos[ev.repoPath]) void get().refresh(ev.repoPath);
  },
  removeWorktree: async (repoPath, name, deleteBranch) => {
    await worktreeRemove(repoPath, name, deleteBranch);
    await get().refresh(repoPath);
  },
}));
```

- [ ] **Step 3: NewSessionDialog**

`src/features/launch/NewSessionDialog.tsx`(shadcn Dialog;核心逻辑,样式从简):

```tsx
// 新建会话对话框:repo 选择(手输 + 原生目录浏览)+ git 校验 + 可选建 worktree。
// 提交流程:worktreeCreate(勾选时)→ sessionCreate(cwd 落 worktree)。
import { useCallback, useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";

import { sessionCreate, worktreeCreate } from "@/ipc/commands";
import { useSessions } from "@/stores/sessionsStore";
import { useWorktrees } from "@/stores/worktreeStore";

interface Props {
  open: boolean;
  onOpenChange: (v: boolean) => void;
  onError: (msg: string) => void;
}

export default function NewSessionDialog({ open: isOpen, onOpenChange, onError }: Props) {
  const [repoPath, setRepoPath] = useState("");
  const [useWorktree, setUseWorktree] = useState(true);
  const [baseRef, setBaseRef] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const add = useSessions((s) => s.add);
  const repo = useWorktrees((s) => (repoPath ? s.repos[repoPath] : undefined));

  // 打开且路径非空时校验(防抖从简:路径变化即重校验)
  useEffect(() => {
    if (isOpen && repoPath) void useWorktrees.getState().loadRepo(repoPath);
  }, [isOpen, repoPath]);

  const browse = useCallback(async () => {
    const picked = await open({ directory: true, multiple: false });
    if (typeof picked === "string") setRepoPath(picked);
  }, []);

  const submit = useCallback(async () => {
    setSubmitting(true);
    try {
      let worktreeName: string | undefined;
      if (useWorktree && repo?.info) {
        const wt = await worktreeCreate(repoPath, "shell", baseRef || undefined);
        worktreeName = wt.name;
      }
      const created = await sessionCreate("shell", 80, 24, {
        repoPath: repo?.info ? repoPath : undefined,
        worktreeName,
      });
      add({
        sessionId: created.sessionId,
        state: created.state,
        startedAtMs: Date.now(),
        exitCode: null,
        pid: null,
        repoPath: repo?.info ? repoPath : null,
        worktreeName: worktreeName ?? null,
      });
      onOpenChange(false);
    } catch (e) {
      onError(`创建会话失败:${String(e)}`);
    } finally {
      setSubmitting(false);
    }
  }, [useWorktree, repo, repoPath, baseRef, add, onOpenChange, onError]);

  const repoState = !repoPath
    ? null
    : repo?.info
      ? `${repo.info.currentBranch ?? "detached"} · ${repo.info.isClean ? "干净" : "有未提交变更"}`
      : "不是有效的 git 仓库";

  return (
    <Dialog open={isOpen} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>新建会话</DialogTitle>
        </DialogHeader>
        <div className="grid gap-4 py-2">
          <div className="grid gap-2">
            <Label htmlFor="repo">仓库路径(可选)</Label>
            <div className="flex gap-2">
              <Input
                id="repo"
                value={repoPath}
                placeholder="/path/to/repo"
                onChange={(e) => setRepoPath(e.target.value)}
              />
              <Button variant="secondary" onClick={() => void browse()}>
                浏览…
              </Button>
            </div>
            {repoState && <p className="text-xs text-muted-foreground">{repoState}</p>}
          </div>
          <div className="flex items-center gap-2">
            <Checkbox
              id="wt"
              checked={useWorktree && !!repo?.info}
              disabled={!repo?.info}
              onCheckedChange={(v) => setUseWorktree(v === true)}
            />
            <Label htmlFor="wt">在 nexus worktree 中打开(默认建在 .nx-worktrees/)</Label>
          </div>
          <div className="grid gap-2">
            <Label htmlFor="base">基线 ref(可选,默认 HEAD)</Label>
            <Input id="base" value={baseRef} placeholder="main" onChange={(e) => setBaseRef(e.target.value)} />
          </div>
        </div>
        <DialogFooter>
          <Button onClick={() => void submit()} disabled={submitting}>
            {submitting ? "创建中…" : "创建"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
```

- [ ] **Step 4: App 接线(git 状态条/Dialog/handleClose)+ 退出反馈**

`App.tsx`:
- 启动 effect 里追加 `gitCheck()`:`setGitInfo(...)`;顶栏右侧渲染:不可用 → `未检测到 git——worktree 功能不可用(请安装 git ≥ 2.20)` 黄色提示条(完成标准④的 UI 端);可用 → 灰字 `git {version}`。
- `handleNew` 改为打开 `<NewSessionDialog open ... />`(state 控制);error 状态由 Dialog 的 onError 回传顶栏既有错误条。
- `handleClose` 重写(关 tab 即删 + 确认):

```tsx
  // 关 tab(必办#2 关 tab 即删):
  // - 终态:sessionDispose(Rust 侧删条目)+ 本地移除
  // - 运行中:AlertDialog 确认「停止并关闭」;绑了 worktree 的附选「同时删除 worktree」
  //   → stop(等 Exit)→ 可选 worktreeRemove → sessionDispose
  const [confirmClose, setConfirmClose] = useState<{
    id: string;
    worktree?: { repoPath: string; name: string };
  } | null>(null);
  const [alsoRemoveWt, setAlsoRemoveWt] = useState(true);

  const handleClose = useCallback((id: string) => {
    const snap = useSessions.getState().sessions[id];
    if (!snap) return;
    if (snap.state === "exited" || snap.state === "failed") {
      void sessionDispose(id)
        .catch((e) => console.error("[app] dispose 失败", e))
        .finally(() => {
          disposeEntry(id);
          closeTab(id);
        });
      return;
    }
    setConfirmClose({
      id,
      worktree:
        snap.repoPath && snap.worktreeName
          ? { repoPath: snap.repoPath, name: snap.worktreeName }
          : undefined,
    });
  }, [closeTab]);

  const confirmStopAndClose = useCallback(async () => {
    if (!confirmClose) return;
    const { id, worktree } = confirmClose;
    setConfirmClose(null);
    try {
      await stopSession(id);
    } catch {
      /* 竞态:以 store 终态为准,继续收尾 */
    }
    if (worktree && alsoRemoveWt) {
      await worktreeRemove(worktree.repoPath, worktree.name, true).catch((e) =>
        setError(`worktree 清理失败:${String(e)}`)
      );
    }
    await sessionDispose(id).catch(() => {}); // stop 后已终态;失败不阻本地收尾
    disposeEntry(id);
    closeTab(id);
  }, [confirmClose, alsoRemoveWt, closeTab]);
```

(AlertDialog 的 JSX:标题「停止并关闭会话?」,worktree 存在时渲染 Checkbox「同时删除 worktree(分支与目录)」绑 alsoRemoveWt,取消/确认两个按钮——shadcn `AlertDialog` 标准用法。)

- 全局事件订阅追加 `onWorktreeChanged((ev) => useWorktrees.getState().applyChange(ev))`。

`terminalManager.ts`(头注释修正 + closed 门禁,必办 #6):

```typescript
// React 外的常驻终端实例注册表(spec §1.5 性能红线):
// - PTY 输出经 Channel 直达 term.write,不进 React state/渲染;
// - 实例生命周期与组件挂载解耦:tab 只隐藏不卸载,关 tab 才真正 dispose;
// - 每个 entry 一条 Channel(attach 幂等守卫);seq 过滤为防御性保留
//   (接缝已由 Rust 侧临界段原子化,I-1)。
```

`Entry` 加 `closed: boolean`(初始 false);`onData` 回调改:

```typescript
  terminal.onData((data) => {
    const e = entries.get(id);
    if (!e || e.closed) return; // 退出/失败后输入门禁(xterm 无 readonly)
    void sessionSendInput(id, data).catch(logInvokeError("session_send_input"));
  });
```

新增导出:`export function setClosed(id: string, closed: boolean): void { const e = entries.get(id); if (e) e.closed = closed; }`

`TerminalPane.tsx`:订阅该会话状态驱动遮罩与门禁:

```tsx
import { useSessions } from "../../stores/sessionsStore";
import { setClosed } from "./terminalManager";
// 组件内:
  const state = useSessions((s) => s.sessions[sessionId]?.state);
  const terminal = state === "running" || state === "stopping" || state === undefined;
  useEffect(() => {
    setClosed(sessionId, !terminal);
  }, [sessionId, terminal]);
// 容器 div 外再包一层相对定位 wrapper,渲染:
  {!terminal && (
    <div className="absolute inset-0 z-10 flex items-center justify-center bg-black/60 text-sm text-gray-300">
      会话已结束(输入已禁用,可关闭 tab)
    </div>
  )}
```

`TerminalTabs.tsx`:tab 在序号后显示 worktree 短名(有则):

```tsx
  const wtShort = snap.worktreeName
    ? snap.worktreeName.split("/").pop()
    : null;
  // 序号 span 后追加:
  {wtShort && (
    <span className="text-muted-foreground" title={snap.worktreeName}>
      {wtShort.length > 16 ? `${wtShort.slice(0, 15)}…` : wtShort}
    </span>
  )}
```

- [ ] **Step 5: 构建 + 提交**

```bash
pnpm build
git add -A && git commit -m "feat(m3): NewSessionDialog/worktreeStore/关tab即删/退出遮罩与输入门禁

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 11: 集成测试——session × worktree 全链路

**Files:**
- Create: `src-tauri/crates/nexus-core/tests/session_worktree.rs`

**Interfaces:**
- Consumes: Task 4/8/9 的 manager(带 LaunchSpec)+ WorktreeManager
- Produces: 钉住"选 repo → 建 worktree → 开终端落 worktree → 关会话删 worktree"的 core 级闭环(spec 完成标准①③的自动化对应物)

- [ ] **Step 1: 写测试(unix only,shell 交互)**

```rust
// session × worktree 全链路:真实 git + 真实 shell(#!cfg(unix)],
// Windows 的 worktree 纯 git 部分已由 worktree_manager.rs 三平台覆盖。
#![cfg(unix)]

mod common; // 若 helper(init_repo)需跨文件,提为 common.rs;否则就地复制

use std::sync::Arc;

use nexus_core::agent::manager::LaunchSpec;
use nexus_core::gitx::cli::GitCliOps;
use nexus_core::gitx::worktree::WorktreeManager;

#[tokio::test]
async fn session_runs_inside_created_worktree_and_cleans_up() {
    let dir = tempfile::tempdir().unwrap();
    super_init_repo(dir.path()); // 与 worktree_manager.rs 同款 init helper
    let wt_mgr = WorktreeManager::new(
        Arc::new(GitCliOps::new()),
        Arc::new(|_| {}), // 事件不检
    );
    let wt = wt_mgr.create(dir.path(), "shell", None).await.unwrap();

    let (mgr, sink_rx) = /* manager_with_channel 同款 */;
    let spec = LaunchSpec {
        cwd: wt.path.clone(),
        repo_path: Some(dir.path().to_string_lossy().into_owned()),
        worktree_name: Some(wt.name.clone()),
    };
    let id = mgr.create("shell", 100, 30, Some(&spec)).await.unwrap();

    // 快照带落点
    let snap = mgr.list().into_iter().find(|s| s.session_id == id).unwrap();
    assert_eq!(snap.worktree_name.as_deref(), Some(wt.name.as_str()));

    // 终端确实在 worktree 里(完成标准①的 core 对应物)
    mgr.send_input(id, "pwd\n").await.unwrap();
    let out = /* 有界收集输出至含 canonicalize 后的 wt.path 字符串,5s deadline */;
    let wt_canon = wt.path.canonicalize().unwrap();
    assert!(
        out.contains(&wt_canon.to_string_lossy()),
        "pwd 输出应含 worktree 路径"
    );

    // 退出 → dispose → worktree 仍在(删除是显式决策,不是自动)
    mgr.send_input(id, "exit\n").await.unwrap();
    /* 等 Exit 事件 */;
    mgr.dispose(id).unwrap();
    assert!(wt.path.is_dir(), "dispose 不删 worktree(清理走 worktree_remove)");

    // 显式清理(完成标准③)
    wt_mgr.remove(dir.path(), &wt.name, true).await.unwrap();
    assert!(!wt.path.exists());
    let _ = sink_rx;
}
```

(执行者注意:`manager_with_channel` 与输出收集辅助已在 `session_manager.rs` 成型——提为 `tests/common/mod.rs` 共享,或复制;两文件都在 `#![cfg(unix)]` 下。)

- [ ] **Step 2: 红→绿→门槛**

```bash
cargo test -p nexus-core && cargo test && cargo fmt && cargo clippy --all-targets -- -D warnings
```

- [ ] **Step 3: 提交**

```bash
git add src-tauri && git commit -m "test(m3): session×worktree 全链路——落点/快照/清理闭环

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 12: M3 端到端验收(spec 完成标准①-⑥)+ PR

- [ ] **Step 1(标准①)**:`pnpm tauri dev` → 新建会话对话框选一个真实 repo → 默认勾选 worktree → 创建 → 终端 `pwd` 显示 `.nx-worktrees/nexus/shell-...` 路径;tab 显示 worktree 短名。
- [ ] **Step 2(标准②)**:在该 worktree 终端里 `git status` / `echo x > f.txt && git add . && git commit -m t` 正常;外部终端 `git -C <repo> worktree list` 显示一致条目。
- [ ] **Step 3(标准③)**:关闭该会话 → AlertDialog 出现 → 勾「同时删除 worktree」→ 确认后 `git worktree list` 无此条目、`.nx-worktrees/` 目录消失、分支已删。
- [ ] **Step 4(标准④)**:`git_check` UI 端——正常机显示版本;自动化对应物 `check_with_missing_binary_reports_unavailable` 已绿;补一手动法:临时 `PATH=/usr/bin:/bin:/usr/sbin:/sbin pnpm tauri dev`(若 git 不在这些目录)观察顶部引导条。
- [ ] **Step 5(标准⑤)**:`cargo test -p nexus-core`(临时 repo 上的 worktree 增删查 + session×worktree 全绿)。
- [ ] **Step 6(标准⑥)**:Windows 侧——本地不做;靠 `paths_with_spaces_and_cjk_work` 在 CI 三平台矩阵跑绿 + `gh pr checks` 四项全绿确认。
- [ ] **Step 7: M2 基线不回退**:多 tab 并行/刷新恢复/配置保留/强杀 3 秒内 Failed 四项目检(暗色新皮肤下)。
- [ ] **Step 8: 全量回归 + 推送 + PR**

```bash
cargo test && cargo fmt && cargo clippy --all-targets -- -D warnings && pnpm build
git push -u origin m3-worktree-workspace   # 注意走代理:git -c http.proxy=http://127.0.0.1:7890 push ...
gh pr create ...   # 标题 feat(m3): git worktree + workspace 拆分 + M2 必办清理
gh pr checks
```

---

## Self-Review 记录

- **Spec 覆盖**:workspace 拆分→T1(§1.1/§1.2);命令目录化→T2(§1.2);kill 序列→T3(§1.3);会话回收→T4(§1.3);config 加固→T5(必办#4);git_check/validate→T7;worktree 增删查+命名规范+porcelain→T8;IPC 全命令+worktree://changed+session 落 worktree→T9(§1.4);RepoPicker/dialog/退出反馈/关 tab 即删→T10;完成标准①③core 对应物→T11;六项验收→T12。M2 必办六条映射表见计划头。✅
- **有意裁剪(非遗漏)**:worktree_diff/merge 归 M5(spec 明示);AgentProvider/launch_fleet 归 M4;TS 类型从 Rust 生成(specta)未纳入——手写镜像 + serde 钉住测试(T2)维持;`worktree://changed` 的 Merged 变体 M5 再加;WaitingInput 仍 v1.1。
- **类型一致性**:LaunchSpec{T1 不存在,T9 引入并在 T11 复用};worktree_remove trait 签名在 T8 修正为三参(repo, path, delete_branch)——T7 的 trait 定义按 T8 版为准(计划内修正已注明);WorktreeInfo{name,path,branch} T8 定义、T9 IPC/T10 TS 同构;SessionSnapshot 增两字段在 T9 落、T10 TS 加可选;GitCheckInfo.worktreeSupported 为 spec 表格的超集(UI gate 需要)。✅
- **已知风险**:① T1 的 workspace 迁移若遇 `itsukanexus_lib` 隐患(tests/examples 引用点),以 `grep -rn itsukanexus_lib` 全量清点为准;② shadcn CLI 非交互参数随版本漂移,已给手写退路;③ busy-shell 强杀测试(T3)依赖 shell job control 行为,CI Linux 默认 bash、macOS zsh,均有 job control;若 CI 出现平台差异,允许在测试内按 `SHELL` 环境适配但不得弱化断言(Exit 必达);④ timefmt 的已知时刻断言先 `date -u -r` 核对再钉死。✅

