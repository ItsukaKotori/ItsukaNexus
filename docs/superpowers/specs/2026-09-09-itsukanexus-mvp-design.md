# ItsukaNexus MVP 实现架构设计

> 版本：v1.2（2026-09-13，M4 重定向：按 Open Design 原型（astra-ade-prototype-v2）重构为"原型 UI 骨架"里程碑——四栏 workbench/项目注册表/亮色唯一主题；AgentProvider 顺延 M5、diff/merge 顺延 M6；参照 orca 源码（本地 `/Users/itsuka/CodeSpace/orca`）的实现模式，技术栈不变）
> 历史：v1.1（2026-09-12，M3 细化修订：进程组 kill 序列、session_dispose 会话回收、事件载荷 camelCase 对齐、UI 库选型 shadcn/ui + Tailwind）
> 状态：已批准
> 参考：[stablyai/orca](https://github.com/stablyai/orca)（形态参考：多 CLI agent 并行编排 + 每 agent 独立 worktree；UI 模式参考：tab 框架/porcelain v2 状态解析/主题令牌/持久化容错；其前端用 React，我们同为 React）
> 约束：开发者本人是 Rust 新手，本设计同时是一份 Rust 学习路径。技术栈（Tauri 2 + React TS + Rust 全量核心逻辑）已锁定，本文为细化设计。
> 原型：`~/Library/Application Support/Open Design/namespaces/release-stable/data/projects/a3e91f89-9d76-4a78-ae04-cf4e4b3d0501/`（astra-ade-prototype-v2.html 为视觉/布局权威，brand-spec.md 为视觉规则权威；产品名保留 ItsukaNexus，不用原型内 Astra 字样）

---

## 0. 设计原则

1. **先线程后 async**：M1 用 `std::thread` + `std::sync::mpsc` 打通 PTY（同步思维），M2 再重构为 tokio。异步是"对线程的重新表达"，先有同步心智模型再学 async 才不虚。
2. **模块边界 = 未来 crate 边界**：M0-M2 单 crate，但模块树严格按未来 workspace 拆分布局摆放，M3 的拆分变成机械搬运而非重新设计。
3. **高频数据走 Channel，低频状态走 emit**：终端输出是流，用 Tauri 2 的 `Channel<T>`；会话状态变化是事件，用全局 `emit`（侧边栏监听所有会话）。
4. **Rust 端有状态、前端无权威状态**：前端只是 Rust 状态的投影，前端崩溃/热重载后通过 `session_list` + `session_attach`（带 replay buffer）无损恢复。
5. **一切外部能力都有 trait 缝**：AgentProvider（CLI → 未来 ACP/MCP）、GitOps（CLI → 未来 git2），v1 各只有一个实现，但接缝先留好。

---

## 1. 整体架构

### 1.1 Workspace 划分：两阶段策略

| 阶段 | 结构 | 理由 |
|---|---|---|
| M0–M2 | 单 crate（`src-tauri`），多模块 | cargo workspace 对新手是额外一层概念，前三个月只会增加挫败感；单 crate 下 `cargo run`/`cargo test` 零心智负担 |
| M3 起 | workspace：`src-tauri`（薄 IPC 层）+ `crates/nexus-core`（领域核心，**不依赖 tauri**） | ① 核心逻辑可以脱离 GUI 用纯 `cargo test` 测（worktree 管理尤其需要集成测试）；② 编译时间分离；③ 此时已有 cargo 基础，workspace 是顺水推舟的学习内容 |

### 1.2 最终目录布局（M4 后的完整形态）

M0-M2 期间 `nexus-core` 内的模块原样放在 `src-tauri/src/` 下同路径，M3 整体搬入。M4 起前端按原型四栏 workbench 重组（§1.5），后端新增 registry 域与 gitx 状态/提交。

```
ItsukaNexus/
├── package.json                    # 前端依赖（React、xterm、zustand）
├── vite.config.ts
├── tsconfig.json
├── index.html
├── docs/
│   └── (本设计文档)
├── src/                            # ---------- 前端 ----------
│   ├── main.tsx
│   ├── App.tsx                     # 薄壳：挂 Workbench + 全局 toast
│   ├── app/
│   │   ├── Workbench.tsx           # 四栏布局（rail | 项目树 | 标签中心 | 右面板）
│   │   ├── useWorkbenchLayout.ts   # 单一布局派生钩子（orca useAppChromeLayout 模式）
│   │   └── Rail.tsx                # 图标导航：项目（M4）/ 任务·市场·设置（占位禁用）
│   ├── components/
│   │   └── ui/                     # shadcn/ui 组件（源码进仓库，M3 起）
│   ├── ipc/
│   │   ├── types.ts                # 与 Rust 侧对齐的 TS 类型（SessionSnapshot/PtyChunk/...）
│   │   ├── commands.ts             # 所有 invoke 的类型安全封装（唯一入口）
│   │   └── events.ts               # 所有 listen 的封装（唯一入口）
│   ├── stores/
│   │   ├── sessionsStore.ts        # zustand：会话快照表 + 派生选择器（数据）
│   │   ├── tabStore.ts             # zustand：标签表（视图）——单一 tab 模型，kind 可扩展
│   │   ├── projectStore.ts         # zustand：项目注册表镜像 + 每项目 git 状态/worktree
│   │   └── configStore.ts
│   ├── features/
│   │   ├── terminal/
│   │   │   ├── TerminalPane.tsx    # 容器：ResizeObserver + 挂载 xterm DOM（常驻只藏不卸）
│   │   │   ├── useTerminalSession.ts   # attach channel / write / resize 的 hook
│   │   │   └── terminalManager.ts  # xterm 实例注册表（会话 -> Terminal），React 外的常驻层
│   │   ├── tabs/
│   │   │   ├── TabStrip.tsx        # 多类型标签条（状态点/标签/kind 徽标/关闭）+「+」新建
│   │   │   └── TabBody.tsx         # 按 tab.kind 分发内容（M4 仅 terminal，其余空态占位）
│   │   ├── project/
│   │   │   ├── ProjectTree.tsx     # 树：项目(branch 徽标) > worktree > session（点击激活 tab）
│   │   │   ├── OpenDirOverlay.tsx  # 打开目录：手输 + dialog 浏览 + 最近打开
│   │   │   └── NewWorktreePopover.tsx  # 项目节点「+」：基线 ref + 建完开终端
│   │   ├── gitpanel/
│   │   │   └── GitPanel.tsx        # 右侧 Git 面板：分支/ahead·behind/变更列表/暂存/提交
│   │   ├── launch/                 # （M5：LaunchFleetDialog / AgentProfileEditor）
│   │   ├── worktree/               # （M6：DiffView / MergeDialog）
│   │   └── settings/ConfigDialog.tsx  # 现有配置对话框暂留（设置 pane 为远期）
│   ├── lib/format.ts               # 时间/字节格式化等工具
│   └── assets/
│       └── fonts/                  # Geist-Variable.woff2 + JetBrainsMono-*.woff2（本地打包）
└── src-tauri/                      # ---------- Rust ----------
    ├── Cargo.toml                  # [workspace] members = ["crates/nexus-core", "."]
    ├── tauri.conf.json
    ├── capabilities/default.json   # Tauri 2 ACL：dialog/opener 等权限
    ├── crates/
    │   └── nexus-core/
    │       ├── Cargo.toml
    │       └── src/
    │           ├── lib.rs          # 模块声明与 re-export
    │           ├── error.rs        # NexusError（thiserror）
    │           ├── ids.rs          # newtype：SessionId / WorktreeName / ProviderId / ProjectId
    │           ├── pty/
    │           │   ├── mod.rs
    │           │   ├── session.rs  # PtySession：spawn/writer/reader 线程/resize/wait
    │           │   ├── batcher.rs  # 输出合帧（时间窗 + 大小上限）
    │           │   └── decode.rs   # 增量 UTF-8 解码（跨 chunk 多字节安全）
    │           ├── agent/
    │           │   ├── mod.rs
    │           │   ├── state.rs    # SessionState 状态机 enum
    │           │   ├── provider.rs # AgentProvider trait + AgentCapabilities（M5 落地）
    │           │   ├── registry.rs # ProviderRegistry（M5 落地）
    │           │   ├── cli.rs      # CliAgentProvider（M5 唯一实现）
    │           │   ├── session.rs  # AgentSession：组合 PtySession + 状态机 + 事件
    │           │   └── manager.rs  # SessionManager：会话表 + 编排（一键 N 会话）
    │           ├── gitx/
    │           │   ├── mod.rs
    │           │   ├── ops.rs      # GitOps trait（CLI 实现的缝）
    │           │   ├── cli.rs      # GitCliOps：tokio::process 调 git，解析 porcelain
    │           │   ├── status.rs   # M4：status porcelain=v2 解析 + stage/commit（类型在 ops.rs）
    │           │   ├── worktree.rs # WorktreeManager：add/list/remove/prune + 命名规范
    │           │   └── diff.rs     # M6：diff/merge 解析
    │           ├── registry/
    │           │   ├── mod.rs      # M4：ProjectEntry 类型
    │           │   └── store.rs    # M4：projects.json 读写（schemaVersion + 单条容错 + 原子写）
    │           ├── config/
    │           │   ├── mod.rs
    │           │   ├── model.rs    # AppConfig/AgentProfile（serde）
    │           │   └── store.rs    # 原子读写（tmp + rename）
    │           └── bus.rs          # EventBus：tokio broadcast，扇出到 IPC 层
    └── src/
        ├── main.rs                 # 入口（Tauri 2 模板自带）
        ├── lib.rs                  # tauri::Builder：注册插件/命令/State 组装
        ├── state.rs                # AppState { core: NexusCore } 注入 tauri State
        └── commands/
            ├── mod.rs              # generate_handler 清单（IPC 唯一注册点）
            ├── app.rs              # app_info / git_check
            ├── session.rs          # session_*
            ├── worktree.rs         # worktree_* / git_validate_repo / git_status / git_stage / git_commit
            ├── project.rs          # M4：project_list / project_add / project_remove
            └── config.rs           # config_get / config_save
```

### 1.3 核心模块职责与并发模型

**pty::PtySession**（portable-pty 封装）
- 职责：构建 CommandBuilder（argv/env/cwd/初始尺寸）、spawn、持有 master reader（`Box<dyn Read + Send>`，阻塞 IO）与 writer、resize、`child.wait()`。
- 并发：**每个 PTY 一个专用读线程**（M1 为 `std::thread`，M2 起为 `tokio::task::spawn_blocking`——portable-pty 的 reader 是同步阻塞的，这不是妥协而是正确做法）。写端 `Arc<Mutex<Box<dyn Write + Send>>>`（M1）→ 独立写任务 + mpsc 命令（M2）。
- env 注入：`TERM=xterm-256color`、`COLORTERM=truecolor`，保证 Claude Code 等 TUI agent 在 ConPTY 下正确渲染颜色。
- **背压链（关键设计）**：reader 线程 → **有界** tokio mpsc（容量 64 条消息）→ batcher 任务（8–16ms 时间窗或 32KB 大小上限合为一帧）→ `Channel.send`。队列满时 reader 线程 `await` 在 send 上 → PTY 内核缓冲涨 → 子进程 write 阻塞 → agent 自然减速。终端字节流不可丢帧（会撕裂转义序列），所以用"有界通道 + 阻塞"而非"丢弃"。
- **replay buffer**：每会话保留最近 256KB 输出的 ring buffer，`session_attach` 时先重放，前端热重载/崩溃后终端不丢上下文。
- **kill 序列（M3）**：force stop = Unix `killpg(pgid, SIGKILL)`（子进程开 controlling terminal 即 session leader，pgid = pid，杀 shell 组）+ **drop master**（内核向前台进程组发 SIGHUP——覆盖"kill 正忙 shell"场景）→ slave 全关 → reader EOF；Windows 维持 killer + Exit 事件为真相 + join 超时兜底（ConPTY 无进程组概念，孙进程为已知局限，文档说明）。

**agent::AgentSession**（编排的最小单元）
- 组合一个 `PtySession` + 会话状态机 + 可选的 worktree 绑定，持有 `CancellationToken`。
- 自身是一个 tokio 任务（M5 起）：消费内部命令（Input/Resize/Stop），产生两类输出——字节流（给 attach 的 Channel）和状态事件（给 EventBus）。
- 子进程退出检测：单独的 `spawn_blocking` waiter 调 `child.wait()`，完成后发状态迁移事件。

**agent::SessionManager**
- `RwLock<HashMap<SessionId, SessionHandle>>`；`SessionHandle` 是瘦句柄（快照 + 命令通道），不是共享可变大对象——读写锁只保护注册表，会话内部状态由会话自己的任务独占（actor 风格，避免锁粒度地狱）。
- 提供 `launch_fleet(repo, provider, n, base)`：原子地"创建 N 个 worktree + N 个会话"，任一步失败则回滚已建部分。
- **会话回收（M3）**：终态（Exited/Failed）条目"关 tab 即删"——`session_dispose` 命令 drop 条目（replay/订阅/句柄/取消令牌全释放），运行中会话调用返回错误；终态时自动置空 subscriber 终结常驻转发任务；`list()` 按 startedAtMs 排序（刷新后 tab 序稳定）。

**agent::SessionState（状态机，Rust enum 为唯一权威）**

```
Created ──start()──> Running ──user/orchestrator stop──> Stopping ──> Exited{code}
                        │                                      └────> Failed{error}
                        ├──子进程退出码 0──────────> Exited{0}
                        └──spawn 失败/PTY 错误────> Failed{reason}
```

- v1 说明：CLI agent 跑在 PTY 里，Rust 端看不到"agent 是否在等输入"，因此 **WaitingInput 不进 MVP 状态机**；预留为 v1.1 的启发式推断（无输出超时 + provider 提供的提示符正则），状态机 enum 里加注释占位。这是有意的简化，不是遗漏。
- 每次迁移携带 `{session_id, prev, next, at, detail?}` 广播到 EventBus。

**gitx::WorktreeManager**
- 命名规范：分支与目录统一 `nexus/<provider>-<yyMMdd-HHmmss>-<short rand>`，目录集中放 `<repo>/.nx-worktrees/`（或用户配置的父目录），便于一键清理。
- v1 全部走 git CLI（选型见 §3）。

**gitx 状态/提交（M4，参照 orca source-control 的命令构造）**
- `GitOps` 扩展三方法：`status(repo) -> GitStatus`、`stage(repo, paths | all)`、`commit(repo, message)`。
- status 命令：`git -c core.quotePath=false status --porcelain=v2 --branch --untracked-files=all`，环境变量 `GIT_OPTIONAL_LOCKS=0`（只读探测不与用户终端里的 git 抢 index.lock）；ahead/behind 从 `--branch` 头部一次折叠，免二次子进程；**条目上限 2000**，超限置 `truncated` 透传 UI（防巨量未跟踪目录）。
- porcelain v2 是机器可读稳定契约（`# branch.ab +x -y`、`1 <XY> ... <path>`、`2 <XY> ... <path><NUL><origPath>` 行），解析纪律同 worktree porcelain：只认行首关键词，未知行跳过。
- 刷新策略：面板手动刷新 + 窗口聚焦时重拉；不做文件系统 watcher、不做后台轮询（orca 的调度器属过度设计，M4 不引入）。

**registry::ProjectRegistry（M4，多项目目录持久化）**
- 数据：`{ schemaVersion: 1, projects: [{ id, name, path, addedAtMs }] }`，存 Tauri app_config_dir `projects.json`。
- 容错（借 orca zod-salvage 哲学）：根结构 `#[serde(default)]`，**单条项目损坏只丢那一条**，只有整档非 JSON 才回退空表；`schemaVersion` 为未来迁移留缝。
- 写入沿用 config store 的原子写（tmp + rename）；项目表低频变更，变更即写，不需要防抖。
- `add(path)` 先 `git_validate_repo` 校验——**仅接受 git 仓库入册**（树的 branch/worktree/session 层全依赖 git）；`remove(id)` 只删注册表记录，不动磁盘。

**events::EventBus**
- `tokio::sync::broadcast` 通道；IPC 层（src-tauri）订阅后转成 `emit`。nexus-core 不知道 tauri 的存在。

**config::store**
- JSON 存 Tauri app_config_dir；写入用"写临时文件 + rename"原子替换，防止崩溃损坏配置。

### 1.4 IPC 协议设计

#### 命令（invoke，前端 → Rust）—— MVP 完整清单

命名规范：`<域>_<动作>`，snake_case。参数/返回值全部是 serde 可序列化结构。

| 命令 | 参数 | 返回 | 引入 |
|---|---|---|---|
| `app_info` | – | `{ name, version, platform }` | M0 |
| `git_check` | – | `{ available, version, path, worktree_supported }` | M3 |
| `git_validate_repo` | `{ path }` | `RepoInfo{ root, current_branch, is_clean }` | M3 |
| `git_status` | `{ repo_path }` | `GitStatus{ branch, ahead, behind, entries, truncated }` | M4 |
| `git_stage` | `{ repo_path, paths? }` | –（无 paths = 全部暂存） | M4 |
| `git_commit` | `{ repo_path, message }` | – | M4 |
| `worktree_list` | `{ repo_path }` | `Vec<WorktreeInfo>` | M3 |
| `worktree_create` | `{ repo_path, name?, base_ref? }` | `WorktreeInfo` | M3 |
| `worktree_remove` | `{ repo_path, name, delete_branch }` | – | M3 |
| `worktree_diff` | `{ repo_path, name, base? }` | `DiffSummary` | M6 |
| `worktree_merge` | `{ repo_path, name, target_ref }` | `MergeResult` | M6 |
| `project_list` | – | `Vec<ProjectEntry>` | M4 |
| `project_add` | `{ path }` | `ProjectEntry`（入册前 git 校验，非 repo 拒绝） | M4 |
| `project_remove` | `{ project_id }` | – | M4 |
| `session_create` | `{ provider_id, repo_path?, worktree_name?, env_overrides? }` | `{ session_id, state }` | M1（M1 时 provider_id 固定传 `"shell"`，内部硬编码默认 shell；M5 变为真实 provider 注册表——**签名从 M1 起保持稳定**） |
| `session_attach` | `{ session_id, output: Channel<PtyChunk> }` | `{ replayed_bytes }` | M2 |
| `session_send_input` | `{ session_id, data }` | – | M1 |
| `session_resize` | `{ session_id, cols, rows }` | – | M1 |
| `session_stop` | `{ session_id, force }` | – | M1 |
| `session_list` | – | `Vec<SessionSnapshot>` | M2 |
| `session_dispose` | `{ session_id }` | – | M3（仅终态可删；"关 tab 即删"语义） |
| `provider_list` | – | `Vec<ProviderInfo>` | M5 |
| `config_get` / `config_save` | – / `{ config }` | `AppConfig` / – | M2 |

#### 事件与流（Rust → 前端）

**全局 emit（低频、广播给所有窗口/监听者）**，命名规范 `域://事件`：

| 事件 | 载荷 | 说明 | 引入 |
|---|---|---|---|
| `session://state` | `{ sessionId, prev, next, atMs, detail? }` | 状态机迁移；项目树 session 节点/标签状态点实时刷新 | M2 |
| `session://exit` | `{ sessionId, code }` | 可并入 state，但独立出来便于前端弱网去重 | M2 |
| `worktree://changed` | `{ repoPath, change }` | Created/Removed/Merged，多面板联动 | M3 |
| `app://error` | `{ source, message, recoverable }` | 全局 toast | M5 |

**Channel（高频流，per-session，非广播）**：

| 流 | 载荷 | 说明 |
|---|---|---|
| `session_attach` 的 `output` | `PtyChunk { sessionId, data: String, seq }` | `data` 已在 Rust 侧经**增量 UTF-8 解码**（`pty/decode.rs` 保存跨 chunk 的不完整多字节尾部），前端 `term.write()` 前无需再处理编码 |

为什么输出不用全局 emit：① emit 是广播，每帧 JSON 序列化发给所有监听者，`cat` 大文件时开销放大 N 倍；② Channel 绑定单个会话，前端订阅生命周期与 React 组件对齐；③ 官方明确 Channel 为流式场景设计。折中代价是"切 tab 重连"需要 replay，由 ring buffer 解决。

**背压与批量参数（MVP 定值，M6 可调）**：读 chunk 8KB；合帧窗口 8–16ms 或 32KB 上限；有界队列 64 帧；replay buffer 256KB。

### 1.5 React 前端结构与 xterm.js 集成

- **技术**：React 19 + Vite + TypeScript；`@xterm/xterm` + `@xterm/addon-fit` + `@xterm/addon-web-links`（xterm.js 5.x 起包名迁到 `@xterm` scope）+ `@xterm/addon-webgl`（大输出时的渲染加速，作为渐进增强，失败自动回退 DOM renderer）。
- **UI 组件（M3 起）**：**shadcn/ui + Tailwind v4**——组件源码进仓库、无运行时框架锁定。**主题（M4 决策）：亮色唯一**——原型亮色令牌落 `:root`（背景 `oklch(0.985 0 0)`、近白反色主操作色、蓝色 accent、状态色 ok/warn/run/err、git 状态色 M/A/D/U），经 `@theme inline` 映射 Tailwind 工具类；令牌全走 CSS 变量，未来加暗色零重构，但 M4 不做 `.dark` 类与切换 UI。视觉规则按原型 brand-spec：单色外壳（导航/侧栏只用中性灰）、发丝分割线（1px border）、颜色即状态、三层阴影。**字体本地打包**（桌面应用禁止 CDN）：Geist 可变字重 + JetBrains Mono 的 woff2 进 `src/assets/fonts/` 经 `@font-face` 声明。
- **布局（M4 起，四栏 workbench，原型为权威）**：`Workbench` = `Rail`（52px 图标导航：项目/任务·市场·设置占位禁用）+ `ProjectSide`（272px 项目树 + 打开目录）+ 中心 `TabStrip/TabBody`（多类型标签）+ `CtxPanel`（296px 右面板：Git 分段实现、文件分段占位）。单一 `useWorkbenchLayout` 派生钩子（orca `useAppChromeLayout` 模式）回答"哪些区域挂载/折叠"；侧栏与右面板可折叠，最小窗口宽度约 960px，不做自动降级断点；保留系统标题栏（不做自定义窗口装饰）。
- **tab 框架（M4 核心新抽象）**：`tabStore`（zustand）单一 tab 模型——`Tab = { id, kind: "terminal" | (M5+ 会话/diff/编辑器…), label, projectId?, worktreeName?, sessionId? }`。**session 是数据（sessionsStore）、tab 是视图（tabStore）**，两者解耦；M4 不做 tab 状态持久化，重启后由 `session_list` 重建终端 tab（orca 双 tab 模型并行是其最大复杂度税，我们自始只有一个）。终端标签标题取"最低空闲序号"（终端 1 关闭后复用，orca 同款）。
- **状态管理**：zustand。会话表是"一个集合 + 多处派生视图（项目树/标签条/右面板）"的典型全局单 store 场景，zustand 的 selector + 浅比较天然匹配且学习成本一晚上。
- **xterm 实例管理（关键决策，M0 起不变）**：`terminalManager.ts` 是 React 树之外的普通 TS 模块，持有 `Map<sessionId, Terminal>`。**每个会话的 Terminal 实例常驻**（切换 tab 用 CSS 隐藏/显示，挂载闩锁——任何布局切换不销毁），避免卸载重建丢失 scrollback 与 TUI 状态；React 只通过 store 订阅"哪些会话存在"，绝不把输出数据放进 React state（输出直达 `term.write`，绕过 React 渲染管线——这是性能红线）。
- **数据接入**：`useTerminalSession(sessionId)` hook 负责 `new Channel<PtyChunk>()` → `session_attach` → `onmessage` 里 `term.write(chunk.data)`；`term.onData` → `session_send_input`；`ResizeObserver` + fit addon → `session_resize`（防抖 100ms）。xterm 主题配色与亮色令牌对齐。
- **注意**：React StrictMode 下 effect 双执行会创建两个 Channel——hook 里做幂等 attach（Rust 侧对同会话重复 attach 直接替换旧 Channel）。

### 1.6 数据流图

```
[键盘] xterm onData(data: String)
   │  invoke session_send_input
   ▼
commands::session::send_input ──> SessionManager 查句柄 ──> PtyWriter.write(bytes)
   │                                                          │
   │                                                    ConPTY master (OS 缓冲)
   │                                                          ▼
   │                                            agent 进程 (claude/codex/... )
   │                                            cwd = <repo>/.nx-worktrees/nexus/xxx
   │                                                          │ 输出
   │                                                    ConPTY master read
   ▼                                                          ▼
[背压回路] reader 线程 (spawn_blocking) ──有界 mpsc(64)──> batcher 任务 (8-16ms/32KB 合帧)
   ▲  队列满则 reader 阻塞 → PTY 缓冲涨 → agent write() 变慢       │ 增量 UTF-8 解码
   │                                                    Channel<PtyChunk>.send (Tauri IPC, JSON)
   │                                                          ▼
   │                              前端 Channel.onmessage → term.write(data) → xterm 渲染
   │                              (同时写入 Rust 侧 256KB ring buffer，供重连 replay)
   │
   └── 状态流（低频）：child.wait() → AgentSession 状态机迁移 → EventBus(broadcast)
        → IPC 层 emit "session://state" → 前端 listen → zustand 更新 → 侧边栏/面板重渲染
```

---

## 2. 渐进式里程碑 M0–M6

节奏建议：每里程碑 1–3 周，以"完成标准全部打勾"为唯一出口条件，不赶日历。每个里程碑配 `examples/` 目录：先写 50–150 行独立小例验证新概念，再进主线（这是 async/PTY 学习风险的核心缓解手段）。

### M0 — 地基：模板跑通 + 第一个命令

| 项 | 内容 |
|---|---|
| 交付物 | `create-tauri-app`（React-TS 模板）生成的可运行应用；自定义命令 `app_info` 返回应用名/版本/platform 并显示在窗口里；`git init` 提交初始代码 |
| Rust 学习主题 | cargo 基础（build/run/test、Cargo.toml 依赖）；模块树与 `mod`/`pub`/`use`；所有权/借用/move、`String` vs `&str`；struct/enum/`Option`；`Result` + `?` + `match` 穷尽性；第一个 `#[derive(Serialize)]` |
| 关键技术 | Tauri 2 模板、`#[tauri::command]`、前端 `invoke`、rust-analyzer + clippy 习惯 |
| 完成标准（验证） | ① `pnpm tauri dev` 出窗口，按钮点击显示 Rust 返回的版本号；② `cargo test` 通过（哪怕一个空测试）；③ 修改 Rust 代码后热重载生效；④ clippy 无 warning |

### M1 — 单终端：PTY 全链路（同步实现）

| 项 | 内容 |
|---|---|
| 交付物 | 内嵌 xterm.js 跑通默认 shell（PowerShell）：输入回显、ANSI 颜色、resize、Ctrl+C、scrollback；`session_create/send_input/resize/stop` 四个命令；输出经 `std::thread` + `std::sync::mpsc` + 16ms 合帧后 emit（M1 先用全局 emit，M2 换 Channel——亲历两种机制差异本身就是学习目标） |
| Rust 学习主题 | `std::process::Command`（对照理解 PTY 与管道的区别）；`std::thread::spawn` 与 move 闭包、`'static` 约束初体验；`Arc<Mutex<>>` 共享 PTY writer；`std::sync::mpsc`；`BufReader`/`Vec<u8>` 字节流思维；错误处理落地——nexus-core 用 `thiserror` 定义错误、IPC 边界用 `anyhow`/`String`；生命周期第一次真用（reader 线程持有 `Box<dyn Read + Send + 'static>`） |
| 关键技术 | portable-pty 0.9、`from_utf8_lossy` 临时方案（已知缺陷：多字节字符跨 chunk 会出替换符，M2 修）、xterm + fit addon、合帧定时器 |
| 完成标准（验证） | ① 跑 `claude --version`、`git log`（分页器）、以及 vim/htop 类全屏 TUI 正常渲染无花屏；② 拖拽窗口终端跟随 resize；③ UI 停止按钮能干净杀进程（ConPTY 无僵尸句柄）；④ 终端内 `cat` 一个 2 万行文件 UI 不卡（合帧生效）；⑤ 关闭应用无 panic |

### M2 — async 重构 + 多会话 + 配置 + 恢复

| 项 | 内容 |
|---|---|
| 交付物 | M1 的线程实现重构为 tokio（spawn_blocking 桥接 PTY 读）；多 tab 并行多终端；输出流迁移到 `Channel<PtyChunk>`；`session_attach` + ring buffer replay（前端刷新页面终端内容不丢）；`session_list` 恢复；`AppConfig`（含 AgentProfile 列表）读写 + `config_get/save`；tauri-plugin-log 接入；`pty/decode.rs` 增量 UTF-8 解码替换 lossy |
| Rust 学习主题 | Future/async fn 的状态机本质；`tokio::spawn`；有界 `mpsc` 与**背压语义**；`select!`；`CancellationToken`（tokio-util）做优雅关停；`spawn_blocking` 与阻塞 IO 的边界；`Arc<RwLock>` vs `Arc<Mutex>` 取舍；`Send + Sync` 自动 trait 为什么重要；serde 完整建模 + 原子文件写 |
| 关键技术 | tokio、Tauri 2 Channel、zustand store、terminalManager（实例常驻方案） |
| 完成标准（验证） | ① 同时 4+ 终端跑不同 shell 互不串流；② 刻意写一个慢消费者观察 agent 输出被背压减速而非丢字/爆内存；③ 前端 Ctrl+R 刷新后 `session_list` + replay 恢复全部终端内容；④ 重启应用配置保留；⑤ 强杀一个子进程，侧边栏 3 秒内显示 Failed；⑥ `cargo test` 覆盖 decode.rs 的跨 chunk UTF-8 用例 |

### M3 — git worktree + workspace 拆分

| 项 | 内容 |
|---|---|
| 交付物 | cargo workspace 拆分（`nexus-core` 脱离 tauri，模块机械搬运）；GitOps：`git_check`（启动检测）、`git_validate_repo`、worktree create/list/remove；RepoPicker（tauri-plugin-dialog）；`session_create` 支持 `repo_path + worktree`，终端 cwd 落在 worktree；关会话可选清理 worktree；`worktree://changed` 联动；前端基建 shadcn/ui + Tailwind v4（含 M2 手写 UI 移植）。**M2 终审遗留必办**：Unix 进程组 kill（killpg + drop master，见 §1.3）、会话回收（`session_dispose` 关 tab 即删 + 终态断订阅）、`list()` 排序、`send_input` 写超时、config 三项加固（读错误不覆盖写/入参 clamp/命令 async 化）、终端退出视觉反馈 |
| Rust 学习主题 | cargo workspace/lib vs bin/path 依赖；`tokio::process::Command` + 管道 stdout/stderr；porcelain 输出解析——字符串处理纪律；`Path`/`PathBuf`/canonicalize 与 Windows 路径陷阱；**trait 正式入门**：定义 `GitOps` trait 只有一个 `GitCliOps` 实现；newtype ID（`SessionId(uuid)`、`WorktreeName(String)`）防字符串滥用；集成测试：`tempfile` 建临时 repo → worktree 全流程断言 |
| 关键技术 | tokio::process、tauri-plugin-dialog、tauri-plugin-opener |
| 完成标准（验证） | ① UI：选 repo → 创建 worktree → 打开终端 `pwd` 显示 worktree 路径；② 在 worktree 里 `git status`/commit 正常，外部 `git worktree list` 一致；③ 关闭会话勾选清理后分支与目录均消失；④ 系统无 git 时启动给引导提示而非 panic；⑤ `cargo test -p nexus-core` 在临时 repo 上跑通 worktree 增删查集成测试；⑥ Windows 含空格/中文路径的 repo 正常 |

### M4 — 原型 UI 骨架：四栏 workbench + 多项目目录（v1.2 重定向）

> 方向变更（2026-09-13）：原 M4（AgentProvider）顺延为 M5。用户以 Open Design 原型（astra-ade-prototype-v2）为准设计了目标 UI，M4 改为把 UI 骨架立起来，为 M5 会话 UI 铺路。落地策略 A：一步到位重构，不留新旧两套布局并存的过渡态。

| 项 | 内容 |
|---|---|
| 交付物 | 前端：四栏 workbench（`Rail` 图标导航 + `ProjectSide` 项目树 + `TabStrip/TabBody` 多类型标签中心 + `CtxPanel` 右面板）；`tabStore` 单一 tab 模型（kind 可扩展，M4 仅实现 terminal，其余空态占位）；终端 tab 从旧扁平布局迁入（常驻只藏不卸红线不变）；tabstrip「+」即时建终端（**无对话框，cwd = 项目树选中节点**，orca 哲学）；项目节点「+」建 worktree 小弹层（基线 ref + 建完开终端，失败孤儿回滚）；打开目录 overlay（手输 + dialog 浏览 + 最近打开）；Git 右面板（分支/ahead·behind/变更列表 M·A·D·U 状态色/暂存全部/提交）；全局 toast；**亮色唯一主题迁移**（原型亮色令牌 + Geist/JetBrains Mono woff2 本地打包 + xterm 亮色配色）。后端：`registry::ProjectRegistry`（projects.json，schemaVersion + 单条容错 + 仅 git 仓库入册）；`GitOps` 扩展 `status/stage/commit`（porcelain v2 + `core.quotePath=false` + `GIT_OPTIONAL_LOCKS=0` + 条目上限 2000）；`project_list/add/remove`、`git_status/stage/commit` IPC 命令。**吸收 M3 遗留必办**：#1 WorktreeName::FromStr 接线加固、#2 孤儿 worktree 回滚、#3 `.nx-worktrees/` 写 `.git/info/exclude`（决策：repo 本地零污染）、#4 前端打磨残余项 |
| Rust 学习主题 | serde 建模进阶（`#[serde(default)]` 分层容错、schemaVersion 迁移意识）；porcelain v2 解析（机器可读稳定契约 vs v1 的人读格式）；枚举建模 git 状态（index/worktree 双侧 StatusKind）；newtype `ProjectId`；模块化复用 config store 的原子写模式 |
| 关键技术 | tokio::process（既有）、porcelain v2、zustand、@font-face 本地字体 |
| 完成标准（验证） | ① 打开目录 → 项目入树（重启还在）→ 树「+」建 worktree → 终端自动开且 `pwd` 落 worktree；② tabstrip「+」即时开终端（选中节点决定 cwd），多项目多终端并行互不串流；③ Git 面板：改文件 → 列表出现 → 暂存 → 填信息提交 → 列表清空（外部 `git log` 核对）；④ 刷新恢复：终端内容 + 项目树 + tab 全部重建；⑤ 亮色主题全界面一致（无暗色残留）、断网启动字体正常；⑥ `cargo test --workspace` 全绿（porcelain v2 快照单测含中文路径）+ 三平台 CI 绿；⑦ M2/M3 基线不回退（强杀/背压/恢复/关 tab 即删） |

### M5 — AgentProvider + 并行编排 + 会话 UI

| 项 | 内容 |
|---|---|
| 交付物 | `AgentProvider` trait + `ProviderRegistry`；`CliAgentProvider` 由配置中的 AgentProfile 驱动（预置 claude/codex/qwen/opencode 四个模板，命令可编辑）；`LaunchFleetDialog`：选 repo + 选 provider ×N + 基线分支 → 原子创建 N worktree + N 会话；`launch_fleet` 失败回滚；`app://error` → 前端 toast；**会话 UI（原型 pane-agent 形态）**：tab kind `agent-session`——消息流/工具调用卡片/任务列表/composer（模型选择 + @上下文），挂在 M4 的 tab 框架上 |
| Rust 学习主题 | **trait 进阶**：`dyn Trait`、对象安全、`Box<dyn AgentProvider>` vs 泛型取舍；enum 状态机驱动 UI（穷尽 match 保证新增状态时编译器点名所有漏改处）；错误类型层次设计（`NexusError` 分层：Config/Git/Pty/Spawn）；registry 模式；broadcast 通道扇出与 lagged 处理 |
| 关键技术 | trait object、EventBus、zustand selector 派生 |
| 完成标准（验证） | ① 一条龙：选 repo → 3 个不同 agent 并行跑 3 个 worktree（真实跑 `claude`/`codex`/`qwen` 命令）→ 项目树/标签状态实时变化 → 一键全部停止 → 清理；② 故意把某 agent 命令写错，该会话进 Failed 且 toast 提示，其余不受影响；③ fleet 创建中途失败（如分支名冲突）自动回滚已建 worktree；④ 会话 UI 消息流/工具卡片随 agent 运行实时渲染；⑤ `cargo test`：用假 agent profile 跑通编排集成测试 |

### M6 — Diff/合并 + 打磨 = MVP

| 项 | 内容 |
|---|---|
| 交付物 | `worktree_diff`（`git diff <base>...HEAD` + 提交列表 + 变更文件清单，Rust 解析成结构化 `DiffSummary`）与前端 `DiffView`（并排/统一视图，多 worktree 对比入口）；`worktree_merge`（默认 fast-forward/merge，冲突时返回冲突文件列表并引导用户在终端手工解决，**MVP 不做内置冲突编辑器**）；窗口状态记忆、single-instance（可选）；NSIS 安装包构建；docs 与 README 更新 |
| Rust 学习主题 | 生命周期进阶（API 返回拥有数据 vs 借用，避免无谓 clone）；性能调优实践（合帧参数、replay 尺寸实测）；`#[tokio::test]` 异步测试；tracing/log 结构化日志在排障中的运用 |
| 关键技术 | git diff 解析、tauri-plugin-window-state、tauri bundler（NSIS）、webgl renderer |
| 完成标准（MVP 验收） | ① **端到端场景**：同一任务发给 2 个 agent 在 2 个 worktree 并行执行 → 并排查看两侧 diff → 选择胜者 merge 回主分支 → 清理全部 worktree，全程不离开应用；② 安装包在无开发环境的干净 Win11 机器上安装可用；③ 30 分钟多会话浸泡后内存平稳、CPU 空闲时接近 0；④ 所有里程碑完成标准仍可复验 |

---

## 3. 关键技术选型

| 领域 | 推荐 | 一句话理由 | 备选与不选原因 |
|---|---|---|---|
| PTY | **portable-pty 0.9** | wezterm 项目出品，Windows ConPTY 支持最成熟且跨平台，API 是"阻塞 IO + Send trait object"，对新手比 async PTY 库友好 | conpty/winpty 直用（不跨平台、裸 COM API）；tokio-pty/async-pty 类（多一层抽象，且阻塞内核一样是 ConPTY，无收益） |
| git | **调用 git CLI（tokio::process）+ `GitOps` trait 留缝** | worktree add/list/prune/merge/diff 全部一条命令 + `--porcelain` 稳定解析，文档海量、学习曲线最平 | **git2**：worktree API 完整，但 C 绑定式 API、lifetime 密集，对新手是 M3 最大的翻车点——留作未来读操作的第二实现；**gitoxide**：checkout/status 机制强但 `worktree add` porcelain 未完成，不选 |
| 异步运行时 | **tokio** | 事实标准，Tauri 2 内部即 tokio，生态/文档覆盖最全 | async-std（维护萎缩）；smol（学习资料少） |
| 序列化 | **serde + serde_json** | Tauri IPC 唯一正解，derive 宏也是学 Rust 宏威力的第一课 | rkyv/bincode（Tauri 不支持） |
| Rust 侧状态 | **Tauri State + "注册表 RwLock + 每会话独立任务"**（演进式：M1-M2 `Mutex<HashMap>`，M5 起会话内部状态收进各自任务，锁只保护注册表） | 避免一把大锁锁全世界，又不必一开始就学完整 actor 框架 | 纯共享可变状态（锁地狱）；actix/actor 框架（过度设计） |
| 前端框架 | **React 19 + Vite + TS** | 已锁定；Vite 是 Tauri 官方模板默认 | – |
| 终端组件 | **@xterm/xterm + addon-fit + addon-web-links + addon-webgl（渐进增强）** | xterm.js 5.x 官方包名（`@xterm` scope），VS Code 同源 | tmux 嵌入/web terminal 自绘（工作量不可控） |
| 前端状态 | **zustand** | 单 store + selector 与"会话表 + 多派生视图"天然匹配，学习成本一晚上 | jotai（原子化适合表单密集场景）；Redux Toolkit（仪式感过重）；无库（prop drilling 到 M4 必炸） |
| UI 组件库 | **shadcn/ui + Tailwind v4**（M3 起；M4 起按原型令牌迁移为**亮色唯一主题**，字体 Geist + JetBrains Mono woff2 本地打包） | 组件源码进仓库可控可改、无运行时框架锁定、贴原型视觉（单色外壳 + 状态色语义），Tauri 社区常用 | Ant Design（组件全、中文文档佳，但包体大、默认风格偏管理后台）；纯手写 CSS（M0–M2 方案，M5 表单/toast 密集后成本上升） |
| 配置存储 | **手写 serde_json + 原子写（tmp+rename）** | 学习价值（serde 建模、路径 API、原子性思维）且 AgentProfile 结构复杂度高；就一个 JSON 文件，插件黑盒反而碍事 | tauri-plugin-store（封装了学习点，且 watcher/迁移能力暂不需要）——若未来配置膨胀再迁 |
| Tauri 2 插件（MVP 应上） | M2：**tauri-plugin-log**；M3：**tauri-plugin-dialog** + **tauri-plugin-opener**；M6：**tauri-plugin-window-state**（+可选 single-instance） | 全部官方维护、各自解决一个真实需求 | updater/process 插件推迟到 MVP 后发布阶段 |

---

## 4. 架构扩展点

### 4.1 AgentProvider trait 设计草图

```rust
// nexus-core/src/agent/provider.rs —— 接口规格（非实现）
pub trait AgentProvider: Send + Sync {
    fn id(&self) -> ProviderId;                      // "cli:claude-code"
    fn display_name(&self) -> &str;                  // "Claude Code"
    fn capabilities(&self) -> AgentCapabilities;     // requires_pty / supports_resume / prompt_hint 正则（供未来 WaitingInput 推断）

    /// 把"要在哪、干什么"翻译成"怎么启动"：argv 模板渲染、env 注入、cwd 决策
    fn prepare(&self, ctx: &LaunchContext) -> Result<LaunchSpec, NexusError>;

    /// 传输形态：v1 只有 Pty；未来 Acp(JsonRpc over stdio) / Mcp
    fn transport(&self) -> AgentTransport;
}

pub struct LaunchContext { repo, worktree, user_prompt: Option<String>, env_overrides }
pub struct LaunchSpec   { argv, env, cwd, initial_cols_rows, term_env }
```

- v1 唯一实现 `CliAgentProvider`：持有配置里的 `AgentProfile { command, args_template, env }`，`prepare` 渲染模板（`{worktree_path}`、`{branch}` 等占位符）。
- **ACP/MCP 接入点**：未来加 `AcpProvider`，同一 trait、`transport() == AgentTransport::Acp`。`AgentSession` 的抽象是"统一的 `SessionEvent` 流 + 可选的 PTY 字节流"：CLI 会话只发 PTY 流（UI 显示终端），ACP 会话发结构化消息（UI 显示聊天视图）——前端按 transport 选视图，会话管理/编排/worktree 复用不变。这就是把"终端"从架构一等公民降级为"CLI agent 的一种渲染方式"的预留。
- 注册：启动时 `ProviderRegistry::from_config()`，运行时可注册新 provider（为未来插件化留门）。

### 4.2 未来工具（API 客户端/DB 客户端）挂载方式

- 新增并列 crate `crates/nexus-tools/`，每个工具 = 一个 Rust module 实现 `ToolDescriptor { id, title, permissions, commands }` + 前端 `features/<tool>/` 一个 lazy route。API 客户端主体是 reqwest 调用编排，DB 客户端用 sqlx——与 agent 编排正交，通过同一 EventBus/IPC 命名空间（`http://`、`db://`）挂入，**不触碰 agent/pty/gitx 任何模块**。
- 与 agent 侧的交汇点只有一个：未来可把工具结果作为上下文喂给 agent（走 provider 的 `LaunchContext` 扩展），这是后话。

### 4.3 协议接入（ACP/MCP）

- 预留 `nexus-protocol` crate 位置：stdio JSON-RPC 传输层 + 协议握手。MCP 优先作为**客户端**接入（给 agent 挂外部工具），ACP 作为 provider 实现（见 4.1）。两者都依赖 M5 的 trait 缝，MVP 不写一行，但 `AgentTransport` enum 现在就把变体占位。

---

## 5. 风险与缓解

| # | 风险 | 缓解 |
|---|---|---|
| 1 | **新手直上 PTY + async 翻车** | 里程碑本身即缓解：M1 纯同步线程，M2 才 async；每主题先在 `examples/` 写独立小例（`pty_echo.rs`、`tokio_mpsc.rs`）再进主线；M2 重构有 M1 作为行为基线可对照验证 |
| 2 | **ConPTY/Windows 细节坑** | 全部由 portable-pty 吸收；我们控制的部分：注入 `TERM=xterm-256color`；UTF-8 用 Rust 侧增量解码器防 chunk 撕裂；初始 PTY 尺寸由前端 fit 实测传入（避免 80x24 默认导致 TUI 重排抖动）；路径一律 `PathBuf` + canonicalize，不手拼字符串 |
| 3 | **xterm.js 高频渲染卡顿** | 三层防线：Rust 侧有界通道背压 → 合帧（16ms/32KB）→ 前端输出绕过 React 直写 `term.write` + webgl renderer；绝不 emit 每个字节；完成标准里有"cat 2 万行不卡"与慢消费者测试 |
| 4 | **Windows Defender/防病毒误报**（未签名 exe 频繁 spawn 子进程可能被启发式盯上） | 开发期把 `target/debug` 目录加入 Defender 排除；发布期走代码签名（MVP 后）+ NSIS；文档记录该注意事项 |
| 5 | **系统 git 缺失/版本过旧** | 启动 `git_check` 显式检测并给安装引导；git 版本 >= 2.20（worktree porcelain 稳定期）才放行 worktree 功能 |
| 6 | **worktree 里依赖缺失**（agent 进 worktree 后 `node_modules` 不存在，装依赖慢） | MVP：文档提示 + `worktree_create` 后自动执行用户配置的 "post-create hook"（可选命令）；符号链接方案列为 post-MVP |
| 7 | **前端热重载/崩溃导致会话孤儿** | Rust 侧 ring buffer + `session_attach` replay + `session_list` 幂等恢复（M2 完成标准③） |
| 8 | **agent 全屏 TUI 在小终端异常** | 终端 pane 设最小尺寸（如 60x12），低于则显示遮罩提示；fit addon 防抖后再 resize，避免高频 resize 风暴 |
| 9 | **IPC JSON 开销天花板**（Channel 仍走 JSON） | MVP 参数下调优足够；天花板场景（日志洪流）post-MVP 评估 Tauri 自定义协议流式响应，架构上隔离在 batcher 之后，可替换 |
| 10 | **状态机与前端漂移** | 状态 enum 是 Rust 单一权威，TS 类型手写镜像 + 一个 `session_list` 对齐测试；穷尽 match 让新增状态时编译器强制改所有处理点 |
| 11 | **M4 前端结构性重构回归**（一步到位换 shell，终端功能可能回退） | 每任务 `pnpm build` 绿；终端 pane 挂载闩锁（一旦挂载只藏不卸）；完成标准⑦把 M2/M3 基线（强杀/背压/刷新恢复/关 tab 即删）列为 M4 验收项；SDD 每任务独立审查 |
| 12 | **porcelain v2 解析缺陷**（中文路径转义、rename 条目的 NUL 分隔 origPath） | `core.quotePath=false` 保证非 ASCII 路径原样输出；快照单测钉住解析（含中文路径、rename、staged/unstaged 双侧组合）；解析纪律只认行首关键词，未知行跳过 |
| 13 | **tab 框架过度设计**（kind 扩展位诱使提前实现会话/diff 标签） | M4 只实现 `terminal` 一种 kind，其余 kind 空态占位（显示"该类标签将在后续里程碑提供"）；YAGNI 红线写进计划 |

---

## 6. 主要参考来源

- [stablyai/orca（GitHub）](https://github.com/stablyai/orca) / [onorca.dev](https://www.onorca.dev/)
- [Tauri 2：Calling Rust from the Frontend（Channel 流式 API）](https://v2.tauri.app/develop/calling-rust/) / [Tauri 2：Calling the Frontend from Rust（emit）](https://v2.tauri.app/develop/calling-frontend/) / [Tauri IPC 性能设计讨论 #5690](https://github.com/orgs/tauri-apps/discussions/5690)
- [git2::Worktree（docs.rs）](https://docs.rs/git2/latest/git2/struct.Worktree.html) / [gitoxide crate 状态文档](https://github.com/GitoxideLabs/gitoxide/blob/main/crate-status.md) / [gitui 对 gitoxide worktree 的评估](https://github.com/gitui-org/gitui/issues/2812)
- [portable-pty（docs.rs）](https://docs.rs/portable-pty) / [portable-pty（crates.io）](https://crates.io/crates/portable-pty)
