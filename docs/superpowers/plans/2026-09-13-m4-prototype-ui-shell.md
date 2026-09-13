# ItsukaNexus M4(原型 UI 骨架)Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 按 Open Design 原型(astra-ade-prototype-v2)把前端重构为四栏 workbench(rail 图标导航 + 项目树 + 多类型标签中心 + Git 右面板),后端新增项目注册表与 git 状态/提交能力,主题迁移为亮色唯一,终端功能在迁移中基线不回退。

**Architecture:** 落地策略 A(一步到位重构,不留新旧布局并存的过渡态)。后端先行:registry 域(projects.json 单条容错持久化)与 gitx status/stage/commit(porcelain v2 `-z` 机器格式解析)都在 nexus-core 里 TDD;IPC 层薄接线。前端按「骨架→树→新建流→Git 面板→打磨」推进,每步 app 保持可运行;tab 框架为单一 tabStore(session 是数据、tab 是视图),终端 pane 挂载闩锁红线全程不变。orca 源码(`/Users/itsuka/CodeSpace/orca`)是模式参考——借它的命令构造/解析纪律/布局派生钩子思想,**不引任何它的依赖**(它是 Electron,我们是 Tauri 2 + Rust)。

**Tech Stack:** Tauri 2、tokio::process、serde、zustand、Tailwind v4(CSS-first)+ shadcn/ui、@fontsource 本地字体、xterm.js。

**Spec:** `docs/superpowers/specs/2026-09-09-itsukanexus-mvp-design.md`(v1.2;§1.2 布局、§1.3 registry/gitx 职责、§1.4 命令表、§1.5 前端结构、§2 M4 章节、§5 风险 #11-#13)
**原型(视觉/布局权威):** `~/Library/Application Support/Open Design/namespaces/release-stable/data/projects/a3e91f89-9d76-4a78-ae04-cf4e4b3d0501/astra-ade-prototype-v2.html` + 同目录 `brand-spec.md`
**M3 完成记录:** `docs/superpowers/plans/2026-09-12-m3-worktree-workspace.md` 末尾(M4 必办来源与平台知识库;本计划已吸收必办 #1-#4,#6 轻量部分在 Task 10)

## Global Constraints

- 开发机:macOS(Apple Silicon),Rust 1.95、node 22、pnpm 12.3.4;跨平台回归靠 CI 三平台矩阵
- 分支:worktree 特性分支 `m4-prototype-ui-shell` 上实现,禁止直接提交 main;**PR 由用户本人合并**
- 提交规范:每任务一次提交,信息结尾 `Co-Authored-By: Claude Code <noreply@anthropic.com>`
- 包管理器只用 **pnpm**
- 质量门槛(每任务收尾必过):`cargo fmt` 已应用、`cargo clippy --workspace --all-targets -- -D warnings` 零警告、`cargo test --workspace` 全绿;涉及前端时 `pnpm build` 全绿(裸 `cargo test` 只覆盖根包——M3 实测陷阱)
- git push/pull/fetch 一律带代理:`git -c http.proxy=http://127.0.0.1:7890 push ...`(gh CLI 可直连)
- **nexus-core 不依赖 tauri** 的缝保持:core 内不得 `use tauri`;wire 类型只定义在 IPC 层
- nexus-core 不新增第三方依赖(现有 tokio/serde/uuid/thiserror/async-trait/tempfile(dev)足够;registry 复用 config store 的手写原子写模式)
- **主题:亮色唯一**(v1.2 决策)——令牌落 `:root`,不做 `.dark` 类与切换 UI;`@custom-variant dark` 收紧保留(既有);UI 文案全中文,产品名一律 ItsukaNexus(原型内 Astra 字样不出现)
- 前端红线不变:输出直达 `term.write` 不进 React;终端 pane 常驻只藏不卸(挂载闩锁);组件外实例注册表在 terminalManager
- M2/M3 参数定值不变:读 chunk 8KB、合帧 16ms/32KB、有界队列 64、订阅流 128、replay 256KB、SEND_INPUT_TIMEOUT 2s
- 事件与命令命名:命令 snake_case;既有事件 `session://state`、`session://exit`、`worktree://changed` 不变,M4 不新增事件
- 命令参数路径一律字符串(camelCase)过 IPC,core 内一律 `PathBuf` + canonicalize
- Windows 陷阱(M1-M3 实测,仍然有效):路径断言不 `contains` 字面子串(用 components/file_name);阻塞读/wait 有界;孙进程测试显式清理

---

### Task 1: registry 域——ProjectRegistry(projects.json 持久化,TDD)

**Files:**
- Modify: `src-tauri/crates/nexus-core/src/ids.rs`(ProjectId newtype)
- Modify: `src-tauri/crates/nexus-core/src/lib.rs`(`pub mod registry;`)
- Create: `src-tauri/crates/nexus-core/src/registry/mod.rs`(类型 + ProjectRegistry)
- Create: `src-tauri/crates/nexus-core/src/registry/store.rs`(磁盘读写)
- Create: `src-tauri/crates/nexus-core/tests/registry_store.rs`

**Interfaces:**
- Consumes: `GitOps::validate_repo`(校验入册路径)、`RepoInfo.root`(canonical 根)
- Produces(Task 3 IPC 依赖):
  - `ids::ProjectId(Uuid)`:`new()`、`FromStr`(Err=String)、`Display`、Serialize/Deserialize
  - `registry::ProjectEntry { id: ProjectId, name: String, path: PathBuf, added_at_ms: u64 }`(serde camelCase)
  - `registry::ProjectRegistry::new(dir: PathBuf, ops: Arc<dyn GitOps>)`;`async fn add(&self, path: &str) -> Result<ProjectEntry, NexusError>`(git 校验→以 RepoInfo.root 入册→写 `.git/info/exclude`→去重);`fn list(&self) -> Vec<ProjectEntry>`(按 added_at_ms 升序);`fn remove(&self, id: ProjectId) -> bool`
  - `registry::store::{load(dir) -> Vec<ProjectEntry>, save(dir, &[ProjectEntry]) -> Result<(), NexusError>}`(单条容错 + tmp+rename 原子写)

**学习点:** ① "单条容错"(orca zod-salvage 哲学的 serde 版):先解析成 `serde_json::Value`,再逐条 `from_value`——一条损坏只丢一条,只有整档非 JSON 才回退空表;② `.git/info/exclude` 是 repo 本地忽略文件(不进版本库、不影响协作者),追加 `.nx-worktrees/` 让主检出不因 worktree 目录变脏——这是 M3 遗留必办 #3 的落地形态(spec v1.2 决策);③ `#[serde(default)]` 让旧档缺字段也能读,`schema_version` 字段为未来迁移留缝。

- [ ] **Step 1: 写失败测试**

`tests/registry_store.rs`:

```rust
// registry 域:projects.json 持久化(单条容错/原子写)+ ProjectRegistry 入册流程。
use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use nexus_core::gitx::cli::GitCliOps;
use nexus_core::gitx::ops::GitOps;
use nexus_core::registry::store;
use nexus_core::registry::{ProjectEntry, ProjectRegistry};

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

fn entry(n: u32) -> ProjectEntry {
    ProjectEntry {
        id: format!("00000000-0000-0000-0000-00000000000{n}").parse().unwrap(),
        name: format!("p{n}"),
        path: format!("/tmp/p{n}").into(),
        added_at_ms: n as u64,
    }
}

#[test]
fn store_roundtrip_preserves_order() {
    let dir = tempfile::tempdir().unwrap();
    let items = vec![entry(1), entry(2)];
    store::save(dir.path(), &items).unwrap();
    assert_eq!(store::load(dir.path()), items);
}

#[test]
fn load_salvages_per_entry_and_falls_back_on_non_json() {
    let dir = tempfile::tempdir().unwrap();
    // 单条损坏(id 非法):只丢那一条
    std::fs::write(
        dir.path().join("projects.json"),
        r#"{"schemaVersion":1,"projects":[{"id":"not-a-uuid","name":"bad","path":"/b","addedAtMs":1},{"id":"00000000-0000-0000-0000-000000000002","name":"ok","path":"/ok","addedAtMs":2}]}"#,
    )
    .unwrap();
    let loaded = store::load(dir.path());
    assert_eq!(loaded.len(), 1, "坏条目丢弃,好条目保留");
    assert_eq!(loaded[0].name, "ok");
    // 整档非 JSON:回退空表
    std::fs::write(dir.path().join("projects.json"), "{{{").unwrap();
    assert!(store::load(dir.path()).is_empty());
    // 缺 schemaVersion/projects 字段:serde default 兜底为空表
    std::fs::write(dir.path().join("projects.json"), "{}").unwrap();
    assert!(store::load(dir.path()).is_empty());
}

#[tokio::test]
async fn add_validates_git_repo_and_writes_exclude() {
    let dir = tempfile::tempdir().unwrap();
    init_repo_at(dir.path());
    let reg = ProjectRegistry::new(
        tempfile::tempdir().unwrap().path().to_path_buf(),
        Arc::new(GitCliOps::new()),
    );
    // 非 git 目录拒绝(NotARepo 透传)
    let notrepo = tempfile::tempdir().unwrap();
    assert!(reg.add(notrepo.path().to_str().unwrap()).await.is_err());

    // git 仓库入册:path 归一到 canonical 根;exclude 被追加
    let added = reg.add(dir.path().to_str().unwrap()).await.unwrap();
    assert_eq!(added.path, dir.path().canonicalize().unwrap());
    assert_eq!(added.name, dir.path().file_name().unwrap().to_string_lossy());
    let exclude = std::fs::read_to_string(dir.path().join(".git/info/exclude")).unwrap();
    assert!(exclude.lines().any(|l| l.trim() == ".nx-worktrees/"), "exclude 应含 .nx-worktrees/: {exclude}");

    // 重复添加同一路径:返回既有条目,不重复入册、不重复追加 exclude
    let again = reg.add(dir.path().to_str().unwrap()).await.unwrap();
    assert_eq!(again.id, added.id);
    assert_eq!(reg.list().len(), 1);
    let cnt = exclude_lines(&dir);
    assert_eq!(cnt, 1, "exclude 只应有一行 .nx-worktrees/");

    // remove:命中 true,再删 false;条目消失
    assert!(reg.remove(added.id));
    assert!(!reg.remove(added.id));
    assert!(reg.list().is_empty());
}

fn exclude_lines(dir: &tempfile::TempDir) -> usize {
    std::fs::read_to_string(dir.path().join(".git/info/exclude"))
        .unwrap()
        .lines()
        .filter(|l| l.trim() == ".nx-worktrees/")
        .count()
}
```

- [ ] **Step 2: 红** — `cargo test --workspace --test registry_store` 编译错(模块不存在)。
- [ ] **Step 3: 实现**

`ids.rs` 追加(与 SessionId 同型):

```rust
/// 项目注册表条目 id(spec §1.3 registry 域)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProjectId(uuid::Uuid);

impl ProjectId {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4())
    }
}

impl Default for ProjectId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for ProjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for ProjectId {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        uuid::Uuid::parse_str(s)
            .map(ProjectId)
            .map_err(|e| format!("非法 project id {s:?}: {e}"))
    }
}
```

`registry/mod.rs`:

```rust
// 项目注册表(spec §1.3):多项目目录的持久化与入册校验。
// 存储在 store.rs;本文件是领域服务:git 校验 + exclude 追加 + 内存表。
pub mod store;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::error::NexusError;
use crate::gitx::ops::GitOps;
use crate::ids::ProjectId;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectEntry {
    pub id: ProjectId,
    pub name: String,
    pub path: PathBuf,
    pub added_at_ms: u64,
}

pub struct ProjectRegistry {
    dir: PathBuf,
    ops: Arc<dyn GitOps>,
    entries: Mutex<Vec<ProjectEntry>>,
}

impl ProjectRegistry {
    pub fn new(dir: PathBuf, ops: Arc<dyn GitOps>) -> Self {
        let entries = store::load(&dir);
        Self {
            dir,
            ops,
            entries: Mutex::new(entries),
        }
    }

    /// 入册:git 校验(仅 git 仓库,spec v1.2)→ 以 RepoInfo.root(canonical)
    /// 归一路径 → 追加 repo 本地 exclude(M3 必办#3)→ 持久化。
    /// 同一路径重复入册返回既有条目(幂等)。
    pub async fn add(&self, path: &str) -> Result<ProjectEntry, NexusError> {
        let info = self.ops.validate_repo(std::path::Path::new(path)).await?;
        let root = info.root;
        {
            let mut guard = self.entries.lock().expect("注册表锁被毒化");
            if let Some(hit) = guard.iter().find(|e| e.path == root) {
                return Ok(hit.clone());
            }
            let entry = ProjectEntry {
                id: ProjectId::new(),
                name: root
                    .file_name()
                    .map(|f| f.to_string_lossy().into_owned())
                    .unwrap_or_else(|| root.to_string_lossy().into_owned()),
                path: root.clone(),
                added_at_ms: now_ms(),
            };
            guard.push(entry.clone());
            guard.sort_by_key(|e| e.added_at_ms);
            if let Err(e) = store::save(&self.dir, &guard) {
                // 写盘失败回滚内存表:表与磁盘不分离
                guard.retain(|e| e.id != entry.id);
                return Err(e);
            }
            drop(guard);
            Self::append_local_exclude(&root);
            Ok(entry)
        }
    }

    pub fn list(&self) -> Vec<ProjectEntry> {
        self.entries.lock().expect("注册表锁被毒化").clone()
    }

    pub fn remove(&self, id: ProjectId) -> bool {
        let mut guard = self.entries.lock().expect("注册表锁被毒化");
        let before = guard.len();
        guard.retain(|e| e.id != id);
        if guard.len() == before {
            return false;
        }
        if let Err(e) = store::save(&self.dir, &guard) {
            log::warn!("projects.json 写盘失败: {e}");
        }
        true
    }

    /// repo 本地忽略(不进版本库):追加 `.nx-worktrees/`,已有则跳过。
    /// best-effort:失败只 warn,不阻塞入册。
    fn append_local_exclude(root: &std::path::Path) {
        let info = root.join(".git").join("info");
        let file = info.join("exclude");
        if let Ok(existing) = std::fs::read_to_string(&file) {
            if existing.lines().any(|l| l.trim() == ".nx-worktrees/") {
                return;
            }
        }
        if let Err(e) = std::fs::create_dir_all(&info) {
            log::warn!(".git/info 创建失败({e}),跳过 exclude 追加");
            return;
        }
        let mut next = std::fs::read_to_string(&file).unwrap_or_default();
        if !next.is_empty() && !next.ends_with('\n') {
            next.push('\n');
        }
        next.push_str(".nx-worktrees/\n");
        if let Err(e) = std::fs::write(&file, next) {
            log::warn!("exclude 追加失败({e}),主检出会显示 .nx-worktrees/ 为未跟踪");
        }
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
```

`registry/store.rs`(单条容错——先 Value 后逐条;原子写同 config store 模式):

```rust
// projects.json 磁盘读写:单条容错(坏一条丢一条)+ tmp+rename 原子写。
// 容错策略(orca zod-salvage 哲学):整档解析成 serde_json::Value,
// 逐条 from_value——单条字段坏只丢该条;整档非 JSON/缺字段 → 空表。
use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::ProjectEntry;
use crate::error::NexusError;

const FILE: &str = "projects.json";
const TMP: &str = "projects.json.tmp";

#[derive(Serialize, Deserialize)]
struct Schema {
    schema_version: u32,
    projects: Vec<ProjectEntry>,
}

pub fn load(dir: &Path) -> Vec<ProjectEntry> {
    let bytes = match std::fs::read(dir.join(FILE)) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(e) => {
            log::warn!("projects.json 读取失败({e}),按空表启动");
            return Vec::new();
        }
    };
    let root: serde_json::Value = match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(e) => {
            log::warn!("projects.json 非 JSON({e}),按空表启动(文件保留现场)");
            return Vec::new();
        }
    };
    let Some(raws) = root.get("projects").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    raws.iter()
        .filter_map(|r| match serde_json::from_value::<ProjectEntry>(r.clone()) {
            Ok(e) => Some(e),
            Err(e) => {
                log::warn!("一条项目记录损坏,丢弃: {e}");
                None
            }
        })
        .collect()
}

pub fn save(dir: &Path, entries: &[ProjectEntry]) -> Result<(), NexusError> {
    let schema = Schema {
        schema_version: 1,
        projects: entries.to_vec(),
    };
    std::fs::create_dir_all(dir)?;
    let json = serde_json::to_vec_pretty(&schema)
        .map_err(|e| NexusError::Pty(std::io::Error::other(format!("注册表序列化失败: {e}"))))?;
    let tmp = dir.join(TMP);
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(&json)?;
        f.flush()?;
    }
    std::fs::rename(&tmp, dir.join(FILE))?;
    Ok(())
}
```

`lib.rs` 加 `pub mod registry;`(模块声明区)。

- [ ] **Step 4: 绿 + 门槛**

```bash
cargo test --workspace && cargo fmt && cargo clippy --workspace --all-targets -- -D warnings
```

- [ ] **Step 5: 提交**

```bash
git add src-tauri && git commit -m "feat(m4): registry 域——项目注册表持久化(单条容错+exclude 追加)

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 2: gitx status/stage/commit——porcelain v2 解析(TDD)

**Files:**
- Modify: `src-tauri/crates/nexus-core/src/gitx/ops.rs`(GitStatus 类型 + trait 三方法)
- Create: `src-tauri/crates/nexus-core/src/gitx/status.rs`(解析器)
- Modify: `src-tauri/crates/nexus-core/src/gitx/cli.rs`(实现三方法 + run_env)
- Modify: `src-tauri/crates/nexus-core/src/gitx/mod.rs`(`pub mod status;`)
- Create: `src-tauri/crates/nexus-core/tests/git_status.rs`

**Interfaces:**
- Consumes: 既有 `GitCliOps::run`
- Produces(Task 3/8 依赖):
  - `ops::FileStatus` enum(serde camelCase):`Modified/Added/Deleted/Renamed/Copied/Untracked/Unmerged`
  - `ops::GitStatusEntry { path: PathBuf, index: Option<FileStatus>, worktree: Option<FileStatus>, orig_path: Option<PathBuf> }`
  - `ops::GitStatus { branch: Option<String>, ahead: u32, behind: u32, entries: Vec<GitStatusEntry>, truncated: bool }`
  - trait `GitOps` 追加:`async fn status(&self, repo: &Path) -> Result<GitStatus, NexusError>`、`async fn stage(&self, repo: &Path, paths: Option<&[PathBuf]>) -> Result<(), NexusError>`、`async fn commit(&self, repo: &Path, message: &str) -> Result<(), NexusError>`
  - `status::parse_status_porcelain_v2(out: &str, cap: usize) -> GitStatus`(pub(crate) 于 status.rs,测试同文件)

**学习点:** ① porcelain v2 `-z` 是 git 的机器接口:头记录(`# ...`)仍以 `\n` 结尾,变更记录以 NUL 分隔,rename 记录是 `path\0origPath\0` 两段——不用 `-z` 时路径会被 C 风格转义、rename 分隔有歧义,`-z` 一并消掉(orca 不带 `-z` 靠 `core.quotePath=false`,我们直接用更稳的 `-z`;spec §1.3 的 quotePath 方案由此细化,意图一致:非 ASCII 路径原样);② `GIT_OPTIONAL_LOCKS=0` 让 status 不拿 index 锁——UI 轮询/聚焦刷新不会跟用户终端里的 git 抢 `index.lock`;③ 条目上限 2000(orca 同款):巨量未跟踪目录不至于把输出和 UI 撑爆,`truncated` 透传给面板显示横幅。

- [ ] **Step 1: 写失败测试**

`tests/git_status.rs`:

```rust
// gitx status/stage/commit:porcelain v2 -z 集成 + 快照由 status.rs 内联单测钉住。
use std::path::Path;
use std::process::Command;

use nexus_core::gitx::cli::GitCliOps;
use nexus_core::gitx::ops::{FileStatus, GitOps};

fn run_git(dir: &Path, args: &[&str]) {
    let st = Command::new("git").arg("-C").arg(dir).args(args).status().unwrap();
    assert!(st.success(), "git {args:?} 失败");
}

fn init_repo(dir: &Path) {
    run_git(dir, &["init", "-q", "-b", "main"]);
    run_git(dir, &["config", "user.email", "test@nx.local"]);
    run_git(dir, &["config", "user.name", "nx-test"]);
    std::fs::write(dir.join("a.txt"), "a\n").unwrap();
    run_git(dir, &["add", "."]);
    run_git(dir, &["commit", "-qm", "init"]);
}

#[tokio::test]
async fn status_reports_branch_entries_and_staging_lifecycle() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path());
    let ops = GitCliOps::new();

    // 干净:branch=main,无条目
    let st = ops.status(dir.path()).await.unwrap();
    assert_eq!(st.branch.as_deref(), Some("main"));
    assert!(st.entries.is_empty(), "刚提交完应干净: {st:?}");
    assert!(!st.truncated);

    // 修改 + 未跟踪 + 重命名(中文文件名走 -z 原样路径)
    std::fs::write(dir.path().join("a.txt"), "a2\n").unwrap();
    std::fs::write(dir.path().join("新 文件.txt"), "n\n").unwrap();
    std::fs::write(dir.path().join("b.txt"), "b\n").unwrap();
    run_git(dir, &["add", "b.txt"]);
    run_git(dir, &["commit", "-qm", "b"]);
    run_git(dir, &["mv", "b.txt", "b-renamed.txt"]);

    let st = ops.status(dir.path()).await.unwrap();
    let find = |p: &str| st.entries.iter().find(|e| e.path == Path::new(p)).unwrap_or_else(|| panic!("缺 {p}: {:?}", st.entries));
    let a = find("a.txt");
    assert_eq!(a.index, None);
    assert_eq!(a.worktree, Some(FileStatus::Modified));
    let un = find("新 文件.txt");
    assert_eq!(un.worktree, Some(FileStatus::Untracked));
    let rn = find("b-renamed.txt");
    assert_eq!(rn.index, Some(FileStatus::Renamed));
    assert_eq!(rn.orig_path.as_deref(), Some(Path::new("b.txt")));

    // 暂存全部 → a 变为 index 侧 Modified;提交 → 干净
    ops.stage(dir.path(), None).await.unwrap();
    let st = ops.status(dir.path()).await.unwrap();
    assert_eq!(find_status(&st, "a.txt").index, Some(FileStatus::Modified));
    ops.commit(dir.path(), "更新 a 与新文件").await.unwrap();
    let st = ops.status(dir.path()).await.unwrap();
    assert!(st.entries.is_empty(), "提交后应干净: {st:?}");
}

fn find_status<'a>(st: &'a nexus_core::gitx::ops::GitStatus, p: &str) -> &'a nexus_core::gitx::ops::GitStatusEntry {
    st.entries.iter().find(|e| e.path == Path::new(p)).unwrap()
}

#[tokio::test]
async fn status_ahead_behind_from_branch_header() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path());
    let origin = tempfile::tempdir().unwrap();
    run_git(origin.path(), &["init", "-q", "--bare"]);
    run_git(dir.path(), &["remote", "add", "origin", origin.path().to_str().unwrap()]);
    run_git(dir.path(), &["push", "-q", "-u", "origin", "main"]);
    // 本地再提交一笔 → ahead 1 / behind 0
    std::fs::write(dir.path().join("c.txt"), "c\n").unwrap();
    run_git(dir.path(), &["add", "."]);
    run_git(dir.path(), &["commit", "-qm", "c"]);

    let ops = GitCliOps::new();
    let st = ops.status(dir.path()).await.unwrap();
    assert_eq!(st.ahead, 1);
    assert_eq!(st.behind, 0);
}

#[tokio::test]
async fn commit_rejects_empty_message() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path());
    let ops = GitCliOps::new();
    assert!(ops.commit(dir.path(), "  ").await.is_err(), "空白提交信息应拒绝");
}

#[tokio::test]
async fn stage_selected_paths_only() {
    let dir = tempfile::tempdir().unwrap();
    init_repo(dir.path());
    std::fs::write(dir.path().join("x.txt"), "x\n").unwrap();
    std::fs::write(dir.path().join("y.txt"), "y\n").unwrap();
    let ops = GitCliOps::new();
    ops.stage(dir.path(), Some(&[Path::new("x.txt").to_path_buf()]))
        .await
        .unwrap();
    let st = ops.status(dir.path()).await.unwrap();
    assert_eq!(find_status(&st, "x.txt").index, Some(FileStatus::Added));
    assert_eq!(find_status(&st, "y.txt").index, None);
}
```

- [ ] **Step 2: 红** — 编译错(trait 方法/类型不存在)。
- [ ] **Step 3: 实现**

`ops.rs` 追加类型与 trait 方法:

```rust
/// 单侧(index 或 worktree)文件状态(porcelain v2 XY 字符映射)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FileStatus {
    Modified,
    Added,
    Deleted,
    Renamed,
    Copied,
    Untracked,
    Unmerged,
}

/// status 条目:index/worktree 双侧;None = 该侧未变化('.')
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitStatusEntry {
    pub path: PathBuf,
    pub index: Option<FileStatus>,
    pub worktree: Option<FileStatus>,
    /// rename/copy 的原路径(仅 R/C 的 index 侧有)
    pub orig_path: Option<PathBuf>,
}

/// git_status 返回:分支/ahead/behind 从 --branch 头部折叠(免二次子进程)
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitStatus {
    pub branch: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub entries: Vec<GitStatusEntry>,
    /// 超过条目上限被截断(orca 同款防巨量未跟踪目录)
    pub truncated: bool,
}
```

trait `GitOps` 内追加(worktree 三方法之后):

```rust
    // ---- 状态/提交(M4,右侧 Git 面板)----

    /// 工作区状态:porcelain v2 -z + --branch(分支与 ahead/behind 一次拿全)。
    async fn status(&self, repo: &Path) -> Result<GitStatus, NexusError>;
    /// 暂存:None = 全部(`git add -A`);Some = 指定路径。
    async fn stage(&self, repo: &Path, paths: Option<&[PathBuf]>) -> Result<(), NexusError>;
    /// 提交暂存区;message trim 后为空返回 InvalidInput。
    async fn commit(&self, repo: &Path, message: &str) -> Result<(), NexusError>;
```

`status.rs`(解析器 + 内联快照单测):

```rust
// porcelain v2 -z 解析(spec §1.3):机器格式,头记录以 \n 结尾、变更记录以 NUL
// 分隔、rename 是 path\0origPath\0 两段。只认行首关键词,未知记录跳过。
use std::path::PathBuf;

use super::ops::{FileStatus, GitStatus, GitStatusEntry};

/// XY 单字符 → FileStatus;'.' → None;未知字符 → None(git 新增状态码不炸)
fn map_char(c: char) -> Option<FileStatus> {
    match c {
        'M' => Some(FileStatus::Modified),
        'A' => Some(FileStatus::Added),
        'D' => Some(FileStatus::Deleted),
        'R' => Some(FileStatus::Renamed),
        'C' => Some(FileStatus::Copied),
        'U' => Some(FileStatus::Unmerged),
        '.' => None,
        other => {
            log::debug!("porcelain v2 未知状态码 {other:?},按无变化处理");
            None
        }
    }
}

/// 解析 `git status --porcelain=v2 --branch -z` 输出。cap 为条目上限,
/// 超过即停并置 truncated=true。
pub fn parse_status_porcelain_v2(out: &str, cap: usize) -> GitStatus {
    let mut status = GitStatus::default();
    let mut expect_orig: Option<GitStatusEntry> = None; // rename 等待 origPath 段

    // -z 语义:首个 NUL 段 = 全部头行 + 第一条变更;其后每段一条变更
    // (rename 的 origPath 独占一段)
    let mut segments = out.split('\0');
    let first = segments.next().unwrap_or("");
    let mut first_record: Option<String> = None;
    for line in first.lines() {
        if let Some(rest) = line.strip_prefix("# ") {
            parse_header(rest, &mut status);
        } else if !line.is_empty() {
            first_record = Some(line.to_string());
        }
    }
    let mut pending: Vec<String> = first_record.into_iter().collect();
    pending.extend(segments.filter(|s| !s.is_empty()).map(str::to_string));

    for line in pending {
        // 上一条是 rename:本段是它的 origPath
        if let Some(mut entry) = expect_orig.take() {
            entry.orig_path = Some(PathBuf::from(&line));
            push_entry(&mut status, entry, cap);
            continue;
        }
        if let Some(path) = line.strip_prefix("? ") {
            push_entry(
                &mut status,
                GitStatusEntry {
                    path: PathBuf::from(path),
                    index: None,
                    worktree: Some(FileStatus::Untracked),
                    orig_path: None,
                },
                cap,
            );
        } else if let Some(rest) = line.strip_prefix("1 ") {
            // 1 <XY> <sub> <mH> <mI> <mW> <hH> <hI> <path>:path 可含空格,splitn 第 8 段起全归路径
            if let Some(entry) = parse_xy_path(rest, 8) {
                push_entry(&mut status, entry, cap);
            }
        } else if let Some(rest) = line.strip_prefix("2 ") {
            // 2 ... <path>\0<origPath>:origPath 在下一段
            if let Some(mut entry) = parse_xy_path(rest, 9) {
                entry.orig_path = Some(PathBuf::new()); // 占位,下一段覆盖
                expect_orig = Some(entry);
            }
        } else if line.starts_with("u ") {
            // 冲突:双侧 Unmerged;path 在第 11 段
            if let Some(path) = line.splitn(11, ' ').nth(10) {
                push_entry(
                    &mut status,
                    GitStatusEntry {
                        path: PathBuf::from(path),
                        index: Some(FileStatus::Unmerged),
                        worktree: Some(FileStatus::Unmerged),
                        orig_path: None,
                    },
                    cap,
                );
            }
        }
        // '#' 以外的未知行:跳过(git 新版本加字段不炸)
    }
    // 截断场景:expect_orig 悬空无害(最后一条 rename 已截断)
    status
}

/// 头行:`branch.head main` / `branch.ab +2 -1` / 其余忽略
fn parse_header(rest: &str, status: &mut GitStatus) {
    if let Some(h) = rest.strip_prefix("branch.head ") {
        let h = h.trim();
        status.branch = if h == "(detached)" { None } else { Some(h.to_string()) };
    } else if let Some(ab) = rest.strip_prefix("branch.ab ") {
        let mut it = ab.split_whitespace();
        if let Some(a) = it.next() {
            status.ahead = a.trim_start_matches('+').parse().unwrap_or(0);
        }
        if let Some(b) = it.next() {
            status.behind = b.trim_start_matches('-').parse().unwrap_or(0);
        }
    }
}

/// `1`/`2` 记录公共段:前两字符 XY,路径在 splitn(fields) 段的最后
fn parse_xy_path(rest: &str, fields: usize) -> Option<GitStatusEntry> {
    let mut parts = rest.splitn(fields, ' ');
    let xy = parts.next()?;
    let mut chars = xy.chars();
    let x = chars.next()?;
    let y = chars.next()?;
    let path = parts.last()?.trim_end();
    Some(GitStatusEntry {
        path: PathBuf::from(path),
        index: map_char(x),
        worktree: map_char(y),
        orig_path: None,
    })
}

fn push_entry(status: &mut GitStatus, entry: GitStatusEntry, cap: usize) {
    if status.entries.len() >= cap {
        status.truncated = true;
        return;
    }
    status.entries.push(entry);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gitx::ops::FileStatus;

    /// -z 快照:头行们 + 修改 + 未跟踪 + rename(注意头行后紧跟第一条记录,NUL 分隔)
    #[test]
    fn parses_z_stream_with_headers_and_rename() {
        let out = "# branch.oid 6cf78bbf16892e48ecef12a92d66d98c7c938453\n\
                   # branch.head main\n\
                   # branch.upstream origin/main\n\
                   # branch.ab +2 -1\n\
                   1 .M N... 100644 100644 100644 6cf78 6cf78 a.txt\0\
                   ? 新 未跟踪.txt\0\
                   2 R  N... 100644 100644 100644 6cf78 11111 b-renamed.txt\0b.txt\0";
        let st = parse_status_porcelain_v2(out, 100);
        assert_eq!(st.branch.as_deref(), Some("main"));
        assert_eq!((st.ahead, st.behind), (2, 1));
        assert_eq!(st.entries.len(), 3);
        assert_eq!(st.entries[0].worktree, Some(FileStatus::Modified));
        assert_eq!(st.entries[0].index, None);
        assert_eq!(st.entries[1].worktree, Some(FileStatus::Untracked));
        assert_eq!(st.entries[2].index, Some(FileStatus::Renamed));
        assert_eq!(st.entries[2].orig_path.as_deref(), Some(std::path::Path::new("b.txt")));
        assert!(!st.truncated);
    }

    #[test]
    fn cap_truncates_and_flags() {
        let out = "1 .M N... 100644 100644 100644 6cf78 6cf78 a\01 .M N... 100644 100644 100644 6cf78 6cf78 b\0";
        let st = parse_status_porcelain_v2(out, 1);
        assert_eq!(st.entries.len(), 1);
        assert!(st.truncated);
    }

    #[test]
    fn detached_branch_is_none_and_unknown_records_skipped() {
        let out = "# branch.head (detached)\n!ignored-record\0";
        let st = parse_status_porcelain_v2(out, 100);
        assert!(st.branch.is_none());
        assert!(st.entries.is_empty());
    }
}
```

`cli.rs`:`GitCliOps` 增加带环境变量的执行辅助与三方法实现(`impl GitOps for GitCliOps` 块内追加):

```rust
    /// 同 run,但注入环境变量(如 GIT_OPTIONAL_LOCKS=0:只读探测不抢 index.lock)
    async fn run_env(
        &self,
        repo: Option<&Path>,
        args: &[&str],
        envs: &[(&str, &str)],
    ) -> Result<String, NexusError> {
        let mut cmd = tokio::process::Command::new(&self.git_bin);
        if let Some(r) = repo {
            cmd.arg("-C").arg(r);
        }
        cmd.args(args);
        for (k, v) in envs {
            cmd.env(k, v);
        }
        let cmd_repr = format!("{args:?}");
        let out = cmd
            .output()
            .await
            .map_err(|e| NexusError::GitUnavailable(format!("无法执行 git({e})")))?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
            return Err(NexusError::GitCommand { cmd: cmd_repr, stderr });
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    /// status 条目上限(spec §1.3):超出截断并置 truncated
    const STATUS_ENTRY_CAP: usize = 2000;
```

```rust
    async fn status(&self, repo: &Path) -> Result<GitStatus, NexusError> {
        // -z:路径原样(非 ASCII 不转义)、rename 无歧义;头记录仍 \n 结尾
        let out = self
            .run_env(
                Some(repo),
                &[
                    "status", "--porcelain=v2", "--branch",
                    "--untracked-files=all", "-z",
                ],
                &[("GIT_OPTIONAL_LOCKS", "0")],
            )
            .await?;
        Ok(super::status::parse_status_porcelain_v2(&out, Self::STATUS_ENTRY_CAP))
    }

    async fn stage(&self, repo: &Path, paths: Option<&[PathBuf]>) -> Result<(), NexusError> {
        match paths {
            None => self.run(Some(repo), &["add", "-A"]).await.map(|_| ()),
            Some(list) if list.is_empty() => Ok(()),
            Some(list) => {
                let mut args: Vec<String> = vec!["add".into(), "--".into()];
                args.extend(list.iter().map(|p| p.to_string_lossy().into_owned()));
                let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
                self.run(Some(repo), &arg_refs).await.map(|_| ())
            }
        }
    }

    async fn commit(&self, repo: &Path, message: &str) -> Result<(), NexusError> {
        let msg = message.trim();
        if msg.is_empty() {
            return Err(NexusError::InvalidInput("提交信息不能为空".into()));
        }
        self.run(Some(repo), &["commit", "-m", msg]).await.map(|_| ())
    }
```

(`run` 的返回 `.into_owned()` 让 String 生命周期干净;既有 `run` 不动。`mod.rs` 加 `pub mod status;`。)

- [ ] **Step 4: 绿 + 门槛**

```bash
cargo test --workspace && cargo fmt && cargo clippy --workspace --all-targets -- -D warnings
```

- [ ] **Step 5: 提交**

```bash
git add src-tauri && git commit -m "feat(m4): gitx status/stage/commit——porcelain v2 -z 解析(TDD)

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 3: IPC 接线——project_* / git_* 命令 + FromStr 加固(必办#1)+ TS 契约

**Files:**
- Create: `src-tauri/src/commands/project.rs`
- Modify: `src-tauri/src/commands/mod.rs`(注册 project)
- Modify: `src-tauri/src/commands/worktree.rs`(git_status/stage/commit 三命令)
- Modify: `src-tauri/src/commands/session.rs`(worktree_name FromStr 校验)
- Modify: `src-tauri/crates/nexus-core/src/gitx/worktree.rs`(manager 转发 status/stage/commit)
- Modify: `src-tauri/src/lib.rs`(共享 ops Arc + manage ProjectRegistry + 注册命令)
- Modify: `src/ipc/types.ts`、`src/ipc/commands.ts`(TS 契约与封装)

**Interfaces:**
- Consumes: Task 1 `ProjectRegistry`、Task 2 trait 三方法、既有 `WorktreeManager.check`
- Produces(前端契约,Task 6/7/8 消费):
  - 命令:`project_list() -> Vec<ProjectEntry>`;`project_add { path } -> ProjectEntry`;`project_remove { projectId } -> ()`;`git_status { repoPath } -> GitStatus`;`git_stage { repoPath, paths? } -> ()`;`git_commit { repoPath, message } -> ()`
  - `session_create` 的 `worktree_name` 经 `WorktreeName::from_str` 校验(非法名直接 InvalidInput,`../` 逃逸加固)
  - TS:`ProjectEntry`、`FileStatus`、`GitStatusEntry`、`GitStatus` 类型与 `projectList/projectAdd/projectRemove/gitStatus/gitStage/gitCommit` 封装

- [ ] **Step 1: WorktreeManager 转发方法**

`gitx/worktree.rs` 的 `impl WorktreeManager` 追加(与 check/validate_repo 同型):

```rust
    /// 工作区状态(porcelain v2,Task 8 Git 面板数据源)。
    pub async fn status(&self, repo: &Path) -> Result<super::ops::GitStatus, NexusError> {
        self.ops.status(repo).await
    }

    /// 暂存:None = 全部。
    pub async fn stage(
        &self,
        repo: &Path,
        paths: Option<&[PathBuf]>,
    ) -> Result<(), NexusError> {
        self.ops.stage(repo, paths).await
    }

    /// 提交暂存区;空信息 InvalidInput。
    pub async fn commit(&self, repo: &Path, message: &str) -> Result<(), NexusError> {
        self.ops.commit(repo, message).await
    }
```

- [ ] **Step 2: project 命令层**

`commands/project.rs`:

```rust
// project_*:项目注册表命令(薄封装,领域逻辑在 nexus-core registry 域)。
// 入册校验(仅 git 仓库)在 ProjectRegistry::add 内完成。
use tauri::State;

use nexus_core::ids::ProjectId;
use nexus_core::registry::{ProjectEntry, ProjectRegistry};

#[tauri::command]
pub async fn project_list(
    state: State<'_, ProjectRegistry>,
) -> Result<Vec<ProjectEntry>, String> {
    let list = state.list();
    Ok(list)
}

#[tauri::command]
pub async fn project_add(
    state: State<'_, ProjectRegistry>,
    path: String,
) -> Result<ProjectEntry, String> {
    state.add(&path).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn project_remove(
    state: State<'_, ProjectRegistry>,
    project_id: String,
) -> Result<(), String> {
    let id = project_id
        .parse::<ProjectId>()
        .map_err(|e| nexus_core::NexusError::InvalidInput(e).to_string())?;
    if state.remove(id) {
        Ok(())
    } else {
        Err(nexus_core::NexusError::SessionNotFound(project_id).to_string())
    }
}
```

(注:remove 未命中复用 `SessionNotFound` 变体会产生"会话不存在"的误导文案——改用 `InvalidInput(format!("项目不存在: {project_id}"))`,上面以编译器接受为准写后者:)

```rust
    if state.remove(id) {
        Ok(())
    } else {
        Err(nexus_core::NexusError::InvalidInput(format!("项目不存在: {project_id}")).to_string())
    }
```

- [ ] **Step 3: git_* 三命令(worktree.rs 追加)+ session.rs FromStr 加固**

`commands/worktree.rs` 追加:

```rust
/// 工作区状态(porcelain v2):分支/ahead/behind/变更条目/截断标志。
#[tauri::command]
pub async fn git_status(
    state: State<'_, WorktreeManager>,
    repo_path: String,
) -> Result<nexus_core::gitx::ops::GitStatus, String> {
    ensure_worktree_ready(&state).await?;
    state
        .status(std::path::Path::new(&repo_path))
        .await
        .map_err(|e| e.to_string())
}

/// 暂存:paths 缺省 = 全部暂存。
#[tauri::command]
pub async fn git_stage(
    state: State<'_, WorktreeManager>,
    repo_path: String,
    paths: Option<Vec<std::path::PathBuf>>,
) -> Result<(), String> {
    ensure_worktree_ready(&state).await?;
    state
        .stage(
            std::path::Path::new(&repo_path),
            paths.as_deref(),
        )
        .await
        .map_err(|e| e.to_string())
}

/// 提交暂存区(空信息后端拒绝)。
#[tauri::command]
pub async fn git_commit(
    state: State<'_, WorktreeManager>,
    repo_path: String,
    message: String,
) -> Result<(), String> {
    ensure_worktree_ready(&state).await?;
    state
        .commit(std::path::Path::new(&repo_path), &message)
        .await
        .map_err(|e| e.to_string())
}
```

`commands/session.rs` 的 `session_create` 里 `(Some(repo), Some(name))` 分支开头插入(M3 必办#1:`../` 逃逸加固):

```rust
        (Some(repo), Some(name)) => {
            // 必办#1:worktree_name 必须是 nexus 命名规范的合法名(FromStr 校验),
            // 拒绝 ../ 等路径逃逸;worktree_remove 不校验——它按 name 精确匹配
            // 列表条目,无路径拼接,外建 worktree 名也要能删。
            let wt_name = nexus_core::ids::WorktreeName::from_str(name)
                .map_err(|e| nexus_core::NexusError::InvalidInput(e).to_string())?;
            let wt_dir = std::path::PathBuf::from(repo)
                .join(".nx-worktrees")
                .join(wt_name.as_str());
            // ...其后的 canonicalize/LaunchSpec 构造不变(name 字段仍用入参字符串)...
```

(实现时把后续 `join(name)` 改为 `join(wt_name.as_str())`,`LaunchSpec.worktree_name` 用 `wt_name.as_str().to_string()`,彻底杜绝双源。)

- [ ] **Step 4: lib.rs 组装 + mod.rs 注册**

`lib.rs` setup 内,把 ops Arc 共享给两个 manager(替换现有 `let ops ... ; app.manage(WorktreeManager::new(ops, wt_events));` 两行):

```rust
            let ops: Arc<dyn GitOps> = Arc::new(GitCliOps::new());
            app.manage(WorktreeManager::new(ops.clone(), wt_events));
            // 项目注册表:与 WorktreeManager 共享同一 GitOps 实例
            app.manage(ProjectRegistry::new(
                app.path().app_config_dir().expect("解析应用配置目录失败"),
                ops,
            ));
```

`generate_handler!` 追加六项:`commands::project_list, commands::project_add, commands::project_remove, commands::git_status, commands::git_stage, commands::git_commit`;头部 use 加 `use nexus_core::registry::ProjectRegistry;`。`commands/mod.rs` 加 `pub mod project;` + `pub use project::*;`。

- [ ] **Step 5: TS 契约与封装**

`src/ipc/types.ts` 追加:

```typescript
/** project_list / project_add 返回(registry/mod.rs ProjectEntry) */
export interface ProjectEntry {
  id: string;
  name: string;
  path: string;
  addedAtMs: number;
}

/** porcelain v2 单侧状态(gitx/ops.rs FileStatus) */
export type FileStatus =
  | "modified"
  | "added"
  | "deleted"
  | "renamed"
  | "copied"
  | "untracked"
  | "unmerged";

/** git_status 条目(gitx/ops.rs GitStatusEntry);null = 该侧无变化 */
export interface GitStatusEntry {
  path: string;
  index: FileStatus | null;
  worktree: FileStatus | null;
  origPath: string | null;
}

/** git_status 返回(gitx/ops.rs GitStatus) */
export interface GitStatus {
  branch: string | null;
  ahead: number;
  behind: number;
  entries: GitStatusEntry[];
  truncated: boolean;
}
```

`src/ipc/commands.ts` 追加:

```typescript
export function projectList(): Promise<ProjectEntry[]> {
  return invoke<ProjectEntry[]>("project_list");
}

/** 入册(仅 git 仓库;后端归一 canonical 根并追加 repo 本地 exclude) */
export function projectAdd(path: string): Promise<ProjectEntry> {
  return invoke<ProjectEntry>("project_add", { path });
}

export function projectRemove(projectId: string): Promise<void> {
  return invoke<void>("project_remove", { projectId });
}

export function gitStatus(repoPath: string): Promise<GitStatus> {
  return invoke<GitStatus>("git_status", { repoPath });
}

/** paths 缺省 = 全部暂存 */
export function gitStage(repoPath: string, paths?: string[]): Promise<void> {
  return invoke<void>("git_stage", {
    repoPath,
    paths: paths ?? null,
  });
}

export function gitCommit(repoPath: string, message: string): Promise<void> {
  return invoke<void>("git_commit", { repoPath, message });
}
```

(import 块同步补 `GitStatus, GitStatusEntry, ProjectEntry, FileStatus`。)

- [ ] **Step 6: 门槛 + 提交**

```bash
cargo test --workspace && cargo fmt && cargo clippy --workspace --all-targets -- -D warnings
pnpm build
git add -A && git commit -m "feat(m4): IPC——project_*/git_* 命令 + worktree_name FromStr 加固 + TS 契约

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 4: 亮色唯一主题迁移 + 字体本地打包

**Files:**
- Modify: `package.json`(`pnpm add @fontsource-variable/geist @fontsource/jetbrains-mono`)
- Modify: `src/main.tsx`(字体 CSS import)
- Modify: `src/index.css`(亮色令牌 + 状态色 + 字体变量 + 基础排版)
- Modify: `src/features/terminal/terminalManager.ts`(xterm 亮色主题)
- Modify: `src/features/terminal/TerminalPane.tsx`(退出遮罩亮色化)
- Modify: `src/features/terminal/TerminalTabs.tsx`(状态徽章亮色适配,一行)
- Modify: `src-tauri/tauri.conf.json`(最小窗口尺寸)

**Interfaces:**
- Consumes: —
- Produces: 亮色令牌体系(CSS 变量 + Tailwind 工具类)、状态色工具类(`text-status-run` 等)、`font-sans`/`font-mono` 字体栈、xterm 亮色配色;后续所有前端任务默认在亮色下开发

**学习点:** ① Tailwind v4 的 `@theme inline` 把 CSS 变量映射成工具类(如 `--color-status-ok` → `bg-status-ok`),换主题只换 `:root` 值——这套结构 M3 已建,本任务只换值+扩展;② 桌面应用禁止 CDN 字体:fontsource 包把 woff2 打进 bundle,离线可用(orca 是把 woff2 放 assets 自写 @font-face,fontsource 是等价的包管理形态);③ 亮色下发丝边框要用**带透明度的深色**(`oklch(0.20 0 0 / 0.12)`),纯灰在白底上会消失;原型的亮色值(`:root[data-theme="light"]`)就是照此设计的,直接照抄。

- [ ] **Step 1: 装字体包 + main.tsx**

```bash
pnpm add @fontsource-variable/geist @fontsource/jetbrains-mono
```

`src/main.tsx` 顶部(vite CSS import,先于 App):

```typescript
import "@fontsource-variable/geist";
import "@fontsource/jetbrains-mono/400.css";
import "@fontsource/jetbrains-mono/500.css";
```

(若任一包名不存在:退路——从 Google Fonts 下载 Geist 可变 woff2 与 JetBrains Mono 400/500 woff2 放 `src/assets/fonts/`,在 index.css 手写 `@font-face` + `font-display: swap`,效果等价。)

- [ ] **Step 2: index.css 重写为亮色令牌**

`src/index.css` 全量替换(`@custom-variant dark` 收紧保留——vendored shadcn 组件里的 dark: 类继续恒不生效):

```css
@import "tailwindcss";
@import "tw-animate-css";

/* dark: 变体确定性 inert:vendored shadcn 组件内含 dark: 类,默认 v4 语义跟
   prefers-color-scheme 走;本应用亮色唯一(永不挂 .dark),收紧为类选择器后
   dark: 类恒不生效,组件外观跨系统主题稳定。 */
@custom-variant dark (&:where(.dark, .dark *));

/* 亮色为唯一主题(spec v1.2):原型 astra-ade-prototype-v2 的 light 令牌。
   结构全走 CSS 变量——未来加暗色只增 .dark 块,零重构。 */
:root {
  --background: oklch(0.985 0 0);
  --foreground: oklch(0.18 0 0);
  --card: oklch(1 0 0);
  --card-foreground: oklch(0.18 0 0);
  --popover: oklch(1 0 0);
  --popover-foreground: oklch(0.18 0 0);
  /* 主操作色:亮色下是近黑(原型 primary 反转语义) */
  --primary: oklch(0.205 0 0);
  --primary-foreground: oklch(0.985 0 0);
  --secondary: oklch(0.97 0 0);
  --secondary-foreground: oklch(0.18 0 0);
  --muted: oklch(0.97 0 0);
  --muted-foreground: oklch(0.50 0 0);
  --accent: oklch(0.955 0 0);
  --accent-foreground: oklch(0.18 0 0);
  --destructive: oklch(0.55 0.2 25);
  --border: oklch(0.20 0 0 / 0.12);
  --input: oklch(0.20 0 0 / 0.16);
  --ring: oklch(0.52 0.17 255 / 0.45);
  --radius: 0.5rem;

  /* 状态色(brand-spec:颜色即状态——蓝=运行/选中,琥珀=警告,绿=通过,红=破坏) */
  --status-ok: oklch(0.55 0.14 152);
  --status-warn: oklch(0.62 0.13 75);
  --status-run: oklch(0.52 0.16 250);
  --status-err: oklch(0.55 0.2 25);
  /* 选中环/焦点(accent 蓝的实色形态,ring 是它的透明形态) */
  --focus: oklch(0.52 0.17 255);
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
  --color-focus: var(--focus);
  --color-status-ok: var(--status-ok);
  --color-status-warn: var(--status-warn);
  --color-status-run: var(--status-run);
  --color-status-err: var(--status-err);
  --radius-sm: calc(var(--radius) - 4px);
  --radius-md: calc(var(--radius) - 2px);
  --radius-lg: var(--radius);
  --radius-xl: calc(var(--radius) + 4px);

  /* 字体(spec §1.5:Geist 正文 + JetBrains Mono 等宽,均本地打包) */
  --font-sans: "Geist Variable", -apple-system, "Segoe UI", system-ui, sans-serif;
  --font-mono: "JetBrains Mono", ui-monospace, "SF Mono", Menlo, monospace;
}

html,
body,
#root {
  height: 100%;
}

body {
  /* 原型 brand-spec:13px 正文、letter-spacing 0.01em、overflow:hidden 防滚动条 */
  @apply bg-background text-foreground overflow-hidden font-sans text-[13px] tracking-[0.01em];
}
```

- [ ] **Step 3: xterm 亮色 + 遮罩/徽章亮色化**

`terminalManager.ts` 的 `createEntry` 里 `new Terminal({...})` 增加 `theme`(字面量对齐令牌,注释说明来源):

```typescript
  const terminal = new Terminal({
    fontFamily: config.fontFamily ?? "JetBrains Mono, ui-monospace, 'SF Mono', Menlo, monospace",
    fontSize: config.fontSize,
    cursorBlink: true,
    scrollback: config.scrollback,
    // 亮色主题(令牌的 xterm 端字面量;xterm 需要具体颜色值,不接受 CSS 变量)
    theme: {
      background: "#fafafa", // ≈ oklch(0.985 0 0)
      foreground: "#242424", // ≈ oklch(0.18 0 0)
      cursor: "#242424",
      selectionBackground: "rgba(85, 120, 220, 0.25)", // ≈ focus 蓝
    },
  });
```

(`DEFAULT_FONT_STACK` 常量同步改为上行的 JetBrains Mono 栈。)

`TerminalPane.tsx` 退出遮罩类:`bg-black/60 text-gray-300` → `bg-background/70 text-muted-foreground`。
`TerminalTabs.tsx` 的 `STATE_META` 配色换亮色可读版:`running → text-emerald-700/bg-emerald-600`、`stopping → text-amber-700/bg-amber-600`、`exited → text-gray-500/bg-gray-400`、`failed → text-red-700/bg-red-600`(该组件 Task 5 删除,这里只保证过渡期不刺眼)。

- [ ] **Step 4: 最小窗口尺寸**

`src-tauri/tauri.conf.json` 的 app windows 节加(spec §1.5:约 960px 起步,四栏不再自动降级):

```json
        "minWidth": 960,
        "minHeight": 600
```

(键名以该文件现有结构为准;若已有 width/height 则并列添加。)

- [ ] **Step 5: 验证 + 提交**

```bash
pnpm build && cargo test --workspace
pnpm tauri dev   # 目检:全界面亮色无暗色残留、字体生效、终端白底黑字
git add -A && git commit -m "feat(m4): 亮色唯一主题迁移 + Geist/JetBrains Mono 本地打包

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 5: tabStore + Workbench 四栏骨架 + 终端 tab 迁移

**Files:**
- Create: `src/stores/tabStore.ts`
- Create: `src/app/Workbench.tsx`
- Create: `src/app/useWorkbenchLayout.ts`
- Create: `src/app/Rail.tsx`
- Create: `src/features/tabs/TabStrip.tsx`
- Create: `src/features/tabs/TabBody.tsx`
- Modify: `src/App.tsx`(重写为组装根)
- Modify: `src/stores/sessionsStore.ts`(去 activeId/closeTab,加 removeSession)
- Delete: `src/features/terminal/TerminalTabs.tsx`

**Interfaces:**
- Consumes: 既有 TerminalPane/terminalManager/sessionsStore/事件封装;Task 4 亮色体系
- Produces(Task 6/7/8 依赖):
  - `tabStore`:`Tab = { id, kind: "terminal", sessionId, label, repoPath: string | null, worktreeName: string | null }`、`useTabs`(tabs/activeTabId/openTerminal(snap)/setActive/closeTab(id)/rebuildFromSessions(snaps));tab id 稳定为 `term-<sessionId>`;标题取最低空闲序号(`终端 1` 复用)
  - `sessionsStore`:`removeSession(id)`(dispose 收尾用);**不再有** activeId/closeTab
  - `useWorkbenchLayout()`:`{ sideOpen, ctxOpen, toggleSide, toggleCtx }`(localStorage 持久化)
  - Workbench 四栏(rail 52px | side 272px | center | ctx 296px);本任务 side/ctx 为占位(Task 6/8 填充)

**学习点:** ① "session 是数据、tab 是视图":sessionsStore 只存快照表,激活态/标签序归 tabStore——两职责解耦后,M5 加会话 UI 只是新增一种 tab kind,不碰 sessionsStore;② 挂载闩锁(orca 模式):TabBody 渲染全部已打开 tab 的 pane、按 active 切 display——组件从不因切换卸载,与 M2/M3 行为完全一致,只是渲染位置从 App 挪进 TabBody;③ 最低空闲序号标题(orca 同款):关闭「终端 2」后新建会复用该名,标题生成是纯函数,便于测试。

- [ ] **Step 1: tabStore.ts**

```typescript
// tab 视图层(spec §1.5):session 是数据(sessionsStore),tab 是视图(这里)。
// 单一 tab 模型:kind 可扩展(M4 仅 terminal);id 稳定 = `term-<sessionId>`。
// 不做 tab 持久化:重启由 session_list 重建(rebuildFromSessions)。
import { create } from "zustand";

import type { SessionSnapshot } from "../ipc/types";

export type TabKind = "terminal";

export interface Tab {
  id: string;
  kind: TabKind;
  sessionId: string;
  label: string;
  /** 会话快照的落点字段镜像(树上挂节点/新建终端 cwd 决策用) */
  repoPath: string | null;
  worktreeName: string | null;
}

function tabOf(snap: SessionSnapshot, label: string): Tab {
  return {
    id: `term-${snap.sessionId}`,
    kind: "terminal",
    sessionId: snap.sessionId,
    label,
    repoPath: snap.repoPath ?? null,
    worktreeName: snap.worktreeName ?? null,
  };
}

function lowestFreeLabel(taken: string[]): string {
  let n = 1;
  while (taken.includes(`终端 ${n}`)) n += 1;
  return `终端 ${n}`;
}

interface TabState {
  tabs: Tab[];
  activeTabId: string | null;
  /** 新会话落 tab(已存在则仅激活) */
  openTerminal: (snap: SessionSnapshot) => void;
  setActive: (id: string | null) => void;
  /** 关 tab 视图(会话数据的清理由调用方——App 的 dispose 流程——负责) */
  closeTab: (id: string) => void;
  /** 启动恢复:按快照序(后端已按 startedAtMs 排)重建终端 tab */
  rebuildFromSessions: (snaps: SessionSnapshot[]) => void;
}

export const useTabs = create<TabState>((set) => ({
  tabs: [],
  activeTabId: null,
  openTerminal: (snap) =>
    set((st) => {
      const id = `term-${snap.sessionId}`;
      if (st.tabs.some((t) => t.id === id)) return { activeTabId: id };
      const label = lowestFreeLabel(st.tabs.map((t) => t.label));
      return { tabs: [...st.tabs, tabOf(snap, label)], activeTabId: id };
    }),
  setActive: (id) => set({ activeTabId: id }),
  closeTab: (id) =>
    set((st) => {
      const idx = st.tabs.findIndex((t) => t.id === id);
      if (idx < 0) return st;
      const tabs = st.tabs.filter((t) => t.id !== id);
      const activeTabId =
        st.activeTabId === id
          ? (tabs[Math.max(0, idx - 1)]?.id ?? null)
          : st.activeTabId;
      return { tabs, activeTabId };
    }),
  rebuildFromSessions: (snaps) =>
    set(() => {
      const taken: string[] = [];
      const tabs = snaps.map((s) => {
        const label = lowestFreeLabel(taken);
        taken.push(label);
        return tabOf(s, label);
      });
      return { tabs, activeTabId: tabs[0]?.id ?? null };
    }),
}));
```

- [ ] **Step 2: sessionsStore 瘦身**

`sessionsStore.ts`:接口与实现里删掉 `activeId`、`setActive`、`closeTab`;`hydrate` 不再设 activeId;追加:

```typescript
  /** dispose 收尾:从快照表移除(Rust 侧条目已删) */
  removeSession: (id: string) => void;
```

实现:

```typescript
  removeSession: (id) =>
    set((st) => {
      const sessions = { ...st.sessions };
      delete sessions[id];
      return { sessions };
    }),
```

- [ ] **Step 3: 布局钩子 + Rail + TabStrip + TabBody + Workbench**

`src/app/useWorkbenchLayout.ts`:

```typescript
// 布局派生钩子(spec §1.5,orca useAppChromeLayout 最小版):
// 回答"侧栏/右面板是否展开";开合持久化到 localStorage。
import { useCallback, useState } from "react";

function readFlag(key: string): boolean {
  return localStorage.getItem(key) !== "0";
}

export function useWorkbenchLayout() {
  const [sideOpen, setSideOpen] = useState(() => readFlag("nx.sideOpen"));
  const [ctxOpen, setCtxOpen] = useState(() => readFlag("nx.ctxOpen"));
  const toggleSide = useCallback(() => {
    setSideOpen((v) => {
      localStorage.setItem("nx.sideOpen", v ? "0" : "1");
      return !v;
    });
  }, []);
  const toggleCtx = useCallback(() => {
    setCtxOpen((v) => {
      localStorage.setItem("nx.ctxOpen", v ? "0" : "1");
      return !v;
    });
  }, []);
  return { sideOpen, ctxOpen, toggleSide, toggleCtx };
}
```

`src/app/Rail.tsx`:

```tsx
// 左侧图标导航(原型 rail):项目(本里程碑)/任务·市场·设置(占位禁用)。
// 折叠钮控制两侧面板(useWorkbenchLayout)。
import { Blocks, LayoutGrid, ListChecks, PanelLeft, PanelRight, Settings } from "lucide-react";

interface Props {
  sideOpen: boolean;
  ctxOpen: boolean;
  onToggleSide: () => void;
  onToggleCtx: () => void;
}

export default function Rail({ sideOpen, ctxOpen, onToggleSide, onToggleCtx }: Props) {
  return (
    <nav
      aria-label="主导航"
      className="flex w-[52px] flex-col items-center gap-1 border-r border-border py-2"
    >
      <button
        type="button"
        title="项目"
        className="relative grid size-[38px] place-items-center rounded-md bg-card text-foreground"
      >
        <LayoutGrid className="size-5" />
        {/* 激活指示条(原型:左侧 3px 竖条) */}
        <span className="absolute -left-2 h-[18px] w-[3px] rounded-full bg-focus" />
      </button>
      <button
        type="button"
        title="任务(后续里程碑提供)"
        disabled
        className="grid size-[38px] cursor-not-allowed place-items-center rounded-md text-muted-foreground/50"
      >
        <ListChecks className="size-5" />
      </button>
      <button
        type="button"
        title="插件市场(后续里程碑提供)"
        disabled
        className="grid size-[38px] cursor-not-allowed place-items-center rounded-md text-muted-foreground/50"
      >
        <Blocks className="size-5" />
      </button>
      <div className="flex-1" />
      <button
        type="button"
        title="设置(后续里程碑提供)"
        disabled
        className="grid size-[38px] cursor-not-allowed place-items-center rounded-md text-muted-foreground/50"
      >
        <Settings className="size-5" />
      </button>
      <button
        type="button"
        title={sideOpen ? "收起项目栏" : "展开项目栏"}
        onClick={onToggleSide}
        className="grid size-[38px] place-items-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground"
      >
        <PanelLeft className="size-5" />
      </button>
      <button
        type="button"
        title={ctxOpen ? "收起右面板" : "展开右面板"}
        onClick={onToggleCtx}
        className="grid size-[38px] place-items-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground"
      >
        <PanelRight className="size-5" />
      </button>
    </nav>
  );
}
```

`src/features/tabs/TabStrip.tsx`(本任务无「+」——Task 7 加菜单):

```tsx
// 多类型标签条(原型 tabstrip):状态点 + 标题 + kind 徽标 + 关闭钮。
// 数据:tabStore(视图)+ sessionsStore(状态点);关闭回调走 App 的 M3
// dispose 流程(handleClose)。
import { useSessions } from "../../stores/sessionsStore";
import { useTabs } from "../../stores/tabStore";
import type { SessionState } from "../../ipc/types";

const DOT: Record<SessionState, string> = {
  running: "bg-status-run",
  stopping: "bg-status-warn",
  exited: "bg-gray-400",
  failed: "bg-status-err",
};

interface Props {
  onClose: (sessionId: string) => void;
}

export default function TabStrip({ onClose }: Props) {
  const tabs = useTabs((s) => s.tabs);
  const activeTabId = useTabs((s) => s.activeTabId);
  const setActive = useTabs((s) => s.setActive);
  const sessions = useSessions((s) => s.sessions);

  return (
    <div className="flex min-h-[37px] items-stretch border-b border-border">
      <div className="flex min-w-0 items-stretch overflow-x-auto [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
        {tabs.map((tab) => {
          const state = sessions[tab.sessionId]?.state;
          const active = tab.id === activeTabId;
          return (
            <button
              key={tab.id}
              type="button"
              onClick={() => setActive(tab.id)}
              title={tab.repoPath ?? tab.sessionId}
              className={`flex max-w-[232px] items-center gap-1.5 border-r border-border px-2.5 text-xs whitespace-nowrap ${
                active
                  ? "border-t-2 border-t-focus bg-card text-foreground"
                  : "text-muted-foreground hover:bg-accent hover:text-foreground"
              }`}
            >
              {state && <span className={`size-1.5 shrink-0 rounded-full ${DOT[state]}`} />}
              <span className="truncate">{tab.label}</span>
              <span className="font-mono text-[10px] text-muted-foreground/70">终端</span>
              <span
                role="button"
                aria-label="关闭标签页"
                title="关闭标签页"
                onClick={(e) => {
                  e.stopPropagation();
                  onClose(tab.sessionId);
                }}
                className="grid size-4 place-items-center rounded text-muted-foreground/60 hover:bg-secondary hover:text-foreground"
              >
                ×
              </span>
            </button>
          );
        })}
      </div>
    </div>
  );
}
```

`src/features/tabs/TabBody.tsx`:

```tsx
// tab 内容区:全部已打开 tab 的 pane 恒久挂载、按 active 切换可见(挂载闩锁,
// spec §1.5 红线——切 tab 不销毁 xterm)。M4 仅 terminal kind;后续 kind 在
// switch 处扩展,未知 kind 显示占位。
import TerminalPane from "../terminal/TerminalPane";
import { useTabs } from "../../stores/tabStore";

interface Props {
  onFitted: (id: string, cols: number, rows: number) => void;
}

export default function TabBody({ onFitted }: Props) {
  const tabs = useTabs((s) => s.tabs);
  const activeTabId = useTabs((s) => s.activeTabId);

  return (
    <div className="relative min-h-0 min-w-0 flex-1">
      {tabs.map((tab) => (
        <div
          key={tab.id}
          className={tab.id === activeTabId ? "h-full w-full" : "hidden"}
        >
          <TerminalPane sessionId={tab.sessionId} onFitted={onFitted} />
        </div>
      ))}
      {tabs.length === 0 && (
        <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
          暂无标签——从左侧项目树或标签栏「+」开始
        </div>
      )}
    </div>
  );
}
```

`src/app/Workbench.tsx`:

```tsx
// 四栏 workbench(spec §1.5):rail(52) | 项目树(272,可折叠) | 标签中心 |
// 右面板(296,可折叠)。本任务侧栏/右面板为占位(Task 6/8 填充)。
import Rail from "./Rail";
import { useWorkbenchLayout } from "./useWorkbenchLayout";
import TabBody from "../features/tabs/TabBody";
import TabStrip from "../features/tabs/TabStrip";

interface Props {
  onCloseTab: (sessionId: string) => void;
  onFitted: (id: string, cols: number, rows: number) => void;
}

export default function Workbench({ onCloseTab, onFitted }: Props) {
  const { sideOpen, ctxOpen, toggleSide, toggleCtx } = useWorkbenchLayout();
  return (
    <div className="flex h-screen min-w-0">
      <Rail sideOpen={sideOpen} ctxOpen={ctxOpen} onToggleSide={toggleSide} onToggleCtx={toggleCtx} />
      <aside className="flex w-[272px] min-w-0 flex-col border-r border-border">
        <div className="flex min-h-[37px] items-center border-b border-border px-3 text-xs font-semibold tracking-wide text-muted-foreground">
          项目
        </div>
        <div className="flex flex-1 items-center justify-center p-4 text-center text-xs text-muted-foreground">
          项目树(下一步提供)
        </div>
      </aside>
      <main className="flex min-w-0 flex-1 flex-col">
        <TabStrip onClose={onCloseTab} />
        <TabBody onFitted={onFitted} />
      </main>
      <aside className="flex w-[296px] min-w-0 flex-col border-l border-border">
        <div className="flex min-h-[37px] items-center border-b border-border px-3 text-xs font-semibold tracking-wide text-muted-foreground">
          Git
        </div>
        <div className="flex flex-1 items-center justify-center p-4 text-center text-xs text-muted-foreground">
          Git 面板(后续任务提供)
        </div>
      </aside>
    </div>
  );
}
```

(注:sideOpen/ctxOpen 折叠在本组件内直接条件渲染 aside——折叠即卸载侧栏不影响终端闩锁;Workbench 重渲染不触发 TerminalPane 卸载。)

- [ ] **Step 4: App.tsx 重写(组装根)**

保留 M3 的:启动恢复链、全局事件接线、handleClose/dispose 流程、确认 AlertDialog、error footer;布局换 Workbench;hydrate 后重建 tabs;删除 handleNew/newDialogOpen(NewSessionDialog 本任务暂留不挂载,Task 7 删文件):

```tsx
// M4 组装根:启动恢复 + 全局事件 + 关 tab 流程(M3 语义不变)。
// 布局在 app/Workbench;会话数据 sessionsStore,tab 视图 tabStore。
import { useCallback, useEffect, useRef, useState } from "react";

import {
  AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent,
  AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Checkbox } from "@/components/ui/checkbox";
import { Label } from "@/components/ui/label";

import Workbench from "./app/Workbench";
import {
  disposeEntry, resizeSession, setTerminalConfig, stopSession,
} from "./features/terminal/terminalManager";
import {
  configGet, gitCheck, sessionDispose, sessionList, worktreeRemove,
} from "./ipc/commands";
import { onSessionExitEvent, onSessionStateEvent, onWorktreeChanged } from "./ipc/events";
import type { GitCheckInfo } from "./ipc/types";
import { useSessions } from "./stores/sessionsStore";
import { useTabs } from "./stores/tabStore";
import { useWorktrees } from "./stores/worktreeStore";

function App() {
  const hydrate = useSessions((s) => s.hydrate);

  const [gitInfo, setGitInfo] = useState<GitCheckInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  const bootedRef = useRef(false);
  const closingRef = useRef(new Set<string>());

  const [confirmClose, setConfirmClose] = useState<{
    id: string;
    worktree?: { repoPath: string; name: string };
  } | null>(null);
  const [alsoRemoveWt, setAlsoRemoveWt] = useState(true);

  // 启动恢复:config → git 探测 → session_list(并重建 tab 视图)
  useEffect(() => {
    if (bootedRef.current) return;
    bootedRef.current = true;
    void (async () => {
      try {
        setTerminalConfig((await configGet()).terminal);
      } catch (e) {
        console.error("[app] config_get 失败,用内置默认终端配置", e);
      }
      try {
        setGitInfo(await gitCheck());
      } catch (e) {
        console.error("[app] git_check 失败", e);
      }
      try {
        const snaps = await sessionList();
        hydrate(snaps);
        useTabs.getState().rebuildFromSessions(snaps);
      } catch (e) {
        console.error("[app] session_list 失败,按空会话启动", e);
      }
    })();
  }, [hydrate]);

  // 全局事件 → store(M3 不变;worktreeStore 在 Task 6 被 projectStore 吸收)
  useEffect(() => {
    const unState = onSessionStateEvent((sc) =>
      useSessions.getState().onState(sc.sessionId, sc.next)
    );
    const unExit = onSessionExitEvent((ev) => useSessions.getState().onExit(ev));
    const unWt = onWorktreeChanged((ev) =>
      useWorktrees.getState().applyChange(ev)
    );
    return () => {
      void unState.then((u) => u());
      void unExit.then((u) => u());
      void unWt.then((u) => u());
    };
  }, []);

  const handleFitted = useCallback((id: string, cols: number, rows: number) => {
    resizeSession(id, cols, rows);
  }, []);

  // 关 tab 流程(M3 原样):终态直接 dispose;运行中确认(可选删 worktree)
  const handleClose = useCallback(
    (sessionId: string) => {
      const snap = useSessions.getState().sessions[sessionId];
      if (!snap || closingRef.current.has(sessionId)) return;
      if (snap.state === "exited" || snap.state === "failed") {
        closingRef.current.add(sessionId);
        void sessionDispose(sessionId)
          .catch((e) => console.error("[app] dispose 失败", e))
          .finally(() => {
            closingRef.current.delete(sessionId);
            disposeEntry(sessionId);
            useSessions.getState().removeSession(sessionId);
            useTabs.getState().closeTab(`term-${sessionId}`);
          });
        return;
      }
      setConfirmClose({
        id: sessionId,
        worktree:
          snap.repoPath && snap.worktreeName
            ? { repoPath: snap.repoPath, name: snap.worktreeName }
            : undefined,
      });
    },
    []
  );

  const confirmStopAndClose = useCallback(async () => {
    if (!confirmClose) return;
    const { id, worktree } = confirmClose;
    setConfirmClose(null);
    if (closingRef.current.has(id)) return;
    closingRef.current.add(id);
    const finish = (): void => {
      closingRef.current.delete(id);
      disposeEntry(id);
      useSessions.getState().removeSession(id);
      useTabs.getState().closeTab(`term-${id}`);
    };
    try {
      await stopSession(id);
    } catch {
      const cur = useSessions.getState().sessions[id];
      if (cur && cur.state !== "exited" && cur.state !== "failed") {
        closingRef.current.delete(id);
        setError(`停止会话失败:当前状态 ${cur.state}`);
        return;
      }
    }
    if (worktree && alsoRemoveWt) {
      await worktreeRemove(worktree.repoPath, worktree.name, true).catch((e) =>
        setError(`worktree 清理失败:${String(e)}`)
      );
    }
    await sessionDispose(id).catch(() => {});
    finish();
  }, [confirmClose, alsoRemoveWt]);

  return (
    <>
      <Workbench onCloseTab={handleClose} onFitted={handleFitted} />
      {error && (
        <footer className="fixed inset-x-0 bottom-0 bg-destructive/10 px-4 py-1 text-sm text-destructive">
          {error}
        </footer>
      )}
      <AlertDialog
        open={confirmClose !== null}
        onOpenChange={(v) => {
          if (!v) setConfirmClose(null);
        }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>停止并关闭会话?</AlertDialogTitle>
            <AlertDialogDescription>
              会话仍在运行:关闭会先优雅停止(Ctrl+C → 宽限 → 超时强杀),输出历史随之释放。
            </AlertDialogDescription>
          </AlertDialogHeader>
          {confirmClose?.worktree && (
            <div className="flex items-center gap-2">
              <Checkbox
                id="also-remove-wt"
                checked={alsoRemoveWt}
                onCheckedChange={(v) => setAlsoRemoveWt(v === true)}
              />
              <Label htmlFor="also-remove-wt">同时删除 worktree(分支与目录)</Label>
            </div>
          )}
          <AlertDialogFooter>
            <AlertDialogCancel>取消</AlertDialogCancel>
            <AlertDialogAction onClick={() => void confirmStopAndClose()}>
              停止并关闭
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  );
}

export default App;
```

同时删除 `src/features/terminal/TerminalTabs.tsx`(App 已不引用;grep 确认无残留 import)。

- [ ] **Step 5: 构建 + 基线验收 + 提交**

```bash
pnpm build && cargo test --workspace
pnpm tauri dev   # 基线:多 tab 并行/刷新恢复(tab 序不变)/关 tab 确认与删 worktree/退出遮罩,全部在四栏骨架下工作
git add -A && git commit -m "feat(m4): tabStore + Workbench 四栏骨架,终端 tab 迁移(挂载闩锁不变)

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 6: projectStore + 项目树 + 打开目录(多项目目录)

**Files:**
- Create: `src/stores/projectStore.ts`
- Create: `src/features/project/ProjectSide.tsx`
- Create: `src/features/project/ProjectTree.tsx`
- Create: `src/features/project/OpenDirOverlay.tsx`
- Modify: `src/app/Workbench.tsx`(占位侧栏换 ProjectSide)
- Modify: `src/App.tsx`(事件接线 worktreeStore → projectStore)
- Delete: `src/stores/worktreeStore.ts`

**Interfaces:**
- Consumes: Task 3 的 `projectList/projectAdd/projectRemove`、既有 `gitValidateRepo/worktreeList`、`worktree://changed` 事件、Task 5 的 `useTabs`
- Produces(Task 7/8 依赖):
  - `useProjects`:`projects: ProjectEntry[]`、`detail: Record<string, { info: RepoInfo | null; worktrees: WorktreeInfo[] }>`(键 = project.path)、`selected: { repoPath: string; worktreeName?: string } | null`、`loadAll()/refresh(repoPath)/addProject(path)/removeProject(id)/select(node)/applyWorktreeChange(ev)`
  - ProjectSide:标题行(「项目」+「打开目录」钮)+ 搜索框(本地过滤,可选实现)+ 树;git 不可用黄条迁移至此
  - 树交互:项目行点击=选中+展开;worktree 行=选中;session 行=选中+激活对应 tab;项目行悬停「×」=移除注册(不删磁盘)

**学习点:** ① 树是派生视图:projects(注册表)+ detail(git 状态/worktree 列表)+ sessions(快照表)三个数据源在组件内拼装,store 不存嵌套树(orca flat-rows 哲学的最小版——数据源各自可刷新,树结构零冗余);② worktree 列表的第一项是主 worktree(git 约定主项在最前),树里按 `path !== project.path` 过滤掉它——项目行本身就是主 worktree;③ 「最近打开」就是注册表本身(按 addedAtMs 排序),不再单存一份最近列表。

- [ ] **Step 1: projectStore.ts**

```typescript
// 项目域 store(spec §1.2):注册表镜像 + 每项目 git 状态/worktree 列表 + 选中节点。
// 选中节点是「新建终端」的 cwd 决策依据(T7);树结构不进 store(组件内派生)。
import { create } from "zustand";

import {
  gitValidateRepo,
  projectAdd,
  projectList,
  projectRemove,
  worktreeList,
} from "../ipc/commands";
import type { ProjectEntry, RepoInfo, WorktreeInfo } from "../ipc/types";

export interface ProjectDetail {
  info: RepoInfo | null;
  worktrees: WorktreeInfo[];
}

export interface Selection {
  repoPath: string;
  worktreeName?: string;
}

interface ProjectState {
  projects: ProjectEntry[];
  detail: Record<string, ProjectDetail>;
  selected: Selection | null;
  loadAll: () => Promise<void>;
  refresh: (repoPath: string) => Promise<void>;
  addProject: (path: string) => Promise<ProjectEntry>;
  removeProject: (id: string) => Promise<void>;
  select: (node: Selection) => void;
  /** worktree://changed 联动:该 repo 的列表失效重拉 */
  applyWorktreeChange: (ev: { repoPath: string }) => void;
}

export const useProjects = create<ProjectState>((set, get) => ({
  projects: [],
  detail: {},
  selected: null,
  loadAll: async () => {
    try {
      const projects = await projectList();
      set({ projects });
      await Promise.all(projects.map((p) => get().refresh(p.path)));
    } catch (e) {
      console.error("[project] project_list 失败", e);
    }
  },
  refresh: async (repoPath) => {
    try {
      const info = await gitValidateRepo(repoPath);
      const worktrees = await worktreeList(repoPath).catch(() => []);
      set((st) => ({ detail: { ...st.detail, [repoPath]: { info, worktrees } } }));
    } catch (e) {
      console.error("[project] 刷新失败", repoPath, e);
      set((st) => ({
        detail: { ...st.detail, [repoPath]: { info: null, worktrees: [] } },
      }));
    }
  },
  addProject: async (path) => {
    const entry = await projectAdd(path); // 非 git 目录等错误向上抛给 UI
    set((st) => ({
      projects: [...st.projects.filter((p) => p.id !== entry.id), entry].sort(
        (a, b) => a.addedAtMs - b.addedAtMs
      ),
    }));
    await get().refresh(entry.path);
    get().select({ repoPath: entry.path });
    return entry;
  },
  removeProject: async (id) => {
    await projectRemove(id);
    const st = get();
    const hit = st.projects.find((p) => p.id === id);
    set((cur) => {
      const detail = { ...cur.detail };
      if (hit) delete detail[hit.path];
      return {
        projects: cur.projects.filter((p) => p.id !== id),
        detail,
        selected:
          hit && cur.selected?.repoPath === hit.path ? null : cur.selected,
      };
    });
  },
  select: (node) => set({ selected: node }),
  applyWorktreeChange: (ev) => {
    if (get().detail[ev.repoPath]) void get().refresh(ev.repoPath);
  },
}));
```

- [ ] **Step 2: ProjectTree.tsx**

```tsx
// 项目树(原型 side):项目(branch 徽标) > worktree > session 三层。
// 三个数据源在组件内拼装:projects/detail(projectStore)+ sessions(sessionsStore)。
// session 挂载规则:snap.repoPath === 项目 path 时,有 worktreeName 挂对应
// worktree 下,否则挂项目根;不匹配任何项目的会话只出现在标签里(不进树)。
import { ChevronDown, FolderGit2, X } from "lucide-react";

import type { ProjectEntry, SessionSnapshot } from "../../ipc/types";
import { useProjects } from "../../stores/projectStore";
import { useSessions } from "../../stores/sessionsStore";
import { useTabs } from "../../stores/tabStore";

const DOT: Record<string, string> = {
  running: "bg-status-run",
  stopping: "bg-status-warn",
  exited: "bg-gray-400",
  failed: "bg-status-err",
};

function SessionRow({ snap }: { snap: SessionSnapshot }) {
  const selected = useProjects((s) => s.selected);
  const mine =
    selected?.repoPath === snap.repoPath &&
    selected?.worktreeName === snap.worktreeName;
  return (
    <button
      type="button"
      onClick={() => {
        useProjects.getState().select({
          repoPath: snap.repoPath ?? "",
          worktreeName: snap.worktreeName ?? undefined,
        });
        useTabs.getState().setActive(`term-${snap.sessionId}`);
      }}
      className={`flex w-full items-center gap-1.5 py-0.5 pr-2 pl-[50px] text-left text-xs ${
        mine ? "bg-card text-foreground" : "text-muted-foreground hover:bg-accent"
      }`}
    >
      <span className={`size-1.5 shrink-0 rounded-full ${DOT[snap.state]}`} />
      <span className="truncate">终端 · {snap.worktreeName?.split("/").pop() ?? snap.repoPath?.split("/").pop() ?? snap.sessionId.slice(0, 8)}</span>
    </button>
  );
}

function ProjectRow({ project }: { project: ProjectEntry }) {
  const detail = useProjects((s) => s.detail[project.path]);
  const selected = useProjects((s) => s.selected);
  const removeProject = useProjects((s) => s.removeProject);
  const mine = selected?.repoPath === project.path && !selected?.worktreeName;
  const branch = detail?.info?.currentBranch;
  return (
    <div className="group/project">
      <button
        type="button"
        onClick={() =>
          useProjects.getState().select({ repoPath: project.path })
        }
        className={`flex w-full items-center gap-1.5 rounded px-2 py-1 text-left text-[13px] ${
          mine ? "bg-card text-foreground" : "text-foreground hover:bg-accent"
        }`}
      >
        <ChevronDown className="size-3.5 shrink-0 text-muted-foreground/70" />
        <FolderGit2 className="size-3.5 shrink-0 text-muted-foreground" />
        <span className="truncate">{project.name}</span>
        <span className="flex-1" />
        {branch && (
          <span className="shrink-0 rounded border border-border px-1 font-mono text-[9.5px] text-muted-foreground">
            {branch}
          </span>
        )}
        <span
          role="button"
          aria-label="从列表移除(不删除磁盘)"
          title="从列表移除(不删除磁盘)"
          onClick={(e) => {
            e.stopPropagation();
            void removeProject(project.id);
          }}
          className="hidden shrink-0 rounded p-0.5 text-muted-foreground/60 hover:bg-secondary hover:text-foreground group-hover/project:block"
        >
          <X className="size-3" />
        </span>
      </button>
      {/* worktree 子节点:过滤主 worktree(项目行即主检出) */}
      {(detail?.worktrees ?? [])
        .filter((w) => w.path !== project.path)
        .map((w) => (
          <button
            key={w.path}
            type="button"
            title={w.path}
            onClick={() =>
              useProjects
                .getState()
                .select({ repoPath: project.path, worktreeName: w.name })
            }
            className={`flex w-full items-center gap-1.5 py-0.5 pr-2 pl-[34px] text-left text-xs ${
              selected?.repoPath === project.path && selected?.worktreeName === w.name
                ? "bg-card text-foreground"
                : "text-muted-foreground hover:bg-accent"
            }`}
          >
            <span className="size-1.5 shrink-0 rounded-full bg-focus/70" />
            <span className="truncate font-mono">{w.name.split("/").pop() ?? w.name}</span>
          </button>
        ))}
    </div>
  );
}

export default function ProjectTree() {
  const projects = useProjects((s) => s.projects);
  const sessions = useSessions((s) => s.sessions);

  if (projects.length === 0) {
    return (
      <p className="p-4 text-center text-xs text-muted-foreground">
        还没有项目——点右上角「打开目录」开始
      </p>
    );
  }
  return (
    <div className="flex-1 overflow-y-auto px-1.5 py-2">
      {projects.map((p) => {
        const mine = Object.values(sessions).filter(
          (s) => s.repoPath === p.path
        );
        const byWorktree = new Map<string, SessionSnapshot[]>();
        for (const s of mine) {
          const key = s.worktreeName ?? "";
          byWorktree.set(key, [...(byWorktree.get(key) ?? []), s]);
        }
        return (
          <div key={p.id} className="mb-1">
            <ProjectRow project={p} />
            {/* 挂项目根的会话(无 worktree) */}
            {(byWorktree.get("") ?? []).map((s) => (
              <SessionRow key={s.sessionId} snap={s} />
            ))}
          </div>
        );
      })}
    </div>
  );
}
```

(worktree 行下的 session 挂载:worktree 子行与 SessionRow 的呈现顺序在上面的最小版里是「项目根会话直接列在项目下」;worktree 节点下的会话按 `byWorktree.get(w.name)` 匹配——执行时把 worktree map 与其 session 一并渲染在 ProjectRow 的 worktree 循环里,结构与上面一致,不另造组件。)

- [ ] **Step 3: OpenDirOverlay.tsx + ProjectSide.tsx**

`OpenDirOverlay.tsx`:

```tsx
// 打开目录(原型 openDirOverlay):手输 + 原生浏览 + 最近打开(=注册表)。
// 提交即 project_add(后端校验 git 仓库、归一 canonical、追加 exclude)。
import { useCallback, useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle,
} from "@/components/ui/dialog";

import { useProjects } from "../../stores/projectStore";

interface Props {
  openState: boolean;
  onOpenChange: (v: boolean) => void;
  onError: (msg: string) => void;
}

export default function OpenDirOverlay({ openState, onOpenChange, onError }: Props) {
  const [path, setPath] = useState("");
  const [err, setErr] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const projects = useProjects((s) => s.projects);

  useEffect(() => {
    if (openState) {
      setPath("");
      setErr(null);
    }
  }, [openState]);

  const browse = useCallback(async () => {
    const picked = await open({ directory: true, multiple: false });
    if (typeof picked === "string") setPath(picked);
  }, []);

  const submit = useCallback(async () => {
    const p = path.trim();
    if (!p) return;
    setSubmitting(true);
    setErr(null);
    try {
      await useProjects.getState().addProject(p);
      onOpenChange(false);
    } catch (e) {
      const msg = `打开失败:${String(e)}`;
      setErr(msg);
      onError(msg);
    } finally {
      setSubmitting(false);
    }
  }, [path, onOpenChange, onError]);

  return (
    <Dialog open={openState} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>打开项目目录</DialogTitle>
        </DialogHeader>
        <div className="grid gap-3 py-1">
          <div className="flex gap-2">
            <Input
              value={path}
              placeholder="/path/to/repo(须为 git 仓库)"
              onChange={(e) => setPath(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") void submit();
              }}
            />
            <Button variant="secondary" onClick={() => void browse()}>
              浏览…
            </Button>
          </div>
          {projects.length > 0 && (
            <div>
              <p className="mb-1 text-xs text-muted-foreground">最近打开</p>
              <div className="max-h-40 overflow-y-auto">
                {projects.map((p) => (
                  <button
                    key={p.id}
                    type="button"
                    onClick={() => setPath(p.path)}
                    className="flex w-full items-center justify-between rounded px-2 py-1 text-left text-xs hover:bg-accent"
                  >
                    <span>{p.name}</span>
                    <span className="truncate pl-3 font-mono text-[10px] text-muted-foreground">
                      {p.path}
                    </span>
                  </button>
                ))}
              </div>
            </div>
          )}
          {err && <p className="text-xs text-destructive">{err}</p>}
        </div>
        <DialogFooter>
          <Button variant="ghost" onClick={() => onOpenChange(false)}>
            取消
          </Button>
          <Button onClick={() => void submit()} disabled={submitting || !path.trim()}>
            {submitting ? "打开中…" : "打开"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
```

`ProjectSide.tsx`(含 git 状态条——M3 顶栏黄条迁来):

```tsx
// 左栏(spec §1.5):标题行 + git 探测条 + 项目树。打开目录走 OpenDirOverlay。
import { useState } from "react";
import { FolderPlus } from "lucide-react";

import type { GitCheckInfo } from "../../ipc/types";
import ProjectTree from "./ProjectTree";
import OpenDirOverlay from "./OpenDirOverlay";

export default function ProjectSide({ gitInfo }: { gitInfo: GitCheckInfo | null }) {
  const [openDir, setOpenDir] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  return (
    <aside className="flex min-w-0 flex-col">
      <div className="flex min-h-[37px] items-center justify-between border-b border-border pr-2 pl-3">
        <span className="text-xs font-semibold tracking-wide text-muted-foreground">
          项目
        </span>
        <button
          type="button"
          title="打开项目目录"
          onClick={() => setOpenDir(true)}
          className="grid size-6 place-items-center rounded text-muted-foreground hover:bg-accent hover:text-foreground"
        >
          <FolderPlus className="size-4" />
        </button>
      </div>
      {gitInfo && !gitInfo.worktreeSupported && (
        <div className="border-b border-border bg-status-warn/10 px-3 py-1.5 text-[11px] text-status-warn">
          {gitInfo.available
            ? `git ${gitInfo.version ?? ""} 版本过低——worktree 功能需要 git ≥ 2.20`
            : "未检测到 git——请安装 git ≥ 2.20"}
        </div>
      )}
      {err && (
        <div className="border-b border-border bg-destructive/10 px-3 py-1 text-[11px] text-destructive">
          {err}
        </div>
      )}
      <ProjectTree />
      <OpenDirOverlay
        openState={openDir}
        onOpenChange={setOpenDir}
        onError={setErr}
      />
    </aside>
  );
}
```

`Workbench.tsx`:占位 aside 换 `<ProjectSide gitInfo={gitInfo} />`(gitInfo 经 props 从 App 传入——App 已有 gitInfo state);侧栏宽度 `w-[272px]` 与折叠逻辑不变。

- [ ] **Step 4: App 接线 + 删 worktreeStore**

`App.tsx`:启动链 `session_list` 之前加 `void useProjects.getState().loadAll()`(不 await,树异步填充);事件接线 `useWorktrees.getState().applyChange` → `useProjects.getState().applyWorktreeChange`;删除 `import { useWorktrees }`。删除文件 `src/stores/worktreeStore.ts`(grep `useWorktrees` 确认仅 NewSessionDialog 还引用——该文件 Task 7 删除,本任务先把它对 worktreeStore 的引用改为直接调 `gitValidateRepo/worktreeList`(内联两行)以解除依赖,或把删除 worktreeStore 挪到 Task 7 一并做——**取后者简单**:本任务 App 停用 worktreeStore 但文件保留,Task 7 删 NewSessionDialog 时一并删)。

- [ ] **Step 5: 构建 + 验收 + 提交**

```bash
pnpm build && cargo test --workspace
pnpm tauri dev
# 验收:打开目录(手输+浏览)→ 入树带 branch 徽标;重启后项目还在;
# 树上无 worktree 项目仅一层;M3 建过 worktree 的 repo 树上出现 worktree 子节点;
# 会话 tab 关闭/刷新恢复后树上 session 行可点击激活
git add -A && git commit -m "feat(m4): projectStore + 项目树 + 打开目录(多项目目录持久化)

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 7: 新建终端流(即时创建)+ NewWorktreePopover + 退役 NewSessionDialog

**Files:**
- Modify: `src/features/tabs/TabStrip.tsx`(「+」下拉菜单)
- Create: `src/features/project/NewWorktreePopover.tsx`
- Modify: `src/features/project/ProjectTree.tsx`(项目行悬停「+」)
- Delete: `src/features/launch/NewSessionDialog.tsx`、`src/stores/worktreeStore.ts`、`src/features/launch/` 目录(空后)

**Interfaces:**
- Consumes: Task 5 tabStore/sessionsStore.add、Task 6 useProjects.selected、既有 `sessionCreate/worktreeCreate/worktreeRemove`
- Produces: M4 的全部会话创建入口(orca 哲学:无大对话框,即时/小弹层)——TabStrip「+」= 即时建终端(cwd=选中节点);项目行「+」= 建 worktree 小弹层(基线 ref + 建完开终端 + 孤儿回滚)

**学习点:** ① orca 的主导流程是无对话框即时创建,cwd 隐式来自上下文(tab 属于 worktree)——我们等价移植为「cwd = 项目树选中节点」:选中 worktree 落 worktree,否则项目根,无选中则禁用并提示;② 孤儿回滚(M3 必办#2):worktreeCreate 成功而 sessionCreate 失败时,刚建的 worktree 无任何用户数据,best-effort `worktreeRemove(repo, name, true)` 直接回收——回滚是安全的,不需要确认;③ M4 起 tabstrip「+」要求先有选中项目,纯 shell(不绑 repo)会话入口随 NewSessionDialog 一起退役(spec v1.2 M4 章节既定裁剪)。

- [ ] **Step 1: shadcn 组件**

```bash
pnpm dlx shadcn@latest add dropdown-menu popover -y
```

(若 CLI 交互卡住:按 M3 T6 的手写退路,从 ui.shadcn.com 复制两组件源码,`cn` 引 `@/lib/utils`。)

- [ ] **Step 2: TabStrip「+」即时建终端**

`TabStrip.tsx` 追加(Props 不变——组件自取 store):

```tsx
import { Plus } from "lucide-react";
import {
  DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { sessionCreate } from "../../ipc/commands";
import type { SessionSnapshot } from "../../ipc/types";
```

组件内:

```tsx
  const selected = useProjects((s) => s.selected);
  const selectedLabel = selected
    ? selected.worktreeName
      ? selected.worktreeName.split("/").pop()
      : selected.repoPath.split("/").pop()
    : null;

  /** orca 哲学:无对话框即时创建;cwd = 项目树选中节点 */
  const newTerminal = useCallback(async () => {
    if (!selected) return;
    const opts = selected.worktreeName
      ? { repoPath: selected.repoPath, worktreeName: selected.worktreeName }
      : { repoPath: selected.repoPath };
    try {
      const created = await sessionCreate("shell", 80, 24, opts);
      const snap: SessionSnapshot = {
        sessionId: created.sessionId,
        state: created.state,
        startedAtMs: Date.now(),
        exitCode: null,
        pid: null,
        repoPath: selected.repoPath,
        worktreeName: selected.worktreeName ?? null,
      };
      useSessions.getState().add(snap);
      useTabs.getState().openTerminal(snap);
    } catch (e) {
      console.error("[tabs] 新建终端失败", e);
    }
  }, [selected]);
```

标签条右端(strip-tools 区)渲染:

```tsx
        <div className="ml-auto flex items-center pr-2">
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <button
                type="button"
                title="新建标签页"
                aria-label="新建标签页"
                className="grid size-6 place-items-center rounded text-muted-foreground hover:bg-accent hover:text-foreground"
              >
                <Plus className="size-4" />
              </button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              <DropdownMenuItem disabled={!selected} onClick={() => void newTerminal()}>
                新建终端{selectedLabel ? ` · ${selectedLabel}` : ""}
              </DropdownMenuItem>
              {!selected && (
                <p className="px-2 py-1 text-[11px] text-muted-foreground">
                  先在左侧打开并选中一个项目
                </p>
              )}
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
```

(import 补 `useCallback`、`useProjects`、`useSessions`、`useTabs` 中缺的。)

- [ ] **Step 3: NewWorktreePopover + 项目行挂「+」**

`NewWorktreePopover.tsx`:

```tsx
// 项目节点「+」:建 worktree 小弹层(基线 ref + 建完自动开终端)。
// 孤儿回滚(必办#2):worktree 建好而会话失败时,刚建 worktree 无用户数据,
// best-effort 连分支一起回收。
import { useEffect, useState } from "react";

import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Popover, PopoverContent, PopoverTrigger,
} from "@/components/ui/popover";

import {
  sessionCreate,
  worktreeCreate,
  worktreeRemove,
} from "../../ipc/commands";
import type { SessionSnapshot, WorktreeInfo } from "../../ipc/types";
import { useSessions } from "../../stores/sessionsStore";
import { useProjects } from "../../stores/projectStore";
import { useTabs } from "../../stores/tabStore";

interface Props {
  repoPath: string;
  /** 建完的后续(选中该 worktree 节点等)由组件内部完成 */
  onDone?: () => void;
}

export default function NewWorktreePopover({ repoPath, onDone }: Props) {
  const [open, setOpen] = useState(false);
  const [baseRef, setBaseRef] = useState("");
  const [autoTerminal, setAutoTerminal] = useState(true);
  const [submitting, setSubmitting] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    if (open) {
      setBaseRef("");
      setAutoTerminal(true);
      setErr(null);
    }
  }, [open]);

  const submit = useCallback(async () => {
    setSubmitting(true);
    setErr(null);
    let wt: WorktreeInfo | null = null;
    try {
      wt = await worktreeCreate(repoPath, "shell", baseRef.trim() || undefined);
      useProjects.getState().select({
        repoPath,
        worktreeName: wt.name,
      });
      if (autoTerminal) {
        const created = await sessionCreate("shell", 80, 24, {
          repoPath,
          worktreeName: wt.name,
        });
        const snap: SessionSnapshot = {
          sessionId: created.sessionId,
          state: created.state,
          startedAtMs: Date.now(),
          exitCode: null,
          pid: null,
          repoPath,
          worktreeName: wt.name,
        };
        useSessions.getState().add(snap);
        useTabs.getState().openTerminal(snap);
      }
      setOpen(false);
      onDone?.();
    } catch (e) {
      // 孤儿回滚:worktree 已建而会话失败——刚建无破坏性,直接回收
      if (wt) {
        await worktreeRemove(repoPath, wt.name, true).catch((rmErr) =>
          console.error("[worktree] 孤儿回滚失败,请手动清理", wt?.name, rmErr)
        );
      }
      setErr(`创建失败:${String(e)}`);
    } finally {
      setSubmitting(false);
    }
  }, [repoPath, baseRef, autoTerminal, onDone]);

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <span
          role="button"
          aria-label="新建 worktree"
          title="新建 worktree"
          className="hidden shrink-0 rounded p-0.5 text-muted-foreground/60 hover:bg-secondary hover:text-foreground group-hover/project:block"
        >
          <Plus className="size-3" />
        </span>
      </PopoverTrigger>
      <PopoverContent className="w-72" align="start">
        <div className="grid gap-3">
          <p className="text-xs font-semibold">新建 worktree</p>
          <div className="grid gap-1.5">
            <Label htmlFor="base-ref" className="text-xs">
              基线 ref(默认 HEAD)
            </Label>
            <Input
              id="base-ref"
              value={baseRef}
              placeholder="main"
              onChange={(e) => setBaseRef(e.target.value)}
            />
          </div>
          <div className="flex items-center gap-2">
            <Checkbox
              id="auto-term"
              checked={autoTerminal}
              onCheckedChange={(v) => setAutoTerminal(v === true)}
            />
            <Label htmlFor="auto-term" className="text-xs">
              建完自动开终端
            </Label>
          </div>
          {err && <p className="text-xs text-destructive">{err}</p>}
          <Button size="sm" onClick={() => void submit()} disabled={submitting}>
            {submitting ? "创建中…" : "创建"}
          </Button>
        </div>
      </PopoverContent>
    </Popover>
  );
}
```

(`import { useCallback } from "react"` 与 `Plus` from lucide-react 补进顶部。)

`ProjectTree.tsx` 的 `ProjectRow`:项目行内(「×」旁、`flex-1` 之后)挂

```tsx
        <NewWorktreePopover repoPath={project.path} />
```

(在 branch 徽标之后、移除「×」之前均可;确保 group-hover/project 与「×」一致显隐。)

- [ ] **Step 4: 退役 NewSessionDialog + 删 worktreeStore**

删除 `src/features/launch/NewSessionDialog.tsx` 与 `src/stores/worktreeStore.ts`;`src/features/launch/` 目录空则一并删。grep `NewSessionDialog|useWorktrees|worktreeStore` 确认零引用。

- [ ] **Step 5: 构建 + 验收 + 提交**

```bash
pnpm build && cargo test --workspace
pnpm tauri dev
# 验收:树选中项目 → TabStrip「+」即时开终端,pwd = 项目根;
# 选中 worktree 节点 → 「+」开终端 pwd = worktree;
# 项目行「+」→ 建默认 worktree + 自动开终端 pwd 落 .nx-worktrees/…;
# 断网/坏 ref 场景:worktree 创建失败报错;会话失败时 worktree 被回滚(外部 git worktree list 核对)
git add -A && git commit -m "feat(m4): 新建终端流(即时创建)+ NewWorktreePopover 孤儿回滚 + 退役 NewSessionDialog

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 8: GitPanel——右侧 Git 面板(状态/暂存/提交)

**Files:**
- Create: `src/features/gitpanel/GitPanel.tsx`
- Modify: `src/app/Workbench.tsx`(右栏占位换 GitPanel)

**Interfaces:**
- Consumes: Task 3 `gitStatus/gitStage/gitCommit`、Task 6 `useProjects.selected`、既有 `onWorktreeChanged`
- Produces: 右面板(Git 分段实现 + 「文件」分段占位)——分支行(↑ahead/↓behind)、变更列表(M/A/D/U/R 状态色 + rename 原路径)、截断横幅、暂存全部、提交框(空信息禁用)、手动刷新;刷新时机=选中项目变化/窗口聚焦/暂存提交后/worktree 变更事件

**学习点:** ① 刷新策略(spec §1.3)是拉模式:选中变化、聚焦、操作后三处触发 `gitStatus` 重拉,不做 watcher/轮询——orca 的调度器(admission tier/read lease)被 spec 明确判为 M4 不引入;② `GIT_OPTIONAL_LOCKS=0` 已在 Task 2 落在命令层,UI 聚焦刷新因此是安全的(不抢用户的 index.lock);③ 状态字母取「index 侧优先」:已暂存显示暂存后状态,未暂存显示工作区状态——与用户直觉一致(先看到将要提交什么)。

- [ ] **Step 1: GitPanel.tsx**

```tsx
// 右侧 Git 面板(原型 ctx/git):分支 + 变更列表 + 暂存全部 + 提交。
// 刷新:选中项目变化/窗口聚焦/操作后/refresh 钮(spec §1.3 拉模式,无轮询)。
import { useCallback, useEffect, useState } from "react";
import { ArrowDown, ArrowUp, GitBranch, RefreshCw } from "lucide-react";

import { Button } from "@/components/ui/button";
import { onWorktreeChanged } from "../../ipc/events";
import { gitCommit, gitStage, gitStatus } from "../../ipc/commands";
import type { GitStatus, GitStatusEntry } from "../../ipc/types";
import { useProjects } from "../../stores/projectStore";

const LETTER: Record<string, { ch: string; cls: string }> = {
  modified: { ch: "M", cls: "text-status-warn" },
  added: { ch: "A", cls: "text-status-ok" },
  deleted: { ch: "D", cls: "text-status-err" },
  renamed: { ch: "R", cls: "text-muted-foreground" },
  copied: { ch: "C", cls: "text-muted-foreground" },
  untracked: { ch: "U", cls: "text-muted-foreground" },
  unmerged: { ch: "!", cls: "text-status-err" },
};

/** 状态字母:index 侧优先(先看到将要提交什么) */
function sideOf(e: GitStatusEntry): string | null {
  return (e.index ?? e.worktree) ?? null;
}

export default function GitPanel() {
  const selected = useProjects((s) => s.selected);
  const repoPath = selected?.repoPath ?? null;
  const [status, setStatus] = useState<GitStatus | null>(null);
  const [message, setMessage] = useState("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    if (!repoPath) {
      setStatus(null);
      return;
    }
    try {
      setStatus(await gitStatus(repoPath));
      setErr(null);
    } catch (e) {
      setErr(`状态获取失败:${String(e)}`);
    }
  }, [repoPath]);

  // 选中变化 / 窗口聚焦 / worktree 变更 → 重拉
  useEffect(() => {
    void refresh();
  }, [refresh]);
  useEffect(() => {
    const onFocus = (): void => void refresh();
    window.addEventListener("focus", onFocus);
    const un = onWorktreeChanged((ev) => {
      if (ev.repoPath === repoPath) void refresh();
    });
    return () => {
      window.removeEventListener("focus", onFocus);
      void un.then((u) => u());
    };
  }, [refresh, repoPath]);

  const stageAll = useCallback(async () => {
    if (!repoPath) return;
    setBusy(true);
    try {
      await gitStage(repoPath);
      await refresh();
    } catch (e) {
      setErr(`暂存失败:${String(e)}`);
    } finally {
      setBusy(false);
    }
  }, [repoPath, refresh]);

  const commit = useCallback(async () => {
    if (!repoPath || !message.trim()) return;
    setBusy(true);
    try {
      await gitCommit(repoPath, message);
      setMessage("");
      await refresh();
    } catch (e) {
      setErr(`提交失败:${String(e)}`);
    } finally {
      setBusy(false);
    }
  }, [repoPath, message, refresh]);

  if (!repoPath) {
    return (
      <p className="p-4 text-center text-xs text-muted-foreground">
        在左侧选择一个项目后显示 Git 状态
      </p>
    );
  }

  return (
    <div className="flex-1 overflow-y-auto">
      {/* 分支行 */}
      <div className="border-b border-border px-3 py-2.5">
        <div className="flex items-center gap-2 text-xs">
          <GitBranch className="size-3.5 text-muted-foreground" />
          <span className="truncate font-mono">
            {status?.branch ?? "…"}
          </span>
          {status && (status.ahead > 0 || status.behind > 0) && (
            <span className="flex items-center gap-1 font-mono text-muted-foreground">
              {status.ahead > 0 && (
                <span className="flex items-center gap-0.5">
                  <ArrowUp className="size-3" />
                  {status.ahead}
                </span>
              )}
              {status.behind > 0 && (
                <span className="flex items-center gap-0.5">
                  <ArrowDown className="size-3" />
                  {status.behind}
                </span>
              )}
            </span>
          )}
          <span className="flex-1" />
          <button
            type="button"
            title="刷新"
            onClick={() => void refresh()}
            className="grid size-6 place-items-center rounded text-muted-foreground hover:bg-accent hover:text-foreground"
          >
            <RefreshCw className="size-3.5" />
          </button>
        </div>
      </div>
      {/* 变更列表 */}
      <div className="px-2 py-2">
        <div className="flex items-center justify-between px-1 pb-1">
          <span className="text-xs font-semibold text-muted-foreground">
            变更文件 · {status?.entries.length ?? 0}
          </span>
          {status && status.entries.length > 0 && (
            <button
              type="button"
              onClick={() => void stageAll()}
              disabled={busy}
              className="text-[11px] text-muted-foreground hover:text-foreground"
            >
              暂存全部
            </button>
          )}
        </div>
        {status?.truncated && (
          <p className="mb-1 rounded bg-status-warn/10 px-2 py-1 text-[11px] text-status-warn">
            变更过多,仅显示前 2000 项
          </p>
        )}
        {status && status.entries.length === 0 && (
          <p className="px-2 py-3 text-xs text-muted-foreground">
            工作区干净,没有未提交的变更。
          </p>
        )}
        {(status?.entries ?? []).map((e) => {
          const side = sideOf(e);
          const meta = side ? LETTER[side] : null;
          return (
            <div
              key={`${e.path}-${e.orig_path ?? ""}`}
              title={e.origPath ? `${e.origPath} → ${e.path}` : e.path}
              className="flex items-center gap-2 rounded px-2 py-1 font-mono text-[11px] hover:bg-accent"
            >
              <span className={`w-3 shrink-0 text-center font-semibold ${meta?.cls ?? ""}`}>
                {meta?.ch ?? "?"}
              </span>
              <span className="truncate">{e.path}</span>
            </div>
          );
        })}
      </div>
      {/* 提交区 */}
      <div className="border-t border-border p-3">
        <textarea
          value={message}
          onChange={(e) => setMessage(e.target.value)}
          placeholder="提交信息…"
          className="min-h-[60px] w-full resize-none rounded-md border border-input bg-card px-2.5 py-2 text-xs outline-none focus:border-ring focus:ring-2 focus:ring-ring/40"
        />
        <div className="mt-2 flex items-center justify-between">
          <span className="text-[11px] text-muted-foreground">
            提交只含已暂存内容
          </span>
          <Button
            size="sm"
            onClick={() => void commit()}
            disabled={busy || !message.trim() || (status?.entries.length ?? 0) === 0}
          >
            提交
          </Button>
        </div>
      </div>
      {err && (
        <p className="border-t border-border px-3 py-2 text-[11px] text-destructive">
          {err}
        </p>
      )}
    </div>
  );
}
```

- [ ] **Step 2: Workbench 右栏接线**

右栏占位换为:标题行分段(Git 激活 + 「文件」禁用占位)+ `<GitPanel />`:

```tsx
      <aside className="flex w-[296px] min-w-0 flex-col border-l border-border">
        <div className="flex min-h-[37px] items-center gap-1 border-b border-border px-2">
          <span className="rounded bg-card px-2 py-1 text-xs text-foreground">Git</span>
          <span
            title="文件(后续里程碑提供)"
            className="cursor-not-allowed rounded px-2 py-1 text-xs text-muted-foreground/50"
          >
            文件
          </span>
        </div>
        <GitPanel />
      </aside>
```

(`import GitPanel from "../features/gitpanel/GitPanel";`。)

- [ ] **Step 3: 构建 + 验收 + 提交**

```bash
pnpm build && cargo test --workspace
pnpm tauri dev
# 验收(spec 完成标准③):选中项目 → 改文件 → 面板出现 M/U 条目 → 暂存全部 →
# M 变位 → 填信息提交 → 列表清空;外部 git log -1 核对提交;rename 文件显示 R + 原路径 tooltip
git add -A && git commit -m "feat(m4): GitPanel——右侧面板状态/暂存/提交(porcelain v2 数据)

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 9: 全局 toast + 打磨包(M3 必办#4 残余)

**Files:**
- Create: `src/stores/toastStore.ts`
- Create: `src/components/Toaster.tsx`
- Modify: `src/App.tsx`(挂 Toaster;error footer 退役;确认框打磨)
- Modify: `src/features/tabs/TabStrip.tsx`、`src/features/project/NewWorktreePopover.tsx`、`src/features/project/OpenDirOverlay.tsx`、`src/features/gitpanel/GitPanel.tsx`(错误改 toast)
- Modify: `src/features/terminal/TerminalPane.tsx`(stopping 输入门禁 + 遮罩 token 化)

**Interfaces:**
- Consumes: Task 5-8 的错误路径
- Produces: `useToasts.push(msg, kind?: "info" | "error")`(2.8s 自动消失,至多同屏 4 条);M3 终审 Minor 修复:stopping 击键门禁、alsoRemoveWt 跨确认框残留、确认框不随后台退出自动收尾、遮罩色 token

- [ ] **Step 1: toastStore + Toaster**

`src/stores/toastStore.ts`:

```typescript
// 全局 toast(原型 toasts):错误/提示的统一出口;2.8s 自动消失,同屏至多 4 条。
import { create } from "zustand";

export interface ToastItem {
  id: number;
  msg: string;
  kind: "info" | "error";
}

let nextId = 1;

interface ToastState {
  toasts: ToastItem[];
  push: (msg: string, kind?: "info" | "error") => void;
}

export const useToasts = create<ToastState>((set) => ({
  toasts: [],
  push: (msg, kind = "info") => {
    const id = nextId++;
    set((st) => ({ toasts: [...st.toasts, { id, msg, kind }].slice(-4) }));
    setTimeout(() => {
      set((st) => ({ toasts: st.toasts.filter((t) => t.id !== id) }));
    }, 2800);
  },
}));

/** React 外便捷口(terminalManager/事件回调等非组件上下文) */
export function toast(msg: string, kind?: "info" | "error"): void {
  useToasts.getState().push(msg, kind);
}
```

`src/components/Toaster.tsx`:

```tsx
// toast 渲染层:右下角堆叠(原型 #toasts 位),不挡标签栏。
import { Check, Zap } from "lucide-react";

import { useToasts } from "../stores/toastStore";

export default function Toaster() {
  const toasts = useToasts((s) => s.toasts);
  if (toasts.length === 0) return null;
  return (
    <div
      aria-live="polite"
      className="fixed right-4 bottom-4 z-50 flex flex-col gap-2"
    >
      {toasts.map((t) => (
        <div
          key={t.id}
          className="flex min-w-[240px] max-w-[400px] items-center gap-2.5 rounded-lg border border-border bg-card px-3 py-2.5 text-xs shadow-lg"
        >
          {t.kind === "error" ? (
            <Zap className="size-4 shrink-0 text-destructive" />
          ) : (
            <Check className="size-4 shrink-0 text-status-ok" />
          )}
          <span className="min-w-0 break-all">{t.msg}</span>
        </div>
      ))}
    </div>
  );
}
```

- [ ] **Step 2: 各处错误改 toast + App 打磨**

- `App.tsx`:挂 `<Toaster />`(JSX 顶部 fragment 内);`setError(...)` 全部替换为 `toast(..., "error")`,删除 error footer 与 error state;`confirmStopAndClose` 的 worktree 清理失败等场景同改。
- **alsoRemoveWt 残留修复(M-5)**:`AlertDialog` 的 `onOpenChange` 里,关闭时 `setAlsoRemoveWt(true)` 重置。
- **确认框后台退出自动收尾(M 系列)**:App 加 effect——`confirmClose` 打开期间会话转终态时,直接走终态 dispose 收尾并关框:

```tsx
  // 确认框打开期间会话自行退出:不再需要确认,直接收尾(M3 终审 Minor)
  useEffect(() => {
    if (!confirmClose) return;
    const snap = useSessions.getState().sessions[confirmClose.id];
    if (snap && (snap.state === "exited" || snap.state === "failed")) {
      const id = confirmClose.id;
      setConfirmClose(null);
      closingRef.current.add(id);
      void sessionDispose(id)
        .catch(() => {})
        .finally(() => {
          closingRef.current.delete(id);
          disposeEntry(id);
          useSessions.getState().removeSession(id);
          useTabs.getState().closeTab(`term-${id}`);
        });
    }
  }, [confirmClose, sessions]);
```

(App 顶层补 `const sessions = useSessions((s) => s.sessions);` 供该 effect 依赖。)
- `TabStrip.tsx` 的 `newTerminal` catch、`NewWorktreePopover.tsx` 的 `setErr`(保留行内错的同时 `toast(..., "error")`——行内红字用于弹层内反馈,toast 用于全局可见)、`OpenDirOverlay.tsx` 的 onError 改 toast、`GitPanel.tsx` 的 `setErr` 同理。
- `TerminalPane.tsx`:输入门禁扩展到 stopping(M-4:stopping 击键触发后端错误刷 console):

```tsx
  // running 才放行输入;stopping 也关闸(击键只会收获后端错误,顺手消除 console 噪音);
  // undefined(store 未及)按活着处理。遮罩仍只对终态。
  const alive = state === "running" || state === undefined;
  useEffect(() => {
    setClosed(sessionId, !alive);
  }, [sessionId, alive]);
```

遮罩类已是 token 化(Task 4 完成),此处核对即可。

- [ ] **Step 3: 构建 + 提交**

```bash
pnpm build && cargo test --workspace
pnpm tauri dev   # 目检:toast 右下角出现/消失;stopping 击键无 console 报错;确认框打开时后台退出自动收尾
git add -A && git commit -m "feat(m4): 全局 toast + M3 必办#4 打磨(stopping 门禁/确认框收尾/选项重置)

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 10: 测试补遗(必办#6)+ 端到端验收 + PR

**Files:**
- Modify: `src-tauri/crates/nexus-core/tests/worktree_manager.rs`(base_ref 用例)
- Modify: `src-tauri/crates/nexus-core/src/agent/manager.rs`(list 同毫秒次级排序键)

**Interfaces:**
- Consumes: 全部前序任务
- Produces: spec M4 完成标准①-⑦ 的验证记录 + PR

- [ ] **Step 1: base_ref 覆盖 + 同毫秒排序键**

`tests/worktree_manager.rs` 追加(复用既有 init helper;repo 需两笔提交):

```rust
/// 必办#6(T8-③):base_ref 生效——从首笔提交建 worktree,HEAD 应停在首笔
#[tokio::test]
async fn create_from_explicit_base_ref_checks_out_that_commit() {
    let dir = tempfile::tempdir().unwrap();
    init_repo_at(dir.path());
    let first = String::from_utf8(
        std::process::Command::new("git")
            .arg("-C").arg(dir.path())
            .args(["rev-parse", "HEAD"])
            .output().unwrap().stdout,
    ).unwrap().trim().to_string();
    // 第二笔
    std::fs::write(dir.path().join("second.txt"), "2\n").unwrap();
    let run = |args: &[&str]| std::process::Command::new("git")
        .arg("-C").arg(dir.path()).args(args).status().unwrap();
    assert!(run(&["add", "."]).success());
    assert!(run(&["commit", "-qm", "second"]).success());

    let (mgr, _events) = manager_with_events();
    let info = mgr.create(dir.path(), "shell", Some(&first)).await.unwrap();
    let wt_head = String::from_utf8(
        std::process::Command::new("git")
            .arg("-C").arg(&info.path)
            .args(["rev-parse", "HEAD"])
            .output().unwrap().stdout,
    ).unwrap().trim().to_string();
    assert_eq!(wt_head, first, "worktree 应停在 base_ref 指定提交");
}
```

`manager.rs` 的 `list()` 排序改为带次级键(同毫秒稳定,杜绝同毫秒建两个会话时刷新后 tab 序抖动):

```rust
        // 按 startedAtMs 升序;同毫秒按 session_id 字符串稳定排序
        // (必办#6 T4-③:同毫秒创建的会话刷新后序不抖动)
        snaps.sort_by(|a, b| {
            (a.started_at_ms, a.session_id.to_string())
                .cmp(&(b.started_at_ms, b.session_id.to_string()))
        });
```

(既有排序测试仍绿——次级键不改变不同毫秒的序;精确同毫秒用例不做,需时钟注入,收益低——计划内裁剪,记入完成记录。)

- [ ] **Step 2: 全量门槛**

```bash
cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace && pnpm build
```

- [ ] **Step 3: 端到端验收(spec M4 完成标准①-⑦)**

从 worktree 目录跑 `pnpm tauri dev`,逐项过:

1. ① 打开目录 → 项目入树 → 重启应用项目还在;项目行「+」建 worktree → 终端自动开、`pwd` 落 `.nx-worktrees/nexus/shell-…`
2. ② tabstrip「+」即时开终端(选中项目落项目根/选中 worktree 落 worktree);两个项目各开终端并行输出互不串流
3. ③ Git 面板:改文件 → 列表出现 → 暂存 → 提交 → 列表清空;`git log -1 --stat` 核对;新仓库首次提交不推送(面板无推送钮)
4. ④ 刷新(Ctrl+R):终端内容、项目树、tab 与激活态全部重建
5. ⑤ 亮色全界面一致(侧栏/标签/面板/弹层无暗色残留);断网启动字体正常(fontsource 本地)
6. ⑥ 自动化已绿(Step 2);三平台 CI 在 PR 上验证
7. ⑦ M2/M3 基线:强杀 3 秒内 Failed、慢消费背压不丢字、`cat` 大文件不卡、关 tab 即删 + 可选删 worktree、退出遮罩 + 输入门禁

- [ ] **Step 4: 推送 + PR**

```bash
git push -u origin m4-prototype-ui-shell   # 带代理:git -c http.proxy=http://127.0.0.1:7890 push -u origin m4-prototype-ui-shell
gh pr create --title "feat(m4): 原型 UI 骨架——四栏 workbench/项目注册表/Git 面板/亮色主题" --body-file <(echo "…PR 描述:目标/变更清单/验收记录/截图…")
gh pr checks --watch
```

(PR body 写入文件后用 `--body-file`,M3 实测内联 body 会被权限拒;CI 四项绿后**由用户本人合并**。)

---

## Self-Review 记录

- **Spec 覆盖**:registry 域→T1(§1.3);gitx status/stage/commit + porcelain v2 →T2;IPC 六命令+FromStr 加固(必办#1)→T3;亮色主题/字体/minWidth→T4(§1.5);tabStore 单一模型/四栏骨架/挂载闩锁→T5;项目树/打开目录/多项目持久化→T6(§1.5 布局);新建终端流/孤儿回滚(必办#2)/exclude(必办#3,已在 T1)→T7;Git 面板→T8(§1.3 刷新策略);toast+必办#4 残余→T9;必办#6 轻量+完成标准①-⑦→T10。✅
- **有意裁剪(非遗漏)**:worktree_remove 不加 FromStr 校验(按 name 精确匹配列表条目、无路径拼接,外建 worktree 名须可删——T3 注明);同毫秒排序精确用例不做(需时钟注入);M3 必办#5 可观测性/完整 Windows 冒烟(#7)顺延 M5(与 spec M4 章节吸收清单一致);「文件」分段/⌘K/设置 pane/任务看板为占位或禁用(spec 既定)。
- **类型一致性**:`GitStatus{branch,ahead,behind,entries,truncated}` T2 定义、T3 TS 同构;`ProjectEntry{id,name,path,addedAtMs}` T1/T3/TS 三处一致;`useProjects.selected{repoPath,worktreeName?}` T6 定义、T7/T8 消费;tab id 恒 `term-<sessionId>`(T5 tabStore/T7/T9 App 收尾三处同源);`Tab.repoPath` 镜像快照字段(spec 写 projectId?,计划统一为 repoPath——与 SessionSnapshot 字段名一致,树匹配零转换)。✅
- **已知风险**:① porcelain v2 `-z` 头记录 `\n`/变更记录 NUL 的混合分隔是文档行为,快照单测 + 真实 git 集成双保险(T2);② T5-T9 是连续前端重构,每步门槛 `pnpm build` + 终端基线目检,回退风险集中在 T5(布局重写)——挂载闩锁语义与 M3 完全一致是防线;③ fontsource 包名漂移:给了手写 @font-face 退路(T4);④ shadcn dropdown-menu/popover CLI 参数随版本漂移:同 M3 退路(T7)。✅

---

## 完成记录(2026-09-13,PR #5 已合并)

全部 10 任务完成,每任务独立审查全 Approved;spec M4 完成标准①-⑦ 全过(⑥自动化:cargo test --workspace **87/87** 三平台绿 + clippy -D warnings 零警告 + pnpm build 绿;①-⑤⑦人工验收通过)。终审(fable):唯一 Important **I-1**(session_create 的 `(Some(repo), None)` 分支走 M2 旧语义,TabStrip「+」选项目根时 cwd 不落项目根——T3 契约×T7 调用的跨任务缝隙,单任务审查盲区,终审网住)已修(8c80463,+专项测试 `session_with_repo_path_only_lands_at_project_root`,范围化复审全 ADDRESSED)。用户人工验收三轮反馈全部闭环:落点语义(I-1)/UI 对齐打磨(3f461bf+68ee0f8,原型 CSS 为权威的纯样式波)/项目工作区层级(用户规格:添加目录成组、git 子目录各为项目行带 branch 徽标——d40c6f9,registry 加 workspace 字段+legacy 回填)。T7 实现者两度流断(代码全在盘上),按 SDD 派接手者收尾(核对 brief→重跑门槛→提交,代码零改动)。CI ubuntu 首轮失败 = `attach_seam_never_duplicates` 10s 超时(M2 既有测试,M4 未触该链路,偶发抖动,rerun --failed 即绿)。终态 15 提交,PR #5 CI 四项全绿,用户本人合并。

### M5 必办(延后 Minor triage 全表,按域分组;SDD ledger 已随 worktree 清理,此处为唯一存档)

**Git 面板(M5 会话 UI 更依赖,优先)**:
1. repo 快速切换竞态(T8-①):旧 repo 在途 gitStatus 晚归覆盖新数据、新失败时旧 status 残留——无 epoch/abort 闸
2. 右栏折叠卸载 GitPanel 丢提交草稿(终审;Workbench 条件渲染 aside 的既定代价)
3. Git 面板忽略 worktree 选中——始终显示项目主检出状态,语义待决策(终审)
4. busy 由 stage/commit 共享,无 per-action loading(T8-②)

**gitx 后端**:
5. porcelain 'T'(typechange)码未映射,按无变化处理(终审)
6. 非 UTF-8 文件名 lossy 失真后按失真名 stage 失败(T2-⑤,&str 管线固有)
7. git_* 三命令复用 ensure_worktree_ready 要求 git≥2.20,porcelain v2 实需 ≥2.11——2.11~2.19 机器 Git 面板被拒且文案误导(T3-①;可单设轻 gate)
8. 旧式 \n 头形态下文件名内嵌 \n 可伪造头行(理论级,T2-②;加固:首段只认 # 行,遇变更前缀即停)
9. parse_xy_path trim_end 截尾空格文件名、u 分支不 trim——不一致(T2-③);stage Some(&[]) no-op 无测试(T2-④);parse_status_porcelain_v2 可收窄 pub(crate)(T2-①)
10. Windows 上路径 split("/") 不分段(终审;SessionRow/NewWorktreePopover 的名字截短显示)
11. commit -m 前置 dash 选项解析(理论级,终审;message 以 `-` 开头会被 git 当选项)

**registry**:
12. add 扫描每子目录跑完整 validate_repo(3 git 子进程×N)+ 前端 refresh 重复探测——可换轻量 rev-parse(工作区层级轮)
13. remove() save 失败内存已删/磁盘未删,下次启动复活(T1-②;与 config store 哲学一致,保留决策)
14. list() 无防御性排序,手改 projects.json 乱序(T1-①;IPC 层一行 sort)
15. "目录不存在"文案以偏概全(canonicalize 失败还可能权限/环)(工作区层级轮);add 内阻塞 fs I/O(与既有风格一致)(同轮);同毫秒次级键后端字节序 vs 前端 UTF-16 序(非 ASCII 目录名极端角落,两端各自确定)(同轮)
16. 迁移备忘:落盘外层键实为 `schema_version`(非 fixture 的 `schemaVersion`),load 忽略外层键故惰性——未来写迁移勿按 fixture 键名假设(T1-③)

**前端**:
17. App 根订阅 sessions 全表(终审;PTY 输出不进 store,触发=低频状态转换,重渲染面无实际风险)
18. Toaster 与 Dialog 同 z-50 且 DOM 序靠前——模态开着时新 toast 画在遮罩下(T9-①)
19. OpenDirOverlay Enter 路径无在途守卫(T6-①,后端幂等无害)
20. NewWorktreePopover 回滚后 selected 短暂指向已回收 worktree(T7-②);「+」点击冒泡选中项目根(T7-③,良性)
21. 树视觉:根会话渲染在 worktree 子行后/选中根会话双高亮(T6-④);hover 触发器键盘不可达(原型欠账,T7-④);会话绑定 worktree 删后行消失(原型语义,T6-⑤)
22. useWorkbenchLayout 在 setState updater 内写 localStorage(StrictMode 双写,幂等无害,T5-③);removeSession 对不存在 key 仍建新引用(无效重渲染一次,T9-③);自动收尾 sessionDispose 失败静默 vs handleClose 的 console 不一致(T9-②);lib.rs app_config_dir() 解析两次(T3-③)

**测试/CI**:
23. `attach_seam_never_duplicates` ubuntu 负载下可再抖——候选加固:放宽 deadline 或查 Linux PTY 时序(M2 既有测试,本轮 rerun 绿)
24. T10-② 新用例 run 闭包失败不带 stderr(测试代码可接受)

### 平台/生态知识库(本轮新踩,执行 M5 前必读)

- **git 2.50.1 porcelain v2 `-z` 头记录也是 NUL 结尾**(计划前提"\n 结尾"错误,实现者实测发现):解析器两态兼容(首段 lines() 认 `# ` 头 + pending 循环也认),\n 头(旧 git)与 NUL 头(现代 git)两种形态各有专项内联测试钉住;变更记录行首 1/2/u/? 不可能 `#` 开头,无误判路径。
- **porcelain v2 rename 真实形态**:`2 R.  ... R100 <path>\0<origPath>\0`——XY 是两字符(第二字符为 submodule 状态位 `.`),计划快照的 `2 R  ` 单字符形态不存在(会使 parse_xy_path 丢条目);校准时断言不变。
- **React 19.2.8 reconcileChildrenArray 源码级结论**:Workbench 条件渲染 aside(`{sideOpen && <aside/>}`)不卸载 main 内组件——main 恒 index 2,折叠走 mapRemainingChildren 兜底复用、展开走 index-skip 不删旧 fiber,全开合序列 TerminalPane 不卸载(挂载闩锁红线保持)。**M5+ 若在 main 前插不同类型常驻栏,该结论需重新验证**(T5 轮"false 占槽"表述不精确,实际是槽位 index 记账)。
- **fontsource 本地字体**:`@fontsource-variable/geist` + `@fontsource/jetbrains-mono`(400/500 两 css)import 即打包 woff2,离线可用;等价于 orca 手放 assets 自写 @font-face 的包管理形态。
- **CI flake 判定法**:单平台超时 + 同测其他平台通过 + 分支未触该链路 → rerun --failed 验证即绿,记备忘不阻塞合并;不要为 flake 改实现代码。
- **ExitWorktree remove 对已合并分支的误拒**:工具与本地 main ref 比较(fetch 前过时)——先带代理 fetch,再 `git branch -r --contains <分支末端>` 确认含 origin/main 后 discard_changes 安全。
- **worktree 隔离会话的 git 边界**:worktree-isolated session 里 `git -C <主检出>` 被拒(harness 保护)——主检出的还原/pull 等操作必须在 ExitWorktree 之后做;ledger 等 git-ignored 工件须在 remove 前拷出。
- **TaskOutput 轮询超时会 dump 子代理 transcript JSONL**(噪音大污染上下文)——等子代理任务尽量一次长等,不短间隔轮询。

### 裁定索引(全程预检 2 条 + 任务/验收轮裁定,原文已随 SDD 工作区清理,此处为关键存档)

- 预检:project_remove 未命中用 `InvalidInput("项目不存在: …")` 非 SessionNotFound(文案误导);计划 Self-Review 声明的裁剪(worktree_remove 不加 FromStr 等)获 spec 背书照准
- T1:Task 3 接口提醒 ProjectId 无 From<Uuid>、错误通道 InvalidInput 定形
- T2:**git -z 头 NUL 结尾**(计划前提错误,实测校准,解析器两态兼容+双形态测试);**rename 快照自相矛盾**(按实测 `2 R.` 形态校准,断言不变,审查者独立复现原快照必挂)
- T4:brief 内部矛盾(内联字体栈 vs DEFAULT_FONT_STACK 常量,noUnusedLocals 二选一)——取常量复用,行为零差
- T5:**Workbench asides 取条件渲染**(brief 代码块无条件渲染,但其自注与 T6 契约指向条件渲染;不实现则折叠钮是死按钮)——闩锁安全性经 React 19.2.8 源码逐行追踪独立证实
- T6:ProjectSide 补 flex-1/ProjectRow 增 byWorktree prop(计划括号既定补全,照准)
- T7:前任实现者两度流断——按 SDD 派接手者收尾(核对 brief→重跑门槛→提交→补报告),不算修复轮;接手者未申报的 tab 容器 div role=tab 改动以 diff 为准绳发现并照准(HTML 合法性,消 T5 的 role 嵌套 a11y 疵)
- T8:条目 key `e.orig_path`→`e.origPath`(brief 笔误,契约字段 origPath,strict 下必编译失败)
- 打磨波:阴影透明度取原型 light 值(.22/.18)非清单笔误的 dark 35%;新增 --border-strong/--faint 两枚原型 light 令牌
- 工作区层级轮:重挂工作区保留 addedAtMs(组按最小时间戳排,重挂排旧位——规格字面推论,行为确定无抖动,用户验收通过)




