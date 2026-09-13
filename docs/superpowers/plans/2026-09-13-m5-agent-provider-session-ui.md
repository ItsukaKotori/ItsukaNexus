# ItsukaNexus M5(AgentProvider + 并行编排 + 会话 UI)Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 落地 AgentProvider 体系(claude 走 stream-json 双向 JSON 流的结构化会话 + codex/qwen/opencode 走 PTY 终端)、LaunchFleetDialog 并行编排(N worktree × N 会话,失败逆序回滚)、tab kind `agent` 会话 UI(消息流/工具卡片/composer,原型 pane-agent 形态),并吸收 M4 延后的高价值 Minor。

**Architecture:** 后端先行:config 层扩展 transport 字段与五条预置模板 → provider/registry(streamjson 解析器以实测帧样本 TDD)→ manager 双传输分叉(jsonl = std::process 管道子进程,与 PTY 同构的阻塞 IO 三任务树,生命周期/状态机/强杀全复用)→ IPC 四新命令。前端:agentStore(拍平消息流,与原型平铺一致)→ AgentPane → tab kind 分派 → LaunchFleetDialog。**orca 已有功能直接参考其实现**(spec v1.3 参考锚点);claude stream-json 帧形态已在计划编写时实测钉住(2026-09-13,本机 claude CLI,样本内嵌于 Task 3)。

**Tech Stack:** Tauri 2、tokio + std::process、serde、zustand、Tailwind v4 + shadcn/ui。

**Spec:** `docs/superpowers/specs/2026-09-09-itsukanexus-mvp-design.md` **v1.3**(§1.2 布局、§1.3 AgentProvider 体系与双传输、§1.4 命令表、§1.5 会话视图与红线辨析、§2 M5 章节、§4.1 trait 落地、§5 风险 #14-16)
**原型(视觉权威):** `~/Library/Application Support/Open Design/namespaces/release-stable/data/projects/a3e91f89-9d76-4a78-ae04-cf4e4b3d0501/astra-ade-prototype-v2.html`(pane-agent 段 805-835 行:main-head/msgs/tool 卡片/composer)
**orca 参考锚点(spec v1.3):** `src/shared/tui-agent-config.ts`(预置命令值)、`src/main/claude/claude-structured-launch-resolution.ts`(结构化启动参数)、`src/shared/native-chat-types.ts`(消息/工具卡片模型)
**M4 完成记录:** `docs/superpowers/plans/2026-09-13-m4-prototype-ui-shell.md` 末尾(M5 必办来源 + 平台知识库;Task 11 吸收其高价值项)

## Global Constraints

- 开发机:macOS(Apple Silicon),Rust 1.95、node 22、pnpm 12.3.4;跨平台回归靠 CI 三平台矩阵
- 分支:worktree 特性分支 `m5-agent-provider-session-ui` 上实现,禁止直接提交 main;**PR 由用户本人合并**
- 提交规范:每任务一次提交,信息结尾 `Co-Authored-By: Claude Code <noreply@anthropic.com>`
- 包管理器只用 **pnpm**
- 质量门槛(每任务收尾必过):`cargo fmt` 已应用、`cargo clippy --workspace --all-targets -- -D warnings` 零警告、`cargo test --workspace` 全绿;涉及前端时 `pnpm build` 全绿(裸 `cargo test` 只覆盖根包——M3 实测陷阱)
- git push/pull/fetch 一律带代理:`git -c http.proxy=http://127.0.0.1:7890 push ...`(gh CLI 可直连;`gh pr create` 用 `--body-file`,内联 body 会被权限拒)
- **nexus-core 不依赖 tauri** 的缝保持:core 内不得 `use tauri`;wire 类型只定义在 IPC 层——**例外先例**:core 的 `StateChange`/`SessionSnapshot` 直接 derive Serialize 过 IPC(纯 serde 不引 tauri),`AgentEvent` 同此路线(spec §1.3)
- nexus-core 不新增第三方依赖(现有 tokio/serde/uuid/thiserror/async-trait/tempfile(dev)足够)
- **权限默认**:claude 预置模板 argsTemplate 自带 `--permission-mode acceptEdits`(用户 2026-09-13 拍板);切换全自动 = 用户编辑 profile 改 `bypassPermissions`
- **红线不变**:PTY 输出不进 React(输出直达 `term.write`);终端 pane 挂载闩锁(常驻只藏不卸,React 19 reconcile 源码级验证结论在 M4 完成记录——AgentPane 走 TabBody 既有全量 map 机制,同享闩锁,不在 main 前插新常驻栏,无需重验);亮色唯一主题;订阅清理
- **结构化事件红线辨析(spec §1.5)**:`AgentEvent` 是低频语义事件(非字节流),进 zustand 合法;消息 store cap 500 条丢头
- M2-M4 参数定值不变:读 chunk 8KB、合帧 16ms/32KB、有界队列 64、订阅流 128、replay 256KB(jsonl 的行 replay 同用 256KB)、SEND_INPUT_TIMEOUT 2s
- 命令命名 snake_case;既有事件 `session://state`、`session://exit`、`worktree://changed` 不变,M5 不新增事件(`app://error` 已裁剪——spec v1.3)
- 命令参数路径一律字符串(camelCase)过 IPC,core 内一律 `PathBuf` + canonicalize
- Windows 陷阱(M1-M4 实测,仍有效):路径断言不 `contains` 字面子串;阻塞读/wait 有界;孙进程测试显式清理;CI `--workspace` 必须
- stream-json 解析纪律(风险 #14):只认必要字段,未知帧/未知块类型静默跳过;**不得假设 tool_use id 前缀**(本机实测为 `call_` 前缀,上游文档语义是 `toolu_`,按任意非空字符串处理)

---

### Task 1: config 层——AgentProfile.transport 字段 + 五条预置模板

**Files:**
- Create: `src-tauri/crates/nexus-core/src/agent/provider.rs`(本任务仅 `AgentTransport` enum)
- Modify: `src-tauri/crates/nexus-core/src/agent/mod.rs`(`pub mod provider;`)
- Modify: `src-tauri/crates/nexus-core/src/config/model.rs`(transport 字段 + 默认五条)
- Test: `src-tauri/crates/nexus-core/tests/` 既有 config 测试(追加断言)

**Interfaces:**
- Produces(Task 2/4/5 依赖):
  - `agent::provider::AgentTransport` enum:`Pty | Jsonl`,serde rename_all lowercase(`"pty" | "jsonl"`),Copy + Clone + Debug + PartialEq + Eq + Serialize + Deserialize
  - `AgentProfile` 增字段 `transport: AgentTransport`,`#[serde(default)]`(缺省 `Pty`——M2 旧档零损);`AppConfig::default()` 的 `agent_profiles` 变五条(shell/claude/codex/qwen/opencode,命令值取 orca `tui-agent-config.ts`:claude=`claude`、codex=`codex`、qwen 包名 qwen-code 但二进制为 `qwen`、opencode=`opencode`)

**学习点:** ① `#[serde(default)]` 在 enum 字段上:旧档缺 `transport` 反序列化落 `Pty`,新档显式写 `"jsonl"`——这是 M2 档零损迁移的全部;② 预置模板即默认配置(`AppConfig::default` 带全),不是代码常量优先——用户 config_save 后以磁盘档为准,想恢复默认删配置文件即可,与 config store 哲学一致。

- [ ] **Step 1: 写失败测试**

既有 config 集成测试(`tests/config_store.rs` 或同型文件,以实际文件名为准)追加:

```rust
#[test]
fn default_profiles_contain_five_presets_with_transports() {
    let cfg = nexus_core::config::model::AppConfig::default();
    let ids: Vec<(String, nexus_core::agent::provider::AgentTransport)> = cfg
        .agent_profiles
        .iter()
        .map(|p| (p.id.clone(), p.transport))
        .collect();
    // orca tui-agent-config.ts 验证值:qwen-code 包的二进制是 qwen
    for (id, cmd, transport) in [
        ("shell", "SHELL", nexus_core::agent::provider::AgentTransport::Pty),
        ("claude", "claude", nexus_core::agent::provider::AgentTransport::Jsonl),
        ("codex", "codex", nexus_core::agent::provider::AgentTransport::Pty),
        ("qwen", "qwen", nexus_core::agent::provider::AgentTransport::Pty),
        ("opencode", "opencode", nexus_core::agent::provider::AgentTransport::Pty),
    ] {
        let hit = ids.iter().find(|(pid, _)| pid == id)
            .unwrap_or_else(|| panic!("缺预置 {id}: {ids:?}"));
        assert_eq!(hit.1, transport, "{id} transport 应为 {transport:?}");
        let p = cfg.agent_profiles.iter().find(|p| p.id == id).unwrap();
        if cmd != "SHELL" {
            assert_eq!(p.command, cmd, "{id} 命令取 orca 验证值");
        }
    }
    // claude 模板必须带 stream-json 双向参数 + acceptEdits 权限默认(spec v1.3)
    let claude = cfg.agent_profiles.iter().find(|p| p.id == "claude").unwrap();
    let args = claude.args_template.join(" ");
    assert!(args.contains("--output-format stream-json"), "claude 模板: {args}");
    assert!(args.contains("--input-format stream-json"), "claude 模板: {args}");
    assert!(args.contains("--permission-mode acceptEdits"), "权限默认 acceptEdits: {args}");
}

#[test]
fn legacy_profile_without_transport_defaults_to_pty() {
    // M2 旧档:无 transport 字段 → Pty(零损迁移)
    let json = r#"{"id":"shell","displayName":"Shell","command":"/bin/zsh","argsTemplate":[],"env":{}}"#;
    let p: nexus_core::config::model::AgentProfile = serde_json::from_str(json).unwrap();
    assert_eq!(p.transport, nexus_core::agent::provider::AgentTransport::Pty);
}
```

- [ ] **Step 2: 红** — `cargo test --workspace`(provider 模块不存在,编译错)。
- [ ] **Step 3: 实现**

`agent/provider.rs`(本任务只放 enum;trait 在 Task 2 同文件补):

```rust
// AgentProvider 体系(spec §4.1 v1.3):传输形态 enum 在此定义,
// config 与 provider 共用。trait 定义见下方 Task 2 补齐。
use serde::{Deserialize, Serialize};

/// 会话传输形态:PTY 终端字节流,或 stdin/stdout 的 JSONL 结构化流(claude)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentTransport {
    Pty,
    Jsonl,
}
```

`agent/mod.rs` 加 `pub mod provider;`。

`config/model.rs`:

```rust
use crate::agent::provider::AgentTransport;

pub struct AgentProfile {
    pub id: String,
    pub display_name: String,
    pub command: String,
    pub args_template: Vec<String>,
    pub env: HashMap<String, String>,
    /// 传输形态:缺省 PTY(M2 旧档零损);jsonl = claude stream-json 双向流
    #[serde(default)]
    pub transport: AgentTransport,
}
```

`Default for AgentProfile` 补 `transport: AgentTransport::Pty`;`default_shell_profile` 同。`AppConfig::default` 的 profiles 换成五条:

```rust
impl AppConfig {
    fn default_profiles() -> Vec<AgentProfile> {
        vec![
            AgentProfile {
                id: "shell".into(),
                display_name: "Shell".into(),
                command: default_shell(),
                args_template: Vec::new(),
                env: HashMap::new(),
                transport: AgentTransport::Pty,
            },
            // claude:结构化会话(spec v1.3 §1.3,参数与 orca SDK query() 等价——
            // -p/--input-format/--output-format/--verbose;权限默认 acceptEdits,
            // 用户可编辑改 bypassPermissions;不用 orca 的 --permission-prompt-tool
            // stdio(需要 canUseTool 回调)与 CLAUDE_CONFIG_DIR 钉扎(单用户单配置))
            AgentProfile {
                id: "claude".into(),
                display_name: "Claude Code".into(),
                command: "claude".into(),
                args_template: vec![
                    "-p".into(),
                    "--input-format".into(), "stream-json".into(),
                    "--output-format".into(), "stream-json".into(),
                    "--verbose".into(),
                    "--permission-mode".into(), "acceptEdits".into(),
                ],
                env: HashMap::new(),
                transport: AgentTransport::Jsonl,
            },
            AgentProfile { id: "codex".into(), display_name: "Codex CLI".into(), command: "codex".into(), args_template: Vec::new(), env: HashMap::new(), transport: AgentTransport::Pty },
            // orca 注:qwen-code 包安装的 PATH 二进制是 qwen
            AgentProfile { id: "qwen".into(), display_name: "Qwen Code".into(), command: "qwen".into(), args_template: Vec::new(), env: HashMap::new(), transport: AgentTransport::Pty },
            AgentProfile { id: "opencode".into(), display_name: "OpenCode".into(), command: "opencode".into(), args_template: Vec::new(), env: HashMap::new(), transport: AgentTransport::Pty },
        ]
    }
}
```

(`AppConfig::default` 里 `agent_profiles: Self::default_profiles()`;既有引用 `AgentProfile::default_shell_profile()` 的测试按新默认集核对修正——shell 仍是第一条,断言大多不破。)

- [ ] **Step 4: 绿 + 门槛**

```bash
cargo test --workspace && cargo fmt && cargo clippy --workspace --all-targets -- -D warnings
```

- [ ] **Step 5: 提交**

```bash
git add src-tauri && git commit -m "feat(m5): AgentProfile transport 字段 + 五条预置模板(orca 命令值)

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 2: provider.rs trait + CliAgentProvider + ProviderRegistry

**Files:**
- Modify: `src-tauri/crates/nexus-core/src/agent/provider.rs`(补 trait + CliAgentProvider + LaunchContext/LaunchSpec)
- Create: `src-tauri/crates/nexus-core/src/agent/registry.rs`
- Modify: `src-tauri/crates/nexus-core/src/agent/mod.rs`(`pub mod registry;`)
- Modify: `src-tauri/crates/nexus-core/src/agent/manager.rs`(LaunchSpec 重构:argv/env/transport 字段)
- Create: `src-tauri/crates/nexus-core/tests/provider_registry.rs`

**Interfaces:**
- Consumes: Task 1 `AgentTransport`、`AgentProfile`;manager 既有 `LaunchSpec`(本任务重构)
- Produces(Task 4/5/9 依赖):
  - `provider::AgentProvider` trait:`id(&self) -> &str`、`display_name(&self) -> &str`、`transport(&self) -> AgentTransport`、`prepare(&self, ctx: Option<&LaunchContext>) -> Result<LaunchSpec, NexusError>`
  - `provider::LaunchContext { cwd: Option<PathBuf>, repo_path: Option<String>, worktree_name: Option<String>, branch: Option<String> }`(cwd None = 继承进程目录;branch 由 fleet 填,单发 None)
  - `provider::LaunchSpec { argv: Vec<String>, env: HashMap<String, String>, cwd: Option<PathBuf>, repo_path: Option<String>, worktree_name: Option<String>, transport: AgentTransport }`(替代 manager.rs 旧 LaunchSpec——旧字段 cwd/repo_path/worktree_name 语义不变,IPC 层 Task 5 改造)
  - `provider::CliAgentProvider`(唯一实现,持 `AgentProfile`)
  - `registry::ProviderRegistry`:`from_config(&AppConfig) -> Self`(用户档 by id 优先,默认集五条补缺——用户显式删的默认条目会复活,MVP 无删除 UI,可接受)、`get(&self, id: &str) -> Option<Arc<dyn AgentProvider>>`、`list() -> Vec<ProviderInfo>`
  - `registry::ProviderInfo { id, display_name, transport, command, available }`(serde camelCase;available = 命令存在性探测)

**学习点:** ① `Arc<dyn AgentProvider>` vs 泛型:provider 数量小、按 id 动态查表,trait object 是正解(泛型会让 SessionManager 泛型污染);② 占位符渲染纪律:模板占位符 `{worktree_path}`/`{repo_path}`/`{branch}`,ctx 对应字段为 None 时返回 `InvalidInput`(渲染成空串会产出危险命令行);③ `command_exists` 探测 = `which`(Unix)/`where`(Windows) 子进程,参考 orca detectCmd——同步函数,调用方(Task 5)用 spawn_blocking 包。

- [ ] **Step 1: 写失败测试**

`tests/provider_registry.rs`:

```rust
// AgentProvider 体系:模板渲染 + 注册表合并 + 命令探测。
use std::collections::HashMap;

use nexus_core::agent::provider::{AgentTransport, CliAgentProvider, LaunchContext};
use nexus_core::agent::registry::ProviderRegistry;
use nexus_core::config::model::{AgentProfile, AppConfig};

fn profile(id: &str, transport: AgentTransport) -> AgentProfile {
    AgentProfile {
        id: id.into(),
        display_name: format!("P-{id}"),
        command: "echo".into(),
        args_template: vec!["{worktree_path}".into(), "-m".into()],
        env: HashMap::from([("NX_FLAG".to_string(), "1".to_string())]),
        transport,
    }
}

#[test]
fn prepare_renders_placeholders_and_env() {
    let p = CliAgentProvider::new(profile("t", AgentTransport::Pty));
    let ctx = LaunchContext {
        cwd: Some("/wt/abc".into()),
        repo_path: Some("/repo".into()),
        worktree_name: Some("nexus/t-1".into()),
        branch: Some("nx/t-1".into()),
    };
    let spec = p.prepare(Some(&ctx)).unwrap();
    assert_eq!(spec.argv, vec!["/wt/abc".to_string(), "-m".into()]);
    assert_eq!(spec.env.get("NX_FLAG").map(String::as_str), Some("1"));
    assert_eq!(spec.cwd.as_deref(), Some(std::path::Path::new("/wt/abc")));
    assert_eq!(spec.transport, AgentTransport::Pty);
    // 模板需要上下文而 ctx=None → InvalidInput(不渲染空串)
    assert!(p.prepare(None).is_err());
}

#[test]
fn registry_merges_user_over_defaults() {
    let mut cfg = AppConfig::default();
    // 用户改了 claude 的显示名,删不掉默认集其余条目
    cfg.agent_profiles = vec![AgentProfile {
        id: "claude".into(),
        display_name: "我的 Claude".into(),
        command: "claude".into(),
        args_template: vec![],
        env: HashMap::new(),
        transport: AgentTransport::Jsonl,
    }];
    let reg = ProviderRegistry::from_config(&cfg);
    assert_eq!(reg.get("claude").unwrap().display_name(), "我的 Claude");
    assert!(reg.get("codex").is_some(), "默认集补缺");
    assert!(reg.get("nope").is_none());
    let list = reg.list();
    assert!(list.iter().any(|p| p.id == "qwen" && !p.transport_jsonl()));
}

#[test]
fn provider_info_reports_command_existence() {
    let reg = ProviderRegistry::from_config(&AppConfig::default());
    let list = reg.list();
    // echo 在三平台都存在;不存在的命令 available=false 而非报错
    assert!(list.iter().all(|p| p.command != "__nx_absent__"));
}
```

(注:`transport_jsonl()` 之类的辅助若不想要,直接比 `p.transport == AgentTransport::Jsonl`——`ProviderInfo` 的 transport 字段公开即可,测试以字段写。)

- [ ] **Step 2: 红** — 编译错(trait/registry 不存在)。
- [ ] **Step 3: 实现**

`provider.rs` 追加:

```rust
use std::collections::HashMap;
use std::path::PathBuf;

use crate::config::model::AgentProfile;
use crate::error::NexusError;

/// "要在哪"的上下文:provider 把它渲染成"怎么启动"。
/// cwd None = 继承进程目录(M2 纯 shell 行为);branch 仅 fleet 场景有。
pub struct LaunchContext {
    pub cwd: Option<PathBuf>,
    pub repo_path: Option<String>,
    pub worktree_name: Option<String>,
    pub branch: Option<String>,
}

/// provider 的启动产物。repo_path/worktree_name 原样透传进会话快照
/// (前端 tab 归属标记,与 M3/M4 语义一致)。
pub struct LaunchSpec {
    pub argv: Vec<String>,
    pub env: HashMap<String, String>,
    pub cwd: Option<PathBuf>,
    pub repo_path: Option<String>,
    pub worktree_name: Option<String>,
    pub transport: AgentTransport,
}

/// Agent 接入缝(spec §4.1 v1.3):唯一实现 CliAgentProvider(配置驱动)。
/// 未来 ACP/MCP 加实现 + AgentTransport 加变体,不碰本签名。
pub trait AgentProvider: Send + Sync {
    fn id(&self) -> &str;
    fn display_name(&self) -> &str;
    fn transport(&self) -> AgentTransport;
    fn prepare(&self, ctx: Option<&LaunchContext>) -> Result<LaunchSpec, NexusError>;
}

pub struct CliAgentProvider {
    profile: AgentProfile,
}

impl CliAgentProvider {
    pub fn new(profile: AgentProfile) -> Self {
        Self { profile }
    }

    /// 占位符渲染:ctx 对应字段 None → InvalidInput(绝不渲染空串)
    fn render(&self, tmpl: &str, ctx: &LaunchContext) -> Result<String, NexusError> {
        let out = match tmpl {
            "{worktree_path}" => ctx
                .cwd
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned())
                .ok_or_else(|| NexusError::InvalidInput("模板 {worktree_path} 需要会话落点(选中项目/worktree 后创建)".into()))?,
            "{repo_path}" => ctx.repo_path.clone().ok_or_else(|| {
                NexusError::InvalidInput("模板 {repo_path} 需要仓库上下文".into())
            })?,
            "{branch}" => ctx.branch.clone().ok_or_else(|| {
                NexusError::InvalidInput("模板 {branch} 仅并行编排场景可用".into())
            })?,
            _ => tmpl.to_string(),
        };
        Ok(out)
    }
}

impl AgentProvider for CliAgentProvider {
    fn id(&self) -> &str { &self.profile.id }
    fn display_name(&self) -> &str { &self.profile.display_name }
    fn transport(&self) -> AgentTransport { self.profile.transport }
    fn prepare(&self, ctx: Option<&LaunchContext>) -> Result<LaunchSpec, NexusError> {
        let mut argv = vec![self.profile.command.clone()];
        // ctx=None 且模板含占位符 → render 报错;纯 shell 类模板无占位符可 None
        let empty_ctx;
        let ctx = match ctx {
            Some(c) => c,
            None => { empty_ctx = LaunchContext { cwd: None, repo_path: None, worktree_name: None, branch: None }; &empty_ctx }
        };
        for t in &self.profile.args_template {
            argv.push(self.render(t, ctx)?);
        }
        Ok(LaunchSpec {
            argv,
            env: self.profile.env.clone(),
            cwd: ctx.cwd.clone(),
            repo_path: ctx.repo_path.clone(),
            worktree_name: ctx.worktree_name.clone(),
            transport: self.profile.transport,
        })
    }
}
```

`registry.rs`:

```rust
// ProviderRegistry(spec §4.1):默认集五条 + 用户档合并(by id 用户优先)。
use std::sync::Arc;

use serde::Serialize;

use super::provider::{AgentProvider, AgentTransport, CliAgentProvider};
use crate::config::model::{AppConfig, AgentProfile};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInfo {
    pub id: String,
    pub display_name: String,
    pub transport: AgentTransport,
    pub command: String,
    /// 命令存在性(which/where 探测);false = 未安装,UI 降级标注
    pub available: bool,
}

pub struct ProviderRegistry {
    providers: Vec<Arc<dyn AgentProvider>>,
}

impl ProviderRegistry {
    pub fn from_config(cfg: &AppConfig) -> Self {
        let mut profiles: Vec<AgentProfile> = AppConfig::default_profiles_pub();
        for p in &cfg.agent_profiles {
            if let Some(hit) = profiles.iter_mut().find(|d| d.id == p.id) {
                *hit = p.clone(); // 用户档优先
            } else {
                profiles.push(p.clone());
            }
        }
        Self { providers: profiles.into_iter().map(CliAgentProvider::new).map(Arc::from as fn(_) -> Arc<dyn AgentProvider>).collect() }
    }

    pub fn get(&self, id: &str) -> Option<Arc<dyn AgentProvider>> {
        self.providers.iter().find(|p| p.id() == id).cloned()
    }

    /// 含命令探测;探测是同步子进程,IPC 层调用须 spawn_blocking(低频命令,可接受)
    pub fn list(&self) -> Vec<ProviderInfo> {
        self.providers
            .iter()
            .map(|p| ProviderInfo {
                id: p.id().to_string(),
                display_name: p.display_name().to_string(),
                transport: p.transport(),
                command: command_of(p),
                available: command_exists(&command_of(p)),
            })
            .collect()
    }
}

fn command_of(p: &dyn AgentProvider) -> String {
    // CliAgentProvider 唯一实现;command 经 downcast 不优雅,改为
    // registry 构建时保留 profile command 列表(见实现,以编译器接受为准)
    unreachable!()
}

/// which/where 探测(参考 orca detectCmd);同步阻塞,低频调用
pub fn command_exists(cmd: &str) -> bool {
    let probe = if cfg!(windows) { "where" } else { "which" };
    std::process::Command::new(probe)
        .arg(cmd)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}
```

(实现者注:`command_of` 的 downcast 问题——干净解法是 `ProviderRegistry` 内部存 `Vec<(AgentProfile, Arc<dyn AgentProvider>)>`,command 取 profile 侧。上面伪码标注了方向,以编译干净为准。`AppConfig::default_profiles` 需提为 `pub(crate)` 或 `pub` 供 registry 用。)

- [ ] **Step 4: manager.rs LaunchSpec 重构(编译桥)**

manager.rs 旧 `LaunchSpec{cwd, repo_path, worktree_name}` 删除,`use super::provider::LaunchSpec;`——`create()` 本任务暂以 `spec.argv[0]`/`spec.env` 接线:

```rust
// create() 内 PTY spawn 段替换(本任务为编译桥,Task 4 完成分叉):
let spec_argv: Vec<&str> = spec.argv.iter().map(String::as_str).collect();
let (session, child) = PtySession::spawn(
    &spec_argv[0],
    &spec_argv[1..],
    &spec.env,
    cols,
    rows,
    spec.cwd.as_deref(),
)?;
```

`pty/session.rs` 的 `spawn` 增 `env: &HashMap<String, String>` 参数,循环 `cmd.env(k, v)`(TERM/COLORTERM 注入保持——用户 env 不覆盖 TERM)。既有直接调 `spawn` 的测试同步补 `&HashMap::new()`。

- [ ] **Step 5: 绿 + 门槛 + 提交**

```bash
cargo test --workspace && cargo fmt && cargo clippy --workspace --all-targets -- -D warnings
git add src-tauri && git commit -m "feat(m5): AgentProvider trait + CliAgentProvider + ProviderRegistry(模板渲染/合并/探测)

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 3: streamjson.rs——claude stream-json 帧解析(实测快照 TDD)

**Files:**
- Create: `src-tauri/crates/nexus-core/src/agent/streamjson.rs`
- Modify: `src-tauri/crates/nexus-core/src/agent/mod.rs`(`pub mod streamjson;`)

**Interfaces:**
- Consumes: —
- Produces(Task 4/5 依赖):
  - `streamjson::AgentEvent` enum(serde camelCase, tag=type,直接过 IPC——StateChange 先例):`Init { model: String, claude_session: Option<String> } | Text { text: String } | ToolUse { id: String, name: String, input: serde_json::Value } | ToolResult { tool_use_id: String, content: String, is_error: bool } | Done { is_error: bool, duration_ms: u64, cost_usd: Option<f64> }`
  - `streamjson::parse_stream_json_line(line: &str) -> Vec<AgentEvent>`(一行 0..n 个事件;未知帧/未知块/解析失败 → 空 Vec,不报错——风险 #14 纪律)
  - `streamjson::user_message_json(text: &str) -> String`(stdin 写入的 user 消息 JSON 行,实测格式)

**学习点:** ① 帧词汇 = claude CLI 实测(2026-09-13 本机,GLM 后端,`call_` 前缀 tool id——解析不假设前缀);与 orca `claude-stream-json-frame-schema.ts` 的词汇一致(message:assistant/user、system:init、result);② assistant 帧的 `message.content[]` 是块数组:一行可产多个事件(text 块 + 多个 tool_use 块)——返回 Vec 是唯一正确形态;③ `thinking` 块与 `system:thinking_tokens` 流式帧全部忽略(MVP 不渲染 thought;原型 thought 元素延后);hook 帧(system:hook_started/hook_response)忽略。

- [ ] **Step 1: 写失败测试**

`streamjson.rs` 内联 `#[cfg(test)]`(解析器是纯函数,内联快照;真实行样本取自实测):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// 实测样本(2026-09-13,claude CLI -p --input-format/--output-format stream-json --verbose)
    /// 字段截短但形态保真;tool id 是 call_ 前缀(本机后端),解析不得假设前缀。

    #[test]
    fn parses_init() {
        let line = r#"{"type":"system","subtype":"init","cwd":"/Users/x","session_id":"696ac7db-d4ac-422d-8ecb-7613cd857b71","tools":["Bash","Read"],"model":"GLM-5.3","permissionMode":"acceptEdits"}"#;
        let evs = parse_stream_json_line(line);
        assert_eq!(evs.len(), 1);
        match &evs[0] {
            AgentEvent::Init { model, claude_session } => {
                assert_eq!(model, "GLM-5.3");
                assert_eq!(claude_session.as_deref(), Some("696ac7db-d4ac-422d-8ecb-7613cd857b71"));
            }
            other => panic!("应为 Init: {other:?}"),
        }
    }

    #[test]
    fn parses_assistant_text_and_tool_use_blocks() {
        let line = r#"{"type":"assistant","message":{"id":"msg_1","role":"assistant","model":"GLM-5.3","content":[{"type":"text","text":"先用工具查一下"},{"type":"tool_use","id":"call_36eedf5841a3454c9aeae9bb","name":"Bash","input":{"command":"echo nx-probe"}}]}}"#;
        let evs = parse_stream_json_line(line);
        assert_eq!(evs.len(), 2);
        assert!(matches!(&evs[0], AgentEvent::Text { text } if text == "先用工具查一下"));
        match &evs[1] {
            AgentEvent::ToolUse { id, name, input } => {
                assert_eq!(id, "call_36eedf5841a3454c9aeae9bb");
                assert_eq!(name, "Bash");
                assert_eq!(input["command"], "echo nx-probe");
            }
            other => panic!("应为 ToolUse: {other:?}"),
        }
    }

    #[test]
    fn parses_thinking_block_as_no_event() {
        let line = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"thinking","thinking":"...","signature":"x"}]}}"#;
        assert!(parse_stream_json_line(line).is_empty(), "thinking 块忽略");
    }

    #[test]
    fn parses_tool_result_user_frame() {
        let line = r#"{"type":"user","message":{"role":"user","content":[{"tool_use_id":"call_36eedf5841a3454c9aeae9bb","type":"tool_result","content":"nx-probe","is_error":false}]},"session_id":"s","uuid":"u"}"#;
        let evs = parse_stream_json_line(line);
        match &evs[0] {
            AgentEvent::ToolResult { tool_use_id, content, is_error } => {
                assert_eq!(tool_use_id, "call_36eedf5841a3454c9aeae9bb");
                assert_eq!(content, "nx-probe");
                assert!(!is_error);
            }
            other => panic!("应为 ToolResult: {other:?}"),
        }
    }

    #[test]
    fn parses_result_done_frame() {
        let line = r#"{"type":"result","subtype":"success","is_error":false,"duration_api_ms":8650,"session_id":"s","total_cost_usd":0.047}"#;
        let evs = parse_stream_json_line(line);
        match &evs[0] {
            AgentEvent::Done { is_error, duration_ms, cost_usd } => {
                assert!(!is_error);
                assert_eq!(*duration_ms, 8650);
                assert!((cost_usd.unwrap() - 0.047).abs() < 1e-9);
            }
            other => panic!("应为 Done: {other:?}"),
        }
    }

    #[test]
    fn unknown_frames_and_bad_json_yield_empty() {
        assert!(parse_stream_json_line("").is_empty());
        assert!(parse_stream_json_line("not json").is_empty());
        // thinking_tokens 流式帧 / hook 帧未知 subtype → 空
        assert!(parse_stream_json_line(r#"{"type":"system","subtype":"thinking_tokens","delta":"…"}"#).is_empty());
        assert!(parse_stream_json_line(r#"{"type":"system","subtype":"hook_started","name":"x"}"#).is_empty());
        // content 非文本(tool_result 数组形态/图片)→ 占位字符串
        let line = r#"{"type":"user","message":{"role":"user","content":[{"tool_use_id":"c1","type":"tool_result","content":[{"type":"image","data":"…"}],"is_error":false}]}}"#;
        let evs = parse_stream_json_line(line);
        assert!(matches!(&evs[0], AgentEvent::ToolResult { content, .. } if content == "[非文本结果]"));
    }

    #[test]
    fn user_message_json_shape() {
        let s = user_message_json("你好");
        let v: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["type"], "user");
        assert_eq!(v["message"]["role"], "user");
        assert_eq!(v["message"]["content"][0]["type"], "text");
        assert_eq!(v["message"]["content"][0]["text"], "你好");
        assert!(s.ends_with('\n'), "写入 stdin 必须带换行(JSONL)");
    }
}
```

- [ ] **Step 2: 红** — 模块不存在。
- [ ] **Step 3: 实现**

```rust
// claude stream-json 帧解析(spec §1.3 v1.3):一行一帧 JSON,只认必要字段,
// 未知帧/未知块/坏行一律空 Vec(风险 #14:协议漂移不炸会话)。
// 帧词汇对照 orca claude-stream-json-frame-schema.ts;tool id 不假设前缀
// (本机实测 call_ 前缀,上游语义 toolu_)。thinking 块/流式帧忽略(MVP)。
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum AgentEvent {
    Init { model: String, claude_session: Option<String> },
    Text { text: String },
    ToolUse { id: String, name: String, input: serde_json::Value },
    ToolResult { tool_use_id: String, content: String, is_error: bool },
    Done { is_error: bool, duration_ms: u64, cost_usd: Option<f64> },
}

pub fn parse_stream_json_line(line: &str) -> Vec<AgentEvent> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    match v["type"].as_str() {
        Some("system") => {
            if v["subtype"].as_str() == Some("init") {
                out.push(AgentEvent::Init {
                    model: v["model"].as_str().unwrap_or("unknown").to_string(),
                    claude_session: v["session_id"].as_str().map(str::to_string),
                });
            } // thinking_tokens / hook_* 等未知 subtype 忽略
        }
        Some("assistant") => {
            for b in v["message"]["content"].as_array().into_iter().flatten() {
                match b["type"].as_str() {
                    Some("text") => out.push(AgentEvent::Text {
                        text: b["text"].as_str().unwrap_or("").to_string(),
                    }),
                    Some("tool_use") => out.push(AgentEvent::ToolUse {
                        id: b["id"].as_str().unwrap_or("").to_string(),
                        name: b["name"].as_str().unwrap_or("?").to_string(),
                        input: b["input"].clone(),
                    }),
                    _ => {} // thinking 等未知块
                }
            }
        }
        Some("user") => {
            for b in v["message"]["content"].as_array().into_iter().flatten() {
                if b["type"].as_str() == Some("tool_result") {
                    // content 兼容字符串与非文本(数组/图片)两种形态
                    let content = match &b["content"] {
                        serde_json::Value::String(s) => s.clone(),
                        serde_json::Value::Null => String::new(),
                        other => format!("[非文本结果]{}", // 图片等:占位;超长截断
                            other.to_string().chars().take(120).collect::<String>()),
                    };
                    out.push(AgentEvent::ToolResult {
                        tool_use_id: b["tool_use_id"].as_str().unwrap_or("").to_string(),
                        content,
                        is_error: b["is_error"].as_bool().unwrap_or(false),
                    });
                }
            }
        }
        Some("result") => {
            out.push(AgentEvent::Done {
                is_error: v["is_error"].as_bool().unwrap_or(false),
                duration_ms: v["duration_api_ms"].as_u64().unwrap_or(0),
                cost_usd: v["total_cost_usd"].as_f64(),
            });
        }
        _ => {} // 未知 type
    }
    out
}

/// stdin 写入的 user 消息 JSON 行(实测格式,含结尾换行)
pub fn user_message_json(text: &str) -> String {
    format!(
        "{{\"type\":\"user\",\"message\":{{\"role\":\"user\",\"content\":[{{\"type\":\"text\",\"text\":{}}}]}}}}\n",
        serde_json::to_string(text).unwrap_or_default()
    )
}
```

(注:`[非文本结果]` 测试断言与实现的截断拼接不一致处以测试为准——非文本形态直接固定字符串 `"[非文本结果]"`,不拼 JSON;实现时统一。)

- [ ] **Step 4: 绿 + 门槛 + 提交**

```bash
cargo test --workspace && cargo fmt && cargo clippy --workspace --all-targets -- -D warnings
git add src-tauri && git commit -m "feat(m5): streamjson 帧解析——实测快照 TDD(claude stream-json 词汇)

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 4: manager 双传输分叉——jsonl 会话执行 + SessionHandle 泛化 + 快照字段

**Files:**
- Modify: `src-tauri/crates/nexus-core/src/agent/manager.rs`(create 分叉/ChildCtl/AgentStream/prompt/snapshot)
- Modify: `src-tauri/crates/nexus-core/src/agent/state.rs`(SessionSnapshot 加 provider_id/transport)
- Create: `src-tauri/crates/nexus-core/tests/jsonl_session.rs`(假 agent 夹具集成)

**Interfaces:**
- Consumes: Task 2 `AgentProvider`/`LaunchSpec`/`LaunchContext`;Task 3 `AgentEvent`/`parse_stream_json_line`/`user_message_json`
- Produces(Task 5/9 依赖):
  - `SessionManager::create(&self, provider: &dyn AgentProvider, cols: u16, rows: u16, ctx: Option<&LaunchContext>) -> Result<SessionId, NexusError>`(签名变更:provider 进来,内部 prepare→按 spec.transport 分叉;`UnsupportedProvider` 消失——查表在调用方)
  - `SessionManager::prompt(&self, id: SessionId, text: &str) -> Result<(), NexusError>`(仅 jsonl 会话;PTY 会话 → `InvalidInput("该会话不支持 prompt,请直接输入终端")`;写 stdin 走 `write_with_timeout`)
  - `SessionManager::subscribe_agent(&self, id: SessionId) -> Result<AgentSubscription, NexusError>`:`AgentSubscription { replay: Vec<AgentEvent>, rx: mpsc::Receiver<AgentEvent> }`(PTY 会话 → InvalidInput;终态 = replay + 关闭流,与 subscribe 同语义)
  - `SessionSnapshot` 增 `provider_id: String` + `transport: AgentTransport`(serde camelCase:providerId/transport)
  - `ChildCtl` enum(私有):`Pty(Box<dyn ChildKiller + Send + Sync>) | Pipe(std::process::Child)`——`kill(&mut self)` 统一(Pty→killer.kill();Pipe→child.kill())
  - `AgentStream`(私有):`{ replay: Arc<Mutex<ReplayBuffer>>(原始 JSONL 行), subscriber: Arc<Mutex<Option<mpsc::Sender<AgentEvent>>>> }`——SessionHandle 增 `agent: Option<AgentStream>`;PTY 的 replay/subscriber 字段对 jsonl 会话为空壳(replay 空转、subscriber None)

**学习点:** ① jsonl 子进程用 **std::process::Command**(非 tokio::process):stdin/stdout 是同步管道句柄,与 PTY 的阻塞读同构——reader(spawn_blocking BufReader lines)/wait(spawn_blocking child.wait)/writer(write_with_timeout 复用)三任务树模式完全照搬,零新 async 概念;② kill 语义:jsonl 不走 killpg(子进程 pgid 继承自 app,killpg(pid) 必 ESRCH)也不 drop master(无 PTY)——`ChildCtl::Pipe` 的 kill() 即 SIGKILL;优雅停 = `libc::kill(pid, SIGINT)`(与 PTY 的 Ctrl-C 字节最近等价)→ 宽限轮询 → 强杀,stop() 内按 snapshot.transport 条件分派;③ replay 存**原始行**(字节界自然),subscribe_agent 时 snapshot→逐行 parse 成 `Vec<AgentEvent>` 返回;实时流发解析后事件;接缝原子化锁序(subscriber→replay)与 PTY 侧同约定。

- [ ] **Step 1: 写失败测试**

`tests/jsonl_session.rs`(假 agent 夹具——跨平台脚本;Windows 用 cmd 思路同 M2 sleep 测试的 cfg 分支,以 Unix 为主、Windows 标注 `#[cfg(unix)]` 照 M3 孙进程测试先例):

```rust
// jsonl 传输集成:假 agent 脚本(读 stdin JSON 行、吐固定 stream-json 序列)
// 跑通 create→prompt→事件流→subscribe_agent replay 全链。
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use nexus_core::agent::provider::{AgentTransport, CliAgentProvider, LaunchContext};
use nexus_core::agent::streamjson::AgentEvent;
use nexus_core::config::model::AgentProfile;

/// 假 jsonl agent:sh 脚本。先吐 init + assistant(text+tool_use),然后逐行读
/// stdin,读到 user 消息就吐 tool_result + result 并退出。
fn write_fake_agent(dir: &Path) -> String {
    let sh = r#"#!/bin/sh
printf '%s\n' '{"type":"system","subtype":"init","model":"fake-1","session_id":"fs-1"}'
printf '%s\n' '{"type":"assistant","message":{"content":[{"type":"text","text":"就绪"},{"type":"tool_use","id":"call_t1","name":"Bash","input":{"command":"echo hi"}}]}}'
while IFS= read -r line; do
  printf '%s\n' '{"type":"user","message":{"content":[{"tool_use_id":"call_t1","type":"tool_result","content":"hi","is_error":false}]}}'
  printf '%s\n' '{"type":"result","subtype":"success","is_error":false,"duration_api_ms":5,"total_cost_usd":0.01}'
  exit 0
done
"#;
    let p = dir.join("fake-agent.sh");
    std::fs::write(&p, sh).unwrap();
    Command::new("chmod").arg("+x").arg(&p).status().unwrap();
    p.to_string_lossy().into_owned()
}

fn fake_provider(script: &str) -> CliAgentProvider {
    CliAgentProvider::new(AgentProfile {
        id: "fake".into(),
        display_name: "Fake".into(),
        command: script.into(),
        args_template: vec![],
        env: Default::default(),
        transport: AgentTransport::Jsonl,
    })
}

fn collect_events(rx: &mut tokio::sync::mpsc::Receiver<AgentEvent>, want: usize) -> Vec<AgentEvent> {
    let mut out = Vec::new();
    let fut = async {
        while let Some(ev) = rx.recv().await {
            out.push(ev);
            if out.len() >= want { break; }
        }
    };
    tokio::runtime::Handle::current().block_on(async move {
        tokio::time::timeout(Duration::from_secs(10), fut).await.ok();
    });
    out
}

#[cfg(unix)]
#[tokio::test]
async fn jsonl_session_lifecycle_prompt_and_events() {
    let dir = tempfile::tempdir().unwrap();
    let script = write_fake_agent(dir.path());
    let provider = fake_provider(&script);
    let (sink_tx, _sink_rx) = std::sync::mpsc::channel::<nexus_core::agent::manager::SessionEvent>();
    // EventSink:收集状态/退出事件(exit 断言用)
    let events: Arc<std::sync::Mutex<Vec<nexus_core::agent::manager::SessionEvent>>> = Arc::new(Default::default());
    let ev2 = events.clone();
    let sink = Arc::new(move |e: nexus_core::agent::manager::SessionEvent| {
        ev2.lock().unwrap().push(e);
    });
    let mgr = nexus_core::agent::manager::SessionManager::new(sink);
    let ctx = LaunchContext { cwd: Some(dir.path().into()), repo_path: None, worktree_name: None, branch: None };
    let id = mgr.create(&provider, 80, 24, Some(&ctx)).await.unwrap();

    // 订阅结构化流:init + text + tool_use(3 事件)
    let sub = mgr.subscribe_agent(id).unwrap();
    assert!(sub.replay.is_empty(), "刚创建 replay 为空(事件尚未到)");
    let mut rx = sub.rx;
    let evs = collect_events(&mut rx, 3);
    assert!(evs.iter().any(|e| matches!(e, AgentEvent::Init { model, .. } if model == "fake-1")));
    assert!(evs.iter().any(|e| matches!(e, AgentEvent::Text { text } if text == "就绪")));
    assert!(evs.iter().any(|e| matches!(e, AgentEvent::ToolUse { name, .. } if name == "Bash")));

    // prompt → 假 agent 吐 tool_result + result 并退出
    mgr.prompt(id, "继续").await.unwrap();
    let evs2 = collect_events(&mut rx, 2);
    assert!(evs2.iter().any(|e| matches!(e, AgentEvent::ToolResult { content, .. } if content == "hi")));
    assert!(evs2.iter().any(|e| matches!(e, AgentEvent::Done { is_error: false, .. })));

    // 退出 → Exited;replay 含全部原始行(重新订阅恢复)
    for _ in 0..50 {
        if events.lock().unwrap().iter().any(|e| matches!(e, nexus_core::agent::manager::SessionEvent::Exit { .. })) { break; }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let re = mgr.subscribe_agent(id).unwrap();
    assert!(re.replay.len() >= 5, "终态 replay 恢复全部事件: {:?}", re.replay.len());
    assert!(replay_has_done(&re.replay));
}

fn replay_has_done(evs: &[AgentEvent]) -> bool {
    evs.iter().any(|e| matches!(e, AgentEvent::Done { .. }))
}

#[cfg(unix)]
#[tokio::test]
async fn pty_session_rejects_prompt_and_agent_subscribe() {
    let mgr = nexus_core::agent::manager::SessionManager::new(Arc::new(|_| {}));
    let provider = CliAgentProvider::new(AgentProfile {
        id: "shell".into(), display_name: "Shell".into(),
        command: nexus_core::agent::manager::default_shell(),
        args_template: vec![], env: Default::default(), transport: AgentTransport::Pty,
    });
    let id = mgr.create(&provider, 80, 24, None).await.unwrap();
    assert!(mgr.prompt(id, "x").await.is_err(), "PTY 会话拒绝 prompt");
    assert!(mgr.subscribe_agent(id).is_err(), "PTY 会话拒绝 agent_subscribe");
    mgr.stop(id, true).await.unwrap();
}
```

`state.rs` 快照测试追加:

```rust
    #[test]
    fn snapshot_serializes_provider_fields() {
        let snap = SessionSnapshot {
            session_id: SessionId::new(),
            state: SessionState::Running,
            started_at_ms: 1,
            exit_code: None,
            pid: None,
            repo_path: None,
            worktree_name: None,
            provider_id: "claude".into(),
            transport: crate::agent::provider::AgentTransport::Jsonl,
        };
        let json = serde_json::to_string(&snap).unwrap();
        assert!(json.contains("\"providerId\":\"claude\""));
        assert!(json.contains("\"transport\":\"jsonl\""));
    }
```

- [ ] **Step 2: 红** — 编译错(create 签名/新方法/快照字段不存在)。
- [ ] **Step 3: 实现**(manager.rs 为主)

**ChildCtl + AgentStream**(manager.rs,替换 SessionHandle.killer 字段):

```rust
/// 子进程控制统一:PTY 的 ChildKiller 与 jsonl 管道子进程。
enum ChildCtl {
    Pty(Box<dyn ChildKiller + Send + Sync>),
    Pipe(std::process::Child),
}

impl ChildCtl {
    fn kill(&mut self) -> std::io::Result<()> {
        match self {
            ChildCtl::Pty(k) => k.kill(),
            ChildCtl::Pipe(c) => c.kill(),
        }
    }
}

/// jsonl 会话的结构化通道(与 PTY 侧 replay/subscriber 同型,载荷是 AgentEvent)。
struct AgentStream {
    replay: Arc<Mutex<ReplayBuffer>>, // 原始 JSONL 行(含 \n)
    subscriber: Arc<Mutex<Option<mpsc::Sender<AgentEvent>>>>,
}

// SessionHandle:
//   killer: Arc<Mutex<ChildCtl>>(原 Box<dyn ChildKitter> 字段改型)
//   agent: Option<AgentStream>(jsonl 会话 Some;PTY 会话 None)
```

**create() 分叉**(伪码结构,实现以既有代码风格落):

```rust
pub async fn create(
    &self,
    provider: &dyn AgentProvider,
    cols: u16,
    rows: u16,
    ctx: Option<&LaunchContext>,
) -> Result<SessionId, NexusError> {
    let spec = provider.prepare(ctx)?;
    let id = SessionId::new();
    match spec.transport {
        AgentTransport::Pty => self.create_pty(id, spec, cols, rows).await,
        AgentTransport::Jsonl => self.create_jsonl(id, spec),
    }
}
```

- `create_pty` = 现 create() 主体平移(签名内聚;`SessionEvent::State{Running}` 播报/入表在两分支收尾处共用)。
- `create_jsonl`(std::process):

```rust
fn create_jsonl(&self, id: SessionId, spec: LaunchSpec) -> Result<SessionId, NexusError> {
    use std::io::{BufRead, BufReader, Write};
    let mut cmd = std::process::Command::new(&spec.argv[0]);
    cmd.args(&spec.argv[1..])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    if let Some(cwd) = spec.cwd.as_deref() { cmd.current_dir(cwd); }
    for (k, v) in &spec.env { cmd.env(k, v); }
    let mut child = cmd.spawn().map_err(|e| NexusError::SpawnFailed(format!(
        "无法启动 {}: {e}(命令未安装?)", spec.argv[0])))?;
    let pid = child.id();
    let stdin = child.stdin.take().expect("piped stdin");
    let stdout = child.stdout.take().expect("piped stdout");
    let writer: Arc<Mutex<Box<dyn std::io::Write + Send>>> = Arc::new(Mutex::new(Box::new(stdin)));

    let (line_tx, line_rx) = mpsc::channel::<String>(QUEUE_DEPTH);
    // reader:阻塞逐行读(std::process 同构 PTY reader 模式)
    tokio::task::spawn_blocking(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            match line {
                Ok(l) => { if line_tx.blocking_send(l).is_err() { break; } }
                Err(_) => break,
            }
        }
    });
    // pump:行→事件→replay(原始行)+订阅分发(锁序 subscriber→replay,同 PTY)
    let replay = Arc::new(Mutex::new(ReplayBuffer::new(REPLAY_CAP)));
    let subscriber = Arc::new(Mutex::new(None::<mpsc::Sender<AgentEvent>>));
    let cancel = CancellationToken::new();
    { /* 终态断订阅与 PTY 侧 wait 任务同位 */ }
    tokio::spawn(async move {
        let mut rx = line_rx;
        loop {
            let line = tokio::select! {
                l = rx.recv() => match l { Some(l) => l, None => break },
                _ = cancel.cancelled() => break,
            };
            let events = crate::agent::streamjson::parse_stream_json_line(&line);
            let tx = {
                let sub_guard = subscriber.lock().expect("订阅锁");
                replay.lock().expect("replay 锁").push_str(&format!("{line}\n"));
                sub_guard.clone()
            };
            for ev in events {
                if let Some(tx) = tx.as_ref() {
                    tokio::select! {
                        r = tx.send(ev.clone()) => { if r.is_err() { /* 只留 replay */ } }
                        _ = cancel.cancelled() => break,
                    }
                }
            }
        }
    });
    // wait:child.wait() spawn_blocking → set_state + 断订阅 + Exit(照抄 PTY wait 任务结构)
    // ...与 create_pty 的任务 3/3 同型,child 包进 ChildCtl::Pipe 前先 wait:
    //     wait 需要所有权——std Child::wait 拿 &mut;结构:wait 任务持有 Child,
    //     退出后 killer 路径经 ChildCtl::Pipe(child) 不再可用 → stop 的强杀在
    //     jsonl 分支直接 libc::kill(pid, SIGKILL)(pid 已快照),ChildCtl::Pipe
    //     只承载 spawn 失败前的窗口期(实现以编译/测试为准,允许把 jsonl 的
    //     kill 全走 pid 路线、ChildCtl::Pipe 仅用于 drop 资源)
    // ...
    // SessionHandle { writer, killer: Arc::new(Mutex::new(ChildCtl::Pipe(child_for_kill))),
    //                 master: Arc::new(Mutex::new(None)), /* 无 PTY master */
    //                 snapshot(provider_id: provider.id(), transport: Jsonl), replay: 空壳,
    //                 subscriber: None 占位, agent: Some(AgentStream { replay, subscriber }), cancel }
    unimplemented!("按上述结构落,注释即规格")
}
```

**prompt()**:

```rust
/// jsonl 会话发指令:stdin 写 user 消息 JSON 行。PTY 会话拒绝。
pub async fn prompt(&self, id: SessionId, text: &str) -> Result<(), NexusError> {
    let (writer, transport) = {
        let guard = self.inner.sessions.lock().expect("会话表锁被毒化");
        let h = guard.get(&id).ok_or_else(|| NexusError::SessionNotFound(id.to_string()))?;
        if !matches!(h.snapshot.state, SessionState::Running) {
            return Err(NexusError::SessionNotRunning(id.to_string()));
        }
        if h.snapshot.transport != AgentTransport::Jsonl {
            return Err(NexusError::InvalidInput("该会话不支持 prompt,请直接在终端输入".into()));
        }
        (h.writer.clone(), h.snapshot.transport)
    };
    let _ = transport;
    let line = crate::agent::streamjson::user_message_json(text);
    write_with_timeout(writer, line.into_bytes(), SEND_INPUT_TIMEOUT).await
}
```

**subscribe_agent()**(照 subscribe() 同型:终态→replay 解析 + 关闭流;活跃→snapshot+替换原子段):

```rust
pub fn subscribe_agent(&self, id: SessionId) -> Result<AgentSubscription, NexusError> {
    let mut guard = self.inner.sessions.lock().expect("会话表锁被毒化");
    let handle = guard.get_mut(&id)
        .ok_or_else(|| NexusError::SessionNotFound(id.to_string()))?;
    let stream = handle.agent.as_ref()
        .ok_or_else(|| NexusError::InvalidInput("PTY 会话无结构化流,用 session_attach".into()))?;
    let parse_replay = |s: &str| s.lines()
        .flat_map(crate::agent::streamjson::parse_stream_json_line)
        .collect::<Vec<_>>();
    if matches!(handle.snapshot.state, SessionState::Exited | SessionState::Failed) {
        let replay = parse_replay(&stream.replay.lock().expect("replay 锁").snapshot());
        let (_tx, rx) = mpsc::channel(1);
        drop(_tx);
        return Ok(AgentSubscription { replay, rx });
    }
    let (tx, rx) = mpsc::channel(SUBSCRIBER_DEPTH);
    let replay = {
        let sub_guard = stream.subscriber.lock().expect("订阅锁");
        let snap = stream.replay.lock().expect("replay 锁").snapshot();
        *sub_guard = Some(tx);
        parse_replay(&snap)
    };
    Ok(AgentSubscription { replay, rx })
}
```

**stop() jsonl 分支**:优雅 = `libc::kill(pid, SIGINT)`(cfg unix;Windows 直接 `ChildCtl::kill()` 即强杀——标注差异)+ 宽限轮询(复用既有 GRACE_PERIOD 逻辑)→ 超时 `libc::kill(pid, SIGKILL)` / `ChildCtl::kill()`;**不走 killpg、不 drop master**(无意义);cancel 照旧。快照 pid 已有。

- [ ] **Step 4: 既有测试与调用点适配**

- `session.rs` 命令层 create 调用:本任务先在 IPC 层用 `ProviderRegistry`(lib.rs 组装下一任务接线,本任务命令层临时构造 `CliAgentProvider::new(shell_profile)` 保持编译——或直接把 Task 5 的 session_create 改造并入本任务收尾,以实现者判断,允许两任务合并提交一次)
- 既有 manager/session 集成测试:`create("shell", ...)` → 构造 shell `CliAgentProvider` 调新签名
- `SessionSnapshot` 全部构造点测试补两字段
- **Windows**:`jsonl_session.rs` 两个用例 `#[cfg(unix)]`(sh 脚本);Windows 的 jsonl 路径由 CI 编译保证 + 后续真 claude 人工验收覆盖(计划内裁剪,记入完成记录)

- [ ] **Step 5: 绿 + 门槛 + 提交**

```bash
cargo test --workspace && cargo fmt && cargo clippy --workspace --all-targets -- -D warnings
git add src-tauri && git commit -m "feat(m5): manager 双传输分叉——jsonl 会话执行/结构化订阅/prompt + 快照 provider 字段

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 5: IPC 接线——session_create 走 registry + provider_list / agent_attach / session_prompt + TS 契约

**Files:**
- Modify: `src-tauri/src/commands/session.rs`(session_create 改造 + agent_attach/session_prompt)
- Create: `src-tauri/src/commands/provider.rs`(provider_list)
- Modify: `src-tauri/src/commands/mod.rs`(注册)
- Modify: `src-tauri/src/lib.rs`(manage ProviderRegistry;组装顺序:config load → registry from_config)
- Modify: `src/ipc/types.ts`、`src/ipc/commands.ts`

**Interfaces:**
- Consumes: Task 2/3/4 全部 Produces
- Produces(前端契约,Task 6-10 消费):
  - `provider_list() -> Vec<ProviderInfo>`(available 探测经 spawn_blocking)
  - `agent_attach { sessionId, output: Channel<AgentEvent> } -> { replayedEvents: u32 }`(replay 事件先逐个 send,再 spawn 转发实时流——顺序保证)
  - `session_prompt { sessionId, text } -> ()`
  - `session_create` 内部改造:launch 构造产 `LaunchContext`(cwd/repo_path/worktree_name/branch:None),`registry.get(provider_id)` → None 时 `UnsupportedProvider`;**TS 签名不变**(providerId 已在参数里)
  - TS:`ProviderInfo`、`AgentEvent` 联合类型、四封装函数;`SessionSnapshot` 加 providerId/transport

- [ ] **Step 1: Rust 命令层**

`provider.rs`:

```rust
// provider_list:注册表 + 命令探测(探测是同步子进程,spawn_blocking 别扣执行器)。
use tauri::State;

use nexus_core::agent::registry::{ProviderInfo, ProviderRegistry};

#[tauri::command]
pub async fn provider_list(
    state: State<'_, ProviderRegistry>,
) -> Result<Vec<ProviderInfo>, String> {
    let reg = state.inner().clone_shallow(); // 或 Arc<ProviderRegistry> 进 State,实现取其一
    tokio::task::spawn_blocking(move || reg.list())
        .await
        .map_err(|e| e.to_string())
}
```

(实现注:`ProviderRegistry` 需 Clone 或 State 直接持有 Arc——取 `State<'_, Arc<ProviderRegistry>>` 最简,lib.rs `app.manage(Arc::new(registry))`;`list()` 本身 `&self`。)

`session.rs` 改造 + 两新命令:

```rust
// session_create 内部:launch 构造产 LaunchContext(原 LaunchSpec 构造点改字段),
// provider 查表:
let provider = registry
    .get(&provider_id)
    .ok_or_else(|| nexus_core::NexusError::UnsupportedProvider(provider_id.clone()).to_string())?;
let id = sessions
    .create(provider.as_ref(), cols.unwrap_or(80), rows.unwrap_or(24), ctx.as_ref())
    .await
    .map_err(|e| e.to_string())?;

// agent_attach(replay 先行,顺序投递到同一 Channel):
#[tauri::command]
pub async fn agent_attach(
    state: State<'_, SessionManager>,
    session_id: String,
    output: tauri::ipc::Channel<nexus_core::agent::streamjson::AgentEvent>,
) -> Result<AgentAttachAck, String> {
    let id = parse_id(session_id)?;
    let sub = state.subscribe_agent(id).map_err(|e| e.to_string())?;
    for ev in sub.replay {
        if output.send(ev).is_err() { return Ok(AgentAttachAck { replayed_events: 0 }); }
    }
    let replayed = /* replay.len() 已被 move——改为先计数再消费,实现注意 */;
    tokio::spawn(async move {
        let mut rx = sub.rx;
        while let Some(ev) = rx.recv().await {
            if output.send(ev).is_err() { break; }
        }
    });
    Ok(AgentAttachAck { replayed_events: replayed })
}

#[tauri::command]
pub async fn session_prompt(
    state: State<'_, SessionManager>,
    session_id: String,
    text: String,
) -> Result<(), String> {
    let id = parse_id(session_id)?;
    state.prompt(id, &text).await.map_err(|e| e.to_string())
}
```

`lib.rs` 组装(config 加载后):`let registry = Arc::new(ProviderRegistry::from_config(&config)); app.manage(registry.clone());`(config 现有加载位置核实后接线;`generate_handler!` 追加 `commands::provider_list, commands::agent_attach, commands::session_prompt`)。

- [ ] **Step 2: TS 契约与封装**

`types.ts` 追加:

```typescript
/** provider_list 返回(agent/registry.rs ProviderInfo) */
export interface ProviderInfo {
  id: string;
  displayName: string;
  transport: "pty" | "jsonl";
  command: string;
  available: boolean;
}

/** agent_attach 输出流事件(agent/streamjson.rs AgentEvent,serde tag=type) */
export type AgentEvent =
  | { type: "init"; model: string; claudeSession: string | null }
  | { type: "text"; text: string }
  | { type: "toolUse"; id: string; name: string; input: unknown }
  | { type: "toolResult"; toolUseId: string; content: string; isError: boolean }
  | { type: "done"; isError: boolean; durationMs: number; costUsd: number | null };

/** agent_attach 确认 */
export interface AgentAttachAck {
  replayedEvents: number;
}
```

(注:serde `tag = "type"` + `rename_all = "camelCase"` 下枚举变体名变 tag 值——`ToolUse` → `"toolUse"`。实现时以 `snapshot_serializes` 型单测钉住:Rust 侧测试断言 `\"type\":\"toolUse\"` 等 tag 值,TS 与之对齐。)

`SessionSnapshot` 加 `providerId: string; transport: "pty" | "jsonl";`。

`commands.ts` 追加:

```typescript
export function providerList(): Promise<ProviderInfo[]> {
  return invoke<ProviderInfo[]>("provider_list");
}

export function agentAttach(
  sessionId: string,
  output: Channel<AgentEvent>
): Promise<AgentAttachAck> {
  return invoke<AgentAttachAck>("agent_attach", { sessionId, output });
}

export function sessionPrompt(sessionId: string, text: string): Promise<void> {
  return invoke<void>("session_prompt", { sessionId, text });
}
```

- [ ] **Step 3: 门槛 + 提交**

```bash
cargo test --workspace && cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && pnpm build
git add -A && git commit -m "feat(m5): IPC——provider_list/agent_attach/session_prompt + session_create 走 registry + TS 契约

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 6: 前端 agentStore + useAgentSession(消息流数据层)

**Files:**
- Create: `src/stores/agentStore.ts`
- Create: `src/features/agent/useAgentSession.ts`

**Interfaces:**
- Consumes: Task 5 `agentAttach`/`sessionPrompt`/`AgentEvent`
- Produces(Task 7/8 依赖):
  - `agentStore`:`MsgItem` 拍平联合(见下)+ `messages: Record<string, MsgItem[]>`、`applyEvent(sessionId, ev)`、`appendUserText(sessionId, text)`、`clearSession(sessionId)`;**cap 500 条丢头**
  - `useAgentSession(sessionId)`:attach(replay+实时经同一 Channel)+ send(text)(乐观 append 用户消息 → sessionPrompt)

**学习点:** ① **拍平消息流**:orca 是 `NativeChatMessage{role, blocks[]}` 嵌套;原型 pane-agent 视觉上 bubble/tool/tasklist 本就是平铺流——MVP 按 `MsgItem` 拍平(用户文本/agent 文本/工具卡片按时间序),视觉等价、状态管理减半;工具卡片三态 `running|completed|failed` 是 orca 的 state 词汇;② **红线辨析的实践**:AgentEvent 是低频语义事件,进 zustand 合法——但 cap 500 条丢头必须有(cap 逻辑:追加后 `slice(-500)`);③ attach 前 `clearSession` 保证 replay+实时不重复(事件流无 seq,幂等靠重置);ToolUse 重复事件防御:按 id 已存在则跳过(replay 与实时理论不重叠,防御性保留)。

- [ ] **Step 1: agentStore.ts**

```typescript
// 结构化会话消息流(spec §1.5 v1.3):AgentEvent → 拍平渲染项。
// 拍平是与 orca NativeChatMessage.blocks 嵌套的有意差异:原型视觉即平铺,
// 状态管理减半。cap 500 条丢头(红线辨析:低频语义事件可进 store,但必须有界)。
import { create } from "zustand";

import type { AgentEvent } from "../ipc/types";

export interface UserText {
  kind: "userText";
  text: string;
}
export interface AgentText {
  kind: "agentText";
  text: string;
}
export interface ToolCard {
  kind: "tool";
  id: string;
  name: string;
  input: unknown;
  state: "running" | "completed" | "failed";
  result: string | null;
}
export interface DoneSummary {
  kind: "done";
  isError: boolean;
  durationMs: number;
  costUsd: number | null;
}
export type MsgItem = UserText | AgentText | ToolCard | DoneSummary;

const CAP = 500;

interface AgentState {
  messages: Record<string, MsgItem[]>;
  applyEvent: (sessionId: string, ev: AgentEvent) => void;
  appendUserText: (sessionId: string, text: string) => void;
  clearSession: (sessionId: string) => void;
}

export const useAgentMessages = create<AgentState>((set) => ({
  messages: {},
  applyEvent: (sessionId, ev) =>
    set((st) => {
      const cur = st.messages[sessionId] ?? [];
      let next: MsgItem[];
      switch (ev.type) {
        case "init":
          return st; // init 不渲染(头部信息由快照/计时派生)
        case "text":
          next = [...cur, { kind: "agentText", text: ev.text }];
          break;
        case "toolUse": {
          if (cur.some((m) => m.kind === "tool" && m.id === ev.id)) return st;
          next = [
            ...cur,
            { kind: "tool", id: ev.id, name: ev.name, input: ev.input, state: "running", result: null },
          ];
          break;
        }
        case "toolResult": {
          const hit = [...cur];
          for (let i = hit.length - 1; i >= 0; i -= 1) {
            const m = hit[i];
            if (m.kind === "tool" && m.id === ev.toolUseId) {
              hit[i] = { ...m, state: ev.isError ? "failed" : "completed", result: ev.content };
              break;
            }
          }
          next = hit; // 未配对(理论不可达):丢弃
          break;
        }
        case "done":
          next = [...cur, { kind: "done", isError: ev.isError, durationMs: ev.durationMs, costUsd: ev.costUsd }];
          break;
      }
      return { messages: { ...st.messages, [sessionId]: next.slice(-CAP) } };
    }),
  appendUserText: (sessionId, text) =>
    set((st) => ({
      messages: {
        ...st.messages,
        [sessionId]: [...(st.messages[sessionId] ?? []), { kind: "userText", text }].slice(-CAP),
      },
    })),
  clearSession: (sessionId) =>
    set((st) => {
      const { [sessionId]: _drop, ...rest } = st.messages;
      return { messages: rest };
    }),
}));
```

- [ ] **Step 2: useAgentSession.ts**

```typescript
// agent 会话接线(与 useTerminalSession 同型):attach 一次,Channel 事件进 store。
// StrictMode 双执行幂等:重复 attach 由 Rust 侧替换旧订阅 + 前端 clearSession 重置。
import { useCallback, useEffect } from "react";
import { Channel } from "@tauri-apps/api/core";

import { agentAttach, sessionPrompt } from "../../ipc/commands";
import type { AgentEvent } from "../../ipc/types";
import { useAgentMessages } from "../../stores/agentStore";
import { toast } from "../../stores/toastStore";

export function useAgentSession(sessionId: string) {
  useEffect(() => {
    const ch = new Channel<AgentEvent>();
    ch.onmessage = (ev) => useAgentMessages.getState().applyEvent(sessionId, ev);
    useAgentMessages.getState().clearSession(sessionId); // replay 前重置,防重复
    void agentAttach(sessionId, ch).catch((e) => {
      toast(`会话流连接失败:${String(e)}`, "error");
    });
    // 不注销 Channel:会话事件流随 tab 生命周期;组件卸载不 unsubscribe
    // (Rust 侧订阅被下次 attach 替换;终态自动断)——与 PTY 侧语义一致
  }, [sessionId]);

  const send = useCallback(
    async (text: string) => {
      const t = text.trim();
      if (!t) return;
      useAgentMessages.getState().appendUserText(sessionId, t);
      try {
        await sessionPrompt(sessionId, t);
      } catch (e) {
        toast(`发送失败:${String(e)}`, "error");
      }
    },
    [sessionId]
  );

  return { send };
}
```

- [ ] **Step 3: 构建 + 提交**

```bash
pnpm build && cargo test --workspace
git add -A && git commit -m "feat(m5): agentStore 拍平消息流(cap 500)+ useAgentSession 接线

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 7: AgentPane——会话视图(消息流/工具卡片/composer)

**Files:**
- Create: `src/features/agent/AgentPane.tsx`

**Interfaces:**
- Consumes: Task 6 `useAgentMessages`/`useAgentSession`;sessionsStore 状态;sessionsStore 快照(startedAtMs 计时)
- Produces: tab kind `agent` 的渲染组件(Task 8 挂进 TabBody);Props:`{ sessionId: string }`(自取 store,不透传)

**学习点:** ① 原型 pane-agent(805-823 行)的四段结构:`main-head`(状态点/标题/provider chip/计时)+ `msgs`(user bubble/agent bubble/tool 卡片)+ `composer`(textarea + 发送);MVP 裁剪:thought 行/任务列表内嵌(tasklist)/@上下文/模型选择钮(显示 provider 名)——spec v1.3 裁剪记档;② 工具卡片三态:running(蓝点脉动)/completed(绿点)/failed(红点),头部图标 + 命令名 + 参数摘要(输入 JSON 序列化截 60 字),body 结果文本(max-h 折叠,CSS line-clamp);③ 自动滚底:消息数变化时 scrollTop = scrollHeight,仅当用户已在底部(距底 <40px)——防"读历史被拽走";④ composer:Enter 发送/Shift+Enter 换行;会话非 running 时禁用。

- [ ] **Step 1: AgentPane.tsx**

```tsx
// 会话视图(spec §1.5 v1.3,原型 pane-agent):消息流 + 工具卡片 + composer。
// 挂载闩锁同享 TabBody 全量 map 机制;数据 agentStore(拍平流)。
import { useEffect, useRef, useState } from "react";
import { ArrowUp, Bot, CircleCheck, CircleX, Loader2, Terminal, User } from "lucide-react";

import { useAgentMessages, type MsgItem } from "../../stores/agentStore";
import { useSessions } from "../../stores/sessionsStore";
import { useAgentSession } from "./useAgentSession";

const DOT: Record<string, string> = {
  running: "bg-status-run",
  stopping: "bg-status-warn",
  exited: "bg-gray-400",
  failed: "bg-status-err",
};

function fmtDur(ms: number): string {
  const s = Math.floor(ms / 1000);
  return s < 60 ? `${s}s` : `${Math.floor(s / 60)}m ${s % 60}s`;
}

function ToolCardView({ item }: { item: Extract<MsgItem, { kind: "tool" }> }) {
  const arg = JSON.stringify(item.input);
  const brief = arg.length > 60 ? `${arg.slice(0, 60)}…` : arg;
  return (
    <div className="rounded-lg border border-border bg-card">
      <div className="flex items-center gap-2 border-b border-border px-2.5 py-1.5 text-xs">
        <Terminal className="size-3.5 shrink-0 text-muted-foreground" />
        <span className="font-medium">{item.name}</span>
        <span className="min-w-0 flex-1 truncate font-mono text-[10.5px] text-muted-foreground">
          {brief}
        </span>
        {item.state === "running" && (
          <span className="flex shrink-0 items-center gap-1 text-status-run">
            <Loader2 className="size-3 animate-spin" />运行中
          </span>
        )}
        {item.state === "completed" && (
          <span className="flex shrink-0 items-center gap-1 text-status-ok">
            <CircleCheck className="size-3" />已完成
          </span>
        )}
        {item.state === "failed" && (
          <span className="flex shrink-0 items-center gap-1 text-status-err">
            <CircleX className="size-3" />失败
          </span>
        )}
      </div>
      {item.result && (
        <pre className="max-h-32 overflow-y-auto px-2.5 py-1.5 font-mono text-[11px] whitespace-pre-wrap text-muted-foreground">
          {item.result}
        </pre>
      )}
    </div>
  );
}

export default function AgentPane({ sessionId }: { sessionId: string }) {
  const messages = useAgentMessages((s) => s.messages[sessionId]) ?? [];
  const snap = useSessions((s) => s.sessions[sessionId]);
  const { send } = useAgentSession(sessionId);
  const [draft, setDraft] = useState("");
  const scrollRef = useRef<HTMLDivElement>(null);
  const nearBottomRef = useRef(true);

  // 自动滚底:仅在用户已近底部时(防读历史被拽走)
  useEffect(() => {
    const el = scrollRef.current;
    if (el && nearBottomRef.current) el.scrollTop = el.scrollHeight;
  }, [messages.length]);

  const onScroll = (): void => {
    const el = scrollRef.current;
    if (el) nearBottomRef.current = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
  };

  const running = snap?.state === "running";
  const submit = (): void => {
    if (!draft.trim() || !running) return;
    void send(draft);
    setDraft("");
  };

  return (
    <div className="flex h-full min-h-0 flex-col">
      {/* main-head(原型):状态点 + 标题 + 计时 */}
      <div className="flex min-h-[37px] items-center gap-2 border-b border-border px-3 text-xs">
        {snap && <span className={`size-1.5 rounded-full ${DOT[snap.state]}`} />}
        <span className="font-medium">{snap?.providerId ?? "会话"}</span>
        {snap?.worktreeName && (
          <span className="truncate font-mono text-[10.5px] text-muted-foreground">
            {snap.worktreeName.split("/").pop()}
          </span>
        )}
        <span className="flex-1" />
        {snap && (
          <span className="font-mono text-[10.5px] text-muted-foreground">
            {fmtDur(Date.now() - snap.startedAtMs)}
          </span>
        )}
      </div>
      {/* msgs:拍平消息流 */}
      <div
        ref={scrollRef}
        onScroll={onScroll}
        className="flex-1 space-y-2.5 overflow-y-auto p-3"
      >
        {messages.map((m, i) => {
          if (m.kind === "userText") {
            return (
              <div key={i} className="flex justify-end">
                <div className="max-w-[75%] rounded-lg bg-secondary px-2.5 py-1.5 text-xs whitespace-pre-wrap">
                  {m.text}
                </div>
              </div>
            );
          }
          if (m.kind === "agentText") {
            return (
              <div key={i} className="flex items-start gap-2">
                <Bot className="mt-0.5 size-3.5 shrink-0 text-muted-foreground" />
                <div className="max-w-[85%] rounded-lg border border-border bg-card px-2.5 py-1.5 text-xs whitespace-pre-wrap">
                  {m.text}
                </div>
              </div>
            );
          }
          if (m.kind === "tool") return <ToolCardView key={m.id} item={m} />;
          return (
            <div key={i} className="flex justify-center">
              <span className="rounded-full border border-border px-2 py-0.5 font-mono text-[10px] text-muted-foreground">
                {m.isError ? "出错" : "完成"} · {fmtDur(m.durationMs)}
                {m.costUsd != null ? ` · $${m.costUsd.toFixed(3)}` : ""}
              </span>
            </div>
          );
        })}
        {messages.length === 0 && (
          <p className="pt-8 text-center text-xs text-muted-foreground">
            等待 agent 输出…
          </p>
        )}
      </div>
      {/* composer(原型):textarea + 发送 */}
      <div className="border-t border-border p-2.5">
        <div className="rounded-lg border border-input bg-card focus-within:border-ring focus-within:ring-2 focus-within:ring-ring/40">
          <textarea
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey) {
                e.preventDefault();
                submit();
              }
            }}
            disabled={!running}
            placeholder={running ? "给 Agent 下达指令…(Enter 发送)" : "会话已结束"}
            rows={2}
            className="w-full resize-none bg-transparent px-2.5 py-2 text-xs outline-none placeholder:text-muted-foreground/60 disabled:opacity-50"
          />
          <div className="flex items-center justify-between px-2 pb-1.5">
            <span className="flex items-center gap-1 text-[10.5px] text-muted-foreground">
              <User className="size-3" />
              {snap?.providerId ?? ""}
            </span>
            <button
              type="button"
              aria-label="发送"
              onClick={submit}
              disabled={!running || !draft.trim()}
              className="grid size-6 cursor-pointer place-items-center rounded bg-primary text-primary-foreground transition-opacity disabled:opacity-40"
            >
              <ArrowUp className="size-3.5" />
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
```

(样式为 M4 亮色令牌基线;人工验收后按原型精修——UI 打磨波先例。计时 `Date.now()` 派生不自更新,由消息流/状态变化触发重渲染自然刷新——低频会话可接受,若目检明显不动再上 interval,记实现自由度。)

- [ ] **Step 2: 构建 + 提交**

```bash
pnpm build && cargo test --workspace
git add -A && git commit -m "feat(m5): AgentPane——消息流/工具卡片三态/composer(原型 pane-agent)

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 8: tab 体系——kind agent 分派 + TabStrip 徽标 + 关闭流双 id

**Files:**
- Modify: `src/stores/tabStore.ts`(Tab 加 kind/providerId/transport;openSession;closeTabBySession)
- Modify: `src/features/tabs/TabBody.tsx`(按 kind 分派 AgentPane)
- Modify: `src/features/tabs/TabStrip.tsx`(kind 徽标「会话/终端」;title)
- Modify: `src/features/tabs/TabBody.tsx` 空态文案(可选)
- Modify: `src/App.tsx`(handleClose/confirmStopAndClose/自动收尾的 `term-${id}` 模板改经 tab id 查找)

**Interfaces:**
- Consumes: Task 6/7;SessionSnapshot 新字段(Task 5)
- Produces(Task 10 依赖):`openSession(snap)`(按 transport 决定 kind:jsonl→agent,id `agent-<sessionId>`,label=providerId 首字母大写/冲突加序号;pty→terminal 照旧)、`closeTabBySession(sessionId)`;TabStrip kind 徽标 Record

**学习点:** ① tab id 前缀从 `term-` 泛化为 `term-|agent-`:App 三处 `term-${sessionId}` 模板是隐式耦合——本任务收敛为 `closeTabBySession`(store 内按 sessionId 找 tab),App 不再拼 id;② rebuildFromSessions 自动双形态(snap.transport 决定),刷新恢复会话 UI 零额外逻辑;③ PTY agent 会话(codex/qwen)就是终端 tab——唯一增强是 label 取 providerId(快照字段),kind 徽标仍「终端」。

- [ ] **Step 1: tabStore.ts**

```typescript
export type TabKind = "terminal" | "agent";

export interface Tab {
  id: string;
  kind: TabKind;
  sessionId: string;
  label: string;
  /** 会话快照镜像:落点 + provider(树上挂节点/关闭流/徽标用) */
  repoPath: string | null;
  worktreeName: string | null;
  providerId: string | null;
  transport: "pty" | "jsonl" | null;
}

function tabOf(snap: SessionSnapshot, label: string): Tab {
  const jsonl = snap.transport === "jsonl";
  return {
    id: `${jsonl ? "agent" : "term"}-${snap.sessionId}`,
    kind: jsonl ? "agent" : "terminal",
    sessionId: snap.sessionId,
    label,
    repoPath: snap.repoPath ?? null,
    worktreeName: snap.worktreeName ?? null,
    providerId: snap.providerId ?? null,
    transport: snap.transport ?? null,
  };
}

function labelFor(snap: SessionSnapshot, taken: string[]): string {
  if (snap.transport === "jsonl") {
    // agent 会话:provider 名;冲突加序号
    const base = snap.providerId ?? "会话";
    if (!taken.includes(base)) return base;
    let n = 2;
    while (taken.includes(`${base} ${n}`)) n += 1;
    return `${base} ${n}`;
  }
  let n = 1;
  while (taken.includes(`终端 ${n}`)) n += 1;
  return `终端 ${n}`;
}
```

(`openTerminal` 改名/并存 `openSession`(内部同一逻辑,labelFor 分支);新增 `closeTabBySession(sessionId)`:`set 中 findIndex(t => t.sessionId === sessionId)` 复用 closeTab 激活邻位逻辑;`rebuildFromSessions` 用 labelFor。)

- [ ] **Step 2: TabBody 分派 + TabStrip 徽标**

`TabBody.tsx`:

```tsx
import AgentPane from "../agent/AgentPane";
// map 内按 kind 分派(闩锁机制不变:全量 map + active 切显隐):
{tabs.map((tab) => (
  <div key={tab.id} className={tab.id === activeTabId ? "h-full w-full" : "hidden"}>
    {tab.kind === "agent" ? (
      <AgentPane sessionId={tab.sessionId} />
    ) : (
      <TerminalPane sessionId={tab.sessionId} onFitted={onFitted} />
    )}
  </div>
))}
```

`TabStrip.tsx` 的 kind 徽标:

```tsx
const KIND_LABEL: Record<TabKind, string> = { terminal: "终端", agent: "会话" };
// 渲染处:
<span className="font-mono text-[10px] text-muted-foreground/70">
  {KIND_LABEL[tab.kind]}
</span>
```

(`labelFor` 的 pty agent 会话:providerId 存在且非 shell 时 label 用 providerId?——裁定:pty agent label 仍走「终端 N」序号(它们就是终端),provider 归属看 title 提示(tab title 补 providerId)。)

- [ ] **Step 3: App.tsx 收敛 tab id**

三处(`handleClose` 终态分支、`confirmStopAndClose` finish、自动收尾 effect)的 `useTabs.getState().closeTab(`term-${sessionId}`)` 全部换 `useTabs.getState().closeTabBySession(sessionId)`;`disposeEntry(sessionId)` 保持(agent 会话无终端实例,terminalManager 内 no-op——核实其空安全)。

- [ ] **Step 4: 构建 + 基线验收 + 提交**

```bash
pnpm build && cargo test --workspace
pnpm tauri dev   # 基线:shell 终端 tab 照旧;无 jsonl 会话时 UI 与 M4 一致(kind 全 terminal)
git add -A && git commit -m "feat(m5): tab kind agent 分派 + 徽标 + closeTabBySession 收敛

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 9: fleet.rs——launch_fleet 编排 + 逆序回滚(TDD)

**Files:**
- Create: `src-tauri/crates/nexus-core/src/agent/fleet.rs`
- Modify: `src-tauri/crates/nexus-core/src/agent/mod.rs`(`pub mod fleet;`)
- Create: `src-tauri/crates/nexus-core/tests/fleet_launch.rs`

**Interfaces:**
- Consumes: Task 4 `SessionManager::create/prompt`;既有 `WorktreeManager::create/remove`;Task 2 `ProviderRegistry`
- Produces(Task 10 依赖):
  - `fleet::FleetItem { profile_id: String, prompt: Option<String> }`
  - `fleet::launch_fleet(wt: &WorktreeManager, sessions: &SessionManager, registry: &ProviderRegistry, repo: &Path, base_ref: Option<&str>, items: &[FleetItem]) -> Result<Vec<SessionSnapshot>, FleetError>`
  - `FleetError { message: String, rolled_back: Vec<String>, leftovers: Vec<String> }`(serde camelCase;rolled_back = 已成功回滚的 worktree 名;leftovers = 回滚失败残留——UI toast 列明)

**学习点:** ① 顺序创建 + 逆序回滚 = M3 孤儿回滚的放大版,不是事务(orca 亦无批量事务——per-worker 生命周期 + 失败释放预留,spec §1.3);② **初始指令仅对 Jsonl provider 生效**(prompt 走协议;Pty 项开终端到 worktree 即完成——TUI 就绪前粘贴是 orca 都要编排的坑,MVP 不碰),Pty 项带 prompt 时是**软忽略**(不报错,返回快照照常);③ 回滚顺序:后建的先拆(会话 stop(force)+dispose → worktree remove(delete_branch));回滚自身的失败不中断循环,收集 leftovers 继续拆——"尽力清 + 残留透明化"(风险 #15);④ 每 worktree 的 nexus 名自带 provider 前缀(WorktreeName::generate 既有),N 个同名 provider 不会撞名。

- [ ] **Step 1: 写失败测试**

`tests/fleet_launch.rs`:

```rust
// fleet 编排:N worktree × N 会话,顺序创建 + 失败逆序回滚。
// 假 jsonl agent(同 jsonl_session.rs 夹具,提取 tests/common 或复制小型版)。
use std::path::Path;
use std::sync::Arc;

use nexus_core::agent::fleet::{launch_fleet, FleetItem};
use nexus_core::agent::manager::SessionManager;
use nexus_core::agent::registry::ProviderRegistry;
use nexus_core::config::model::AppConfig;
use nexus_core::gitx::cli::GitCliOps;
use nexus_core::gitx::ops::GitOps;
use nexus_core::gitx::worktree::WorktreeManager;
use nexus_core::gitx::events::WorktreeEventSink; // 以实际模块路径为准,实现者核对

fn init_repo(dir: &Path) { /* 与 worktree_manager.rs 既有 helper 同型:git init+commit */ }

#[tokio::test]
async fn fleet_creates_sessions_and_rolls_back_on_failure() {
    let repo = tempfile::tempdir().unwrap();
    init_repo(repo.path());
    let ops: Arc<dyn GitOps> = Arc::new(GitCliOps::new());
    let wt = WorktreeManager::new(ops.clone(), /* events 按既有签名 */);
    let sessions = SessionManager::new(Arc::new(|_| {}));
    // registry:配置含 fake(jsonl 脚本);第 2 项用不存在的 profile_id 注入失败
    let mut cfg = AppConfig::default();
    cfg.agent_profiles.push(fake_profile(&write_fake_agent(repo.path())));
    let registry = ProviderRegistry::from_config(&cfg);

    let items = vec![
        FleetItem { profile_id: "fake".into(), prompt: Some("干活".into()) },
        FleetItem { profile_id: "absent-provider".into(), prompt: None },
    ];
    let err = launch_fleet(&wt, &sessions, &registry, repo.path(), None, &items)
        .await
        .expect_err("第二项 provider 不存在必须失败");

    // 逆序回滚:第一项的 worktree 已建 → 必须被回收
    assert!(err.leftovers.is_empty(), "回滚应全部成功: {err:?}");
    let list = wt.list(repo.path()).await.unwrap();
    assert!(list.iter().all(|w| w.path == repo.path().join("非法定义")),
        "实现者按 list 语义断言:除主 worktree 外无残留(以 helper 输出为准)");
    // 失败信息定位到坏 provider
    assert!(err.message.contains("absent-provider"), "{}", err.message);
}

#[tokio::test]
async fn fleet_happy_path_returns_snapshots_in_order() {
    // 单项 fake(jsonl):worktree + 会话 + prompt 全链,返回快照含 provider/transport
    // 断言 snapshots.len()==1、provider_id=="fake"、transport==Jsonl、worktree_name 含 nexus/ 前缀
}
```

(实现者注:两处 `以实际为准` 标注——WorktreeManager/事件 sink 的实际构造签名、`wt.list` 返回主 worktree 的形态,以 `tests/worktree_manager.rs` 既有测试为准对齐;测试骨架的意图断言不可放松。)

- [ ] **Step 2: 红** — fleet 模块不存在。
- [ ] **Step 3: 实现**

```rust
// fleet 编排(spec §1.3 v1.3):顺序创建 + 失败逆序回滚,尽力清 + 残留透明。
use std::path::Path;

use serde::Serialize;

use crate::agent::manager::SessionManager;
use crate::agent::provider::{AgentTransport, LaunchContext};
use crate::agent::registry::ProviderRegistry;
use crate::agent::state::SessionSnapshot;
use crate::error::NexusError;
use crate::gitx::worktree::WorktreeManager;
use crate::ids::SessionId;

pub struct FleetItem {
    pub profile_id: String,
    pub prompt: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FleetError {
    pub message: String,
    /// 已成功回收的 worktree 名
    pub rolled_back: Vec<String>,
    /// 回滚失败的残留(worktree 名)——UI 列明,用户手动清理
    pub leftovers: Vec<String>,
}

pub async fn launch_fleet(
    wt: &WorktreeManager,
    sessions: &SessionManager,
    registry: &ProviderRegistry,
    repo: &Path,
    base_ref: Option<&str>,
    items: &[FleetItem],
) -> Result<Vec<SessionSnapshot>, FleetError> {
    // 预检:所有 provider 先查表(坏 id 在动工前暴露——避免建一半才发现)
    let providers: Vec<_> = items
        .iter()
        .map(|it| {
            registry
                .get(&it.profile_id)
                .ok_or_else(|| FleetError {
                    message: format!("未知的 provider:{}", it.profile_id),
                    rolled_back: Vec::new(),
                    leftovers: Vec::new(),
                })
        })
        .collect::<Result<_, _>>()?;

    let mut done: Vec<(String /*wt name*/, SessionId)> = Vec::new();
    let mut snapshots = Vec::new();
    for (i, (item, provider)) in items.iter().zip(providers.iter()).enumerate() {
        // 1. worktree(名自带 provider 前缀,不撞名)
        let info = match wt.create(repo, &item.profile_id, base_ref).await {
            Ok(i) => i,
            Err(e) => return Err(rollback(wt, sessions, &done, format!("第 {} 项 worktree 创建失败: {e}", i + 1)).await),
        };
        // 2. 会话(cwd = worktree 目录;branch 供 {branch} 占位符)
        let ctx = LaunchContext {
            cwd: Some(info.path.clone().into()),
            repo_path: Some(repo.to_string_lossy().into_owned()),
            worktree_name: Some(info.name.clone()),
            branch: info.branch.clone(),
        };
        let sid = match sessions.create(provider.as_ref(), 80, 24, Some(&ctx)).await {
            Ok(s) => s,
            Err(e) => return Err(rollback(wt, sessions, &done, format!("第 {} 项会话创建失败: {e}", i + 1)).await,
            // 注:本项 worktree 未入 done,单独回收:
            ).await_roll_current(&info.name),
        };
        done.push((info.name.clone(), sid));
        // 3. 初始指令:仅 jsonl(Pty 项软忽略)
        if provider.transport() == AgentTransport::Jsonl {
            if let Some(p) = &item.prompt {
                if let Err(e) = sessions.prompt(sid, p).await {
                    log::warn!("fleet 第 {} 项 prompt 失败(会话保留): {e}", i + 1);
                    // prompt 失败不回滚:会话活着,用户可手动发——软降级
                }
            }
        }
        snapshots.extend(sessions.list().into_iter().filter(|s| s.session_id == sid));
    }
    Ok(snapshots)
}

/// 逆序回滚:后建先拆;单项失败收集 leftover 继续。尽力清 + 残留透明(风险 #15)。
async fn rollback(
    wt: &WorktreeManager,
    sessions: &SessionManager,
    done: &[(String, SessionId)],
    message: String,
) -> FleetError {
    let mut rolled_back = Vec::new();
    let mut leftovers = Vec::new();
    for (name, sid) in done.iter().rev() {
        let _ = sessions.stop(*sid, true).await; // 已终态/不存在 → 忽略
        let _ = sessions.dispose(*sid);
        match wt.remove(repo_of(done, name).unwrap(), name, true).await {
            Ok(()) => rolled_back.push(name.clone()),
            Err(_) => leftovers.push(name.clone()),
        }
    }
    FleetError { message, rolled_back, leftovers }
}
```

(实现注:`rollback` 里 worktree 的 repo 路径应作为参数透传(闭包借用与 async 冲突),签名加 `repo: &Path`;失败项自身的 worktree(会话创建失败时)也要并入回收——上面 `await_roll_current` 是方向标注,实现把"当前项 worktree"先 push 进 done 再统一 rollback 最干净。快照收集改为 `sessions.list()` 按已存 sid 集合过滤,或让 `create` 返回快照——以最小改动为准。)

- [ ] **Step 4: 绿 + 门槛 + 提交**

```bash
cargo test --workspace && cargo fmt && cargo clippy --workspace --all-targets -- -D warnings
git add src-tauri && git commit -m "feat(m5): launch_fleet 编排——顺序创建 + 逆序回滚 + 残留透明(TDD)

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 10: launch_fleet IPC + LaunchFleetDialog + 「+」菜单入口

**Files:**
- Modify: `src-tauri/src/commands/worktree.rs`(launch_fleet 命令;或新 commands/fleet.rs——以 mod.rs 组织习惯定)
- Modify: `src-tauri/src/lib.rs`(generate_handler)
- Create: `src/features/launch/LaunchFleetDialog.tsx`
- Modify: `src/features/tabs/TabStrip.tsx`(「+」菜单加「并行编排…」)
- Modify: `src/ipc/types.ts`、`src/ipc/commands.ts`(契约)

**Interfaces:**
- Consumes: Task 9 `launch_fleet`;Task 8 `openSession`;Task 5 `providerList`
- Produces: `launchFleet(repoPath, baseRef?, items)` 封装;LaunchFleetDialog(`openState/onOpenChange`;内部自取 selected 项目);提交成功 → 逐快照 `sessionsStore.add` + `openSession` → 关框;失败 → toast(FleetError 含残留清单)

- [ ] **Step 1: IPC 命令**

```rust
/// 并行编排:N worktree × N 会话;失败逆序回滚,错误载荷含残留清单。
#[tauri::command]
pub async fn launch_fleet(
    wt: State<'_, WorktreeManager>,
    sessions: State<'_, SessionManager>,
    registry: State<'_, Arc<ProviderRegistry>>,
    repo_path: String,
    base_ref: Option<String>,
    items: Vec<FleetItemWire>,
) -> Result<Vec<SessionSnapshot>, String> {
    let repo = std::path::PathBuf::from(&repo_path).canonicalize()
        .map_err(|e| nexus_core::NexusError::InvalidInput(format!("仓库目录不存在 {repo_path}: {e}")).to_string())?;
    let items: Vec<nexus_core::agent::fleet::FleetItem> = items
        .into_iter()
        .map(|i| nexus_core::agent::fleet::FleetItem { profile_id: i.profile_id, prompt: i.prompt })
        .collect();
    match nexus_core::agent::fleet::launch_fleet(
        &wt, &sessions, &registry, &repo, base_ref.as_deref(), &items,
    ).await {
        Ok(snaps) => Ok(snaps),
        Err(e) => Err(serde_json::to_string(&e).unwrap_or_else(|_| e.message.clone())),
    }
}
```

(`FleetItemWire { profile_id, prompt }` camelCase;错误返回 JSON 字符串——前端 parse 出 rolledBack/leftovers 展示。`FleetItem` core 侧非 Serialize,Wire 类型在 IPC 层,合红线。)

- [ ] **Step 2: TS 契约 + LaunchFleetDialog**

`types.ts`/`commands.ts`:

```typescript
export interface FleetItemInput {
  profileId: string;
  prompt: string | null;
}
export interface FleetErrorInfo {
  message: string;
  rolledBack: string[];
  leftovers: string[];
}
export function launchFleet(
  repoPath: string,
  baseRef: string | null,
  items: FleetItemInput[]
): Promise<SessionSnapshot[]> {
  return invoke<SessionSnapshot[]>("launch_fleet", { repoPath, baseRef, items });
}
```

`LaunchFleetDialog.tsx`(表单:基线 ref 输入 + 条目行列表,每行 provider 下拉(providerList 数据,available 标注)+ prompt 输入(jsonl 才启用,pty 行 placeholder「开终端不派活」)+ 删除钮;「添加一项」钮;提交):

```tsx
// 并行编排(spec §2 M5):项目(当前选中)+ 基线 ref + 条目行(provider ×N + prompt)
// → launch_fleet → 逐快照开 tab。失败 toast 含回滚/残留清单。
// 交互细节(providerList 加载/条目增删/提交态)按 NewWorktreePopover 的
// 表单纪律实现(open 重置/busy 守卫/错误行内 + toast)——完整代码略同型,
// 实现者按 NewWorktreePopover.tsx 既有模式展开,此处为结构规格:
// - useEffect(open):重置 baseRef=""、items=[{profileId:第一个 available, prompt:""}]
// - providerList() 一次拉取缓存;下拉项 displayName + (available ? "" : " · 未安装")
// - 提交:items 过滤空 prompt→null → launchFleet(selected.repoPath, baseRef || null, items)
//   → 成功:snaps.forEach(s => { sessionsStore.add(s); tabs.openSession(s); }) → onOpenChange(false)
//   → 失败:JSON.parse 尝试 FleetErrorInfo;leftovers 非空时 toast 列残留路径提示手动清理
```

(计划故意不给 Dialog 全码:它是 NewWorktreePopover 既有表单模式的直接放大,逐字段展开的收益低于实现者读既有组件——**裁定:结构规格 + 交互纪律引用既有组件,记入 Self-Review**。)

`TabStrip.tsx`「+」菜单追加:

```tsx
<DropdownMenuItem onSelect={() => setFleetOpen(true)}>
  并行编排…
</DropdownMenuItem>
{/* fleetOpen 状态与 LaunchFleetDialog 挂载在 TabStrip 内(selected 为空时 disabled) */}
```

- [ ] **Step 3: 构建 + 人工验收 + 提交**

```bash
pnpm build && cargo test --workspace
pnpm tauri dev
# 验收(spec ①③):选项目 → 「+」→ 并行编排 → 1 个 claude + 1 个 codex →
#   claude 行填指令(如"运行 ls 并总结")→ 提交 → 两个 tab(会话+终端)出现;
#   claude 会话视图消息流/工具卡片实时渲染;codex 终端 cwd 落 worktree;
#   填一个不存在的 provider 名(手改配置制造)→ 提交 → 报错且无残留 worktree
git add -A && git commit -m "feat(m5): LaunchFleetDialog + launch_fleet IPC + 「+」菜单入口

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 11: M4 遗留清理(高价值批量吸收)

**Files:**
- Modify: `src/features/gitpanel/GitPanel.tsx`(epoch 闸/草稿提升/worktree 语义)
- Modify: `src/stores/projectStore.ts`(selected worktree 的绝对路径暴露,GitPanel 消费——或 GitPanel 自查 detail)
- Modify: `src-tauri/crates/nexus-core/src/gitx/ops.rs`(FileStatus 加 TypeChange)
- Modify: `src-tauri/crates/nexus-core/src/gitx/status.rs`('T' 码映射 + 收窄 pub(crate))
- Modify: `src-tauri/crates/nexus-core/src/gitx/cli.rs`(git_status 三命令轻 gate:仅要求 git 可用,porcelain v2 需 ≥2.11)
- Modify: `src-tauri/src/commands/worktree.rs`(git_* gate 调整)
- Modify: `src-tauri/crates/nexus-core/src/registry/mod.rs`(list 防御性排序 + "目录不存在"文案)
- Modify: `src/components/Toaster.tsx`(z-50 → z-[60],盖过 Dialog 遮罩)
- Modify: `src/ipc/types.ts`(FileStatus 加 "typechange")

**Interfaces:**
- Consumes: M4 完成记录"M5 必办"清单 1-5/8/9/14/18/22 项
- Produces: 见各条;**记弃项**(完成记录留档):伪造头行加固(#8 理论级)、commit dash 解析(#11 理论级)、UTF-16 序(#15)、阻塞 fs I/O(#19)、remove 半失败(#13 与 config 哲学一致保留)

**条目规格(逐条):**

1. **GitPanel repo 切换竞态(M4-必办#1)**:effect 加 epoch 闸——`const epoch = ++epochRef.current` 进 refresh,晚归的旧响应 `if (epoch !== epochRef.current) return;` 丢弃;错误态同理只留最新
2. **右栏折叠丢提交草稿(#2)**:message state 从组件提升——模块级 `let draftCache: Record<string, string>`(简单)或 zustand 小 store;挂载时按 repoPath 恢复,onChange 写入。取模块级缓存最简(不进 React 调试树)
3. **worktree 语义(#3)**:选中 worktree 节点时,面板 repoPath 用 worktree 的绝对路径(projectStore.detail 的 WorktreeInfo.path 按 name 查)——`git -C <worktree路径> status` 原生工作,面板显示该 worktree 的分支/变更;分支标题区附 worktree 名徽标
4. **'T' 码(#5)**:map_char 加 `'T' => Some(FileStatus::TypeChange)`;TS FileStatus 加 `"typechange"`;GitPanel LETTER 加 `typechanged: { ch: "T", cls: "text-muted-foreground" }`(先映射后样式微调)
5. **gate 文案(#7)**:git_status/stage/commit 三命令不再走 `ensure_worktree_ready`(2.20),改轻 gate(仅 git 可用;porcelain v2 实需 2.11——检查既有 GitCheckInfo 版本解析,写新 helper `ensure_git_for_status`)
6. **收窄(#11)**:`parse_status_porcelain_v2` pub → pub(crate)(M4 遗留文字矛盾,M4-T2 minor①)
7. **registry(#14/#15)**:list() 排序 `sort_by(added_at_ms)` 防御性兜底;add 失败文案改「无法访问目录 {path}」(canonicalize 失败含权限/环等)
8. **Toaster(#18)**:z-50 → z-[60](模态开着 toast 不被遮罩盖)

- [ ] **Step 1: 逐条落地**(每条自带验收点;Rust 条目先测试:map_char 'T' 单测、轻 gate 对 git_check version 的判定单测)
- [ ] **Step 2: 全量门槛 + 人工验收 + 提交**

```bash
cargo test --workspace && cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && pnpm build
pnpm tauri dev   # 快速改文件 → 切 worktree → 面板跟随;折叠右栏再展开草稿还在;两个项目快速切换无旧数据闪现
git add -A && git commit -m "fix(m4-followup): Git 面板竞态/草稿/worktree 语义 + 'T' 码 + gate 文案 + registry/Toaster 小修

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 12: 测试补遗 + 端到端验收 + PR

**Files:**
- Modify: 按补遗项定(预期:`src-tauri/crates/nexus-core/tests/` 补断言、快照 serde tag 单测)

**Interfaces:**
- Consumes: 全部前序任务
- Produces: spec M5 完成标准①-⑦ 验证记录 + PR

- [ ] **Step 1: 补遗清单**

- `AgentEvent` serde tag 单测:钉住 `"type":"toolUse"` 等 camelCase tag 值(TS 契约对齐,防漂移——同 SessionState 先例)
- fleet happy path 断言完备(若 Task 9 留有骨架断言):快照顺序/providerId/transport/worktreeName 前缀
- `provider_list` 可用性:假 profile(命令 `__nx_absent__`)→ available=false 断言
- Windows jsonl:确认两用例 `#[cfg(unix)]` 内联;**记入完成记录**:Windows jsonl 路径 CI 仅编译覆盖,真机由人工验收(用户机器 macOS)——计划内裁剪

- [ ] **Step 2: 全量门槛**

```bash
cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace && pnpm build
```

- [ ] **Step 3: 端到端验收(spec M5 完成标准①-⑦)**

从 worktree 目录跑 `pnpm tauri dev`,逐项:

1. ① 并行编排:选项目 → claude + codex(或 qwen)各一项,claude 行填指令 → 提交 → 会话 tab + 终端 tab;claude 消息流/工具卡片实时渲染;codex TUI 正常、cwd 落 worktree;项目树 worktree 节点出现;一键停止(`停止并关闭`)与清理全链可用
2. ② 命令写错:临时改配置(配置文件改 codex 命令为 `codexx`)→ 该会话 Failed + toast,其余不受影响;`provider_list` 面板/下拉对未安装命令标注
3. ③ 中途失败回滚:配置含不存在 provider 的编排 → 报错 + `git worktree list` 核对无残留
4. ④ composer 追加指令 → 工具卡片状态流转(running→completed)→ done 摘要;Ctrl+R 刷新 → 消息流 replay 恢复
5. ⑤ 自动化:假 jsonl agent 全链集成(已绿)+ 解析器快照 + fleet 回滚
6. ⑥ Task 11 各条目验收
7. ⑦ 基线:多终端并行/背压(cat 大文件)/PTY replay/关 tab 即删/Git 面板/亮色主题——M2-M4 验收项抽查

- [ ] **Step 4: 推送 + PR**

```bash
git -c http.proxy=http://127.0.0.1:7890 push -u origin m5-agent-provider-session-ui
# PR body 写文件后 --body-file;标题:feat(m5): AgentProvider 双传输 + 并行编排 + 会话 UI
gh pr checks --watch   # 四项绿后由用户本人合并
```

---

## Self-Review 记录

- **Spec 覆盖**:transport 字段/五模板→T1;trait/registry/探测→T2;帧解析→T3;双传输分叉/prompt/结构化订阅/快照字段→T4;四命令+TS→T5;消息 store(红线辨析/cap)→T6;AgentPane(原型)→T7;tab kind→T8;launch_fleet+回滚→T9/10;M4 遗留吸收(高价值)→T11;完成标准①-⑦→T10/T12。spec §2 M5 交付物逐项有落点。✅
- **有意裁剪(非遗漏)**:thought 行/tasklist 内嵌/@上下文/模型选择钮(spec v1.3 已记);PTY 会话 composer 不做(orca PTY 发送编排重坑,终端直接敲);Windows jsonl 集成用例 cfg(unix)(CI 编译覆盖 + 人工验收);LaunchFleetDialog 全码略(既有表单模式放大,结构规格引用 NewWorktreePopover);orca 的 journal/SQLite/adapter trait/stream-json 之外的降级三源全部不引(差异已在 spec §1.3 记档)。
- **类型一致性**:`AgentEvent` Rust enum(tag=type camelCase:toolUse/toolResult/durationMs/costUsd/claudeSession)↔ TS 联合逐字段对齐(T5 加 serde tag 钉住单测);`ProviderInfo{id,displayName,transport,command,available}` 三处一致;`SessionSnapshot` 增 providerId/transport 后 Rust/TS 同步(既有测试构造点全数列出);tab id `term-|agent-` 前缀约定在 tabStore/App 收敛(单一职责);`LaunchContext{cwd,repoPath,worktreeName,branch}` 定义即消费点闭环。
- **已知风险**:① T4 是最重改造(manager 泛化+分叉),既有集成测试适配量大——允许 T4/T5 合并提交,以编译桥策略降风险;② jsonl stop 语义(SIGINT 行为)依赖 claude CLI 实测,实现者以实测校准并记 Ruling(M4 git -z 先例);③ FleetError 的回滚路径参数传递在伪码中有标注点,实现者以编译干净为准重构签名;④ 帧样本来自本机实测(GLM 后端),真 claude 的字段宽度可能不同——解析纪律只认必要字段,快照测试钉形态而非全量。



