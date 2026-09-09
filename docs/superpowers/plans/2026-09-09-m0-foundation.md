# ItsukaNexus M0（地基）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 跑通 Tauri 2 + React-TS 应用骨架，交付自定义 `app_info` 命令（含单元测试），达到 clippy 零警告，作为后续所有里程碑的地基。

**Architecture:** 用 create-tauri-app 生成标准骨架（React-TS 模板），在 Rust 侧建立第一个领域模块 `app.rs`（纯逻辑，可测试）+ `lib.rs`（薄 IPC 封装），前端建立 `src/ipc/` 封装层（types + commands），M1/M2 直接在此结构上扩展。

**Tech Stack:** Tauri 2、React 19 + TypeScript + Vite、pnpm、cargo test/clippy/rustfmt。

**Spec:** `docs/superpowers/specs/2026-09-09-itsukanexus-mvp-design.md`（M0 章节 + §1.4 IPC 命令清单）

## Global Constraints

- 仓库：`D:\CodeSpace\ItsukaNexus`，git 主分支 `main`，每任务一次提交，提交信息结尾加 `Co-Authored-By: Claude Code <noreply@anthropic.com>`
- 命名统一：cargo 包名 `itsukanexus`、lib 名 `itsukanexus_lib`、Tauri productName `ItsukaNexus`、identifier `dev.itsukanexus.app`
- 包管理器只用 **pnpm**（禁 npm/yarn install）
- 环境注意：Node 装在自定义路径且不在 bash 会话 PATH 中。任何 bash 调用中 node/npm/pnpm 报"command not found"时，先执行：
  `export PATH="/d/Develop/nodejs:/c/Users/ZYWUD/AppData/Roaming/npm:$PATH"`
- 质量门槛（每任务收尾必须过）：`cargo fmt` 已应用、`cargo clippy --all-targets -- -D warnings` 零警告、`cargo test` 全绿
- 环境现状（2026-09-09 实测）：Rust 1.98.1 stable-msvc ✅；Node v24.20.0 @ `D:\Develop\nodejs` ✅；pnpm 12.3.4 @ `%APPDATA%\npm` ✅（本计划 Task 1 已完成环境安装）；WebView2 在 Win11 默认内置（若 `tauri dev` 白屏再排查）

---

### Task 1: 环境就绪（✅ 已完成，保留作记录）

**Files:**
- Create: 无（纯环境变更，无代码提交）

**Interfaces:**
- Consumes: —
- Produces: node v24.20.0、pnpm 12.3.4 可用（Task 2 依赖）

- [x] **Step 1: 验证 Node（自定义路径 `D:\Develop\nodejs`，不在 bash PATH）**

```bash
"/d/Develop/nodejs/node" --version
```

Result: `v24.20.0` ✅（用户已预装）

- [x] **Step 2: 安装 pnpm（全局，落入 %APPDATA%\npm）**

```bash
export PATH="/d/Develop/nodejs:$PATH" && npm install -g pnpm
```

Result: pnpm `12.3.4` ✅（2026-09-09 已执行）

---

### Task 2: 生成 Tauri 脚手架 + 统一命名 + 首次提交

**Files:**
- Create: `package.json`、`vite.config.ts`、`tsconfig.json`、`index.html`、`src/main.tsx`、`src/App.tsx`、`src-tauri/Cargo.toml`、`src-tauri/tauri.conf.json`、`src-tauri/capabilities/default.json`、`src-tauri/src/main.rs`、`src-tauri/src/lib.rs`、图标资源等（模板全套）
- Modify: `.gitignore`（与模板自带的合并去重）
- Modify: `src-tauri/Cargo.toml`（包名/lib 名统一为 itsukanexus）

**Interfaces:**
- Consumes: Task 1 的 node/pnpm
- Produces: 可 `pnpm tauri dev` 运行的骨架；`src-tauri/src/lib.rs` 中的 `run()` 入口（Task 3 修改）；模板自带 `greet` 命令（Task 3 删除）

- [ ] **Step 1: 生成到临时子目录（根目录非空，不能直接生成）**

```bash
cd "D:/CodeSpace/ItsukaNexus" && export PATH="/d/Develop/nodejs:/c/Users/ZYWUD/AppData/Roaming/npm:$PATH" && pnpm create tauri-app itsukanexus-scaffold --identifier dev.itsukanexus.app --template react-ts --manager pnpm --yes
```

Expected: 生成 `itsukanexus-scaffold/` 目录，含 `src/`、`src-tauri/`、`package.json` 等。若提示交互确认，全部接受默认。

- [ ] **Step 2: 内容移到仓库根（含隐藏文件），删除临时目录**

```bash
cd "D:/CodeSpace/ItsukaNexus" && mv itsukanexus-scaffold/* itsukanexus-scaffold/.vscode . 2>/dev/null; mv itsukanexus-scaffold/.gitignore ./scaffold-gitignore 2>/dev/null; rmdir itsukanexus-scaffold
```

- [ ] **Step 3: 合并两个 .gitignore（保留并集，去重）**

把 `scaffold-gitignore` 中我们根 `.gitignore` 没有的行（如 `*.local`、`src-tauri/build/` 等模板特有条目）并入 `.gitignore`，然后 `rm scaffold-gitignore`。

- [ ] **Step 4: 统一命名（Cargo.toml + tauri.conf.json）**

`src-tauri/Cargo.toml`：
- `[package] name = "itsukanexus-scaffold"` → `name = "itsukanexus"`
- `[lib] name = "itsukanexus_scaffold_lib"` → `name = "itsukanexus_lib"`
- `src-tauri/src/main.rs` 内 `itsukanexus_scaffold_lib::run()` → `itsukanexus_lib::run()`

`src-tauri/tauri.conf.json`：确认 `productName` 为 `"ItsukaNexus"`、`identifier` 为 `"dev.itsukanexus.app"`、`frontendDist`/`devUrl` 保持模板默认。

- [ ] **Step 5: 安装依赖**

```bash
cd "D:/CodeSpace/ItsukaNexus" && export PATH="/d/Develop/nodejs:/c/Users/ZYWUD/AppData/Roaming/npm:$PATH" && pnpm install
```

Expected: `Done in ...s`，生成 `node_modules/` 与 `pnpm-lock.yaml`。

- [ ] **Step 6: Rust 侧编译验证（不启动窗口）**

```bash
cd "D:/CodeSpace/ItsukaNexus/src-tauri" && cargo check
```

Expected: `Finished`，零 error。（首次编译下载全部依赖，3-10 分钟属正常。）

- [ ] **Step 7: 提交**

```bash
cd "D:/CodeSpace/ItsukaNexus" && git add -A && git commit -m "chore: Tauri 2 + React-TS 脚手架（create-tauri-app）

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

- [ ] **Step 8 (可选，需管理员 PowerShell，失败则跳过并记入 README 待办): 将 src-tauri/target 加入 Windows Defender 排除**

避免后续频繁编译被实时扫描拖慢/误报（设计文档风险 #4）。

---

### Task 3: `app_info` 命令（Rust 侧，TDD）

**Files:**
- Create: `src-tauri/src/app.rs`（AppInfo 结构 + 纯逻辑函数，未来演进为 commands/app.rs）
- Create: `src-tauri/tests/app_info.rs`（集成测试）
- Modify: `src-tauri/src/lib.rs`（删除 `greet`，注册 `app_info` 命令）
- Modify: `src-tauri/src/lib.rs`（`mod app;` 声明）

**Interfaces:**
- Consumes: 模板 `lib.rs` 的 `run()` 骨架
- Produces: `itsukanexus_lib::app::app_info() -> AppInfo`；`AppInfo { name: String, version: String, platform: String }`（serde 序列化字段名即此，Task 4 的 TS 类型与此对齐）；Tauri 命令 `app_info`

**Rust 学习点（写给执行者）：** `env!("CARGO_PKG_*")` 编译期宏 vs `std::env` 运行期；`#[derive(Serialize)]` 如何让结构体跨 IPC；集成测试（tests/ 目录）与单元测试（#[cfg(test)]）的边界——lib crate 才能被集成测试导入。

- [ ] **Step 1: 写失败测试**

`src-tauri/tests/app_info.rs`：

```rust
use itsukanexus_lib::app::{app_info, AppInfo};

#[test]
fn app_info_returns_name_version_platform() {
    let info: AppInfo = app_info();
    assert_eq!(info.name, "itsukanexus");
    assert!(!info.version.is_empty());
    assert_eq!(info.platform, "windows"); // 当前开发平台
}
```

- [ ] **Step 2: 运行验证失败（红）**

```bash
cd "D:/CodeSpace/ItsukaNexus/src-tauri" && cargo test --test app_info
```

Expected: 编译错误 `unresolved module app`（红=测试有效）。

- [ ] **Step 3: 最小实现**

`src-tauri/src/app.rs`：

```rust
use serde::Serialize;

/// 应用元信息（M0 第一个跨 IPC 结构体）。
/// name/version 来自编译期 env 宏（Cargo.toml），platform 来自目标 OS 常量。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub name: String,
    pub version: String,
    pub platform: String,
}

pub fn app_info() -> AppInfo {
    AppInfo {
        name: env!("CARGO_PKG_NAME").to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        platform: std::env::consts::OS.to_string(),
    }
}
```

`src-tauri/src/lib.rs` 改造：顶部加 `mod app;`；删除 `greet` 命令及其 `GreetArgs` 结构；新增命令薄封装：

```rust
#[tauri::command]
fn app_info() -> app::AppInfo {
    app::app_info()
}
```

`generate_handler!` 清单里把 `greet` 换成 `app_info`。

- [ ] **Step 4: 运行验证通过（绿）**

```bash
cd "D:/CodeSpace/ItsukaNexus/src-tauri" && cargo test
```

Expected: `app_info_returns_name_version_platform ... ok`，全部通过。

- [ ] **Step 5: fmt + clippy 门槛**

```bash
cd "D:/CodeSpace/ItsukaNexus/src-tauri" && cargo fmt && cargo clippy --all-targets -- -D warnings
```

Expected: clippy 零输出零警告。（若报 lib.rs 中未使用的 import，删除之。）

- [ ] **Step 6: 提交**

```bash
cd "D:/CodeSpace/ItsukaNexus" && git add src-tauri/src src-tauri/tests && git commit -m "feat(m0): app_info 命令与 AppInfo 结构（含集成测试）

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 4: 前端接入 app_info（IPC 封装层 + UI）

**Files:**
- Create: `src/ipc/types.ts`（与 Rust 侧对齐的 TS 类型，唯一事实来源的镜像）
- Create: `src/ipc/commands.ts`（所有 invoke 的类型安全封装，唯一入口）
- Modify: `src/App.tsx`（重写为应用信息卡片，删除模板 greet 演示）

**Interfaces:**
- Consumes: Task 3 的 Tauri 命令 `app_info`（返回 camelCase JSON：`{ name, version, platform }`）
- Produces: `src/ipc/commands.ts` 导出 `getAppInfo(): Promise<AppInfo>`（M1 的 session_* 封装将并列加入此文件）

说明：模板未配前端测试框架，M0 不引入（YAGNI——本任务逻辑是纯 IPC 调用与展示，由 Task 5 手动 E2E 覆盖；vitest 留到有真实前端逻辑的 M2 再议）。

- [ ] **Step 1: 建 IPC 封装层**

`src/ipc/types.ts`：

```typescript
/** 与 src-tauri/src/app.rs 的 AppInfo 对齐（serde camelCase） */
export interface AppInfo {
  name: string;
  version: string;
  platform: string;
}
```

`src/ipc/commands.ts`：

```typescript
import { invoke } from "@tauri-apps/api/core";
import type { AppInfo } from "./types";

/** 所有 Tauri invoke 的类型安全封装——前端唯一入口 */
export function getAppInfo(): Promise<AppInfo> {
  return invoke<AppInfo>("app_info");
}
```

- [ ] **Step 2: 重写 App.tsx**

用下方实现整体替换模板演示代码（greet 输入框等），样式用内联（模板 css 文件保留不动）：

```tsx
import { useState } from "react";
import { getAppInfo } from "./ipc/commands";
import type { AppInfo } from "./ipc/types";

function App() {
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function handleLoad() {
    try {
      setInfo(await getAppInfo());
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }

  return (
    <main style={{ display: "grid", placeItems: "center", minHeight: "100vh", fontFamily: "system-ui" }}>
      <div style={{ textAlign: "center" }}>
        <h1>ItsukaNexus</h1>
        <p>M0 · Agent Development Environment</p>
        <button onClick={handleLoad}>获取应用信息</button>
        {info && (
          <ul style={{ listStyle: "none", padding: 0 }}>
            <li>name: {info.name}</li>
            <li>version: {info.version}</li>
            <li>platform: {info.platform}</li>
          </ul>
        )}
        {error && <p style={{ color: "red" }}>错误: {error}</p>}
      </div>
    </main>
  );
}

export default App;
```

- [ ] **Step 3: 类型检查通过**

```bash
cd "D:/CodeSpace/ItsukaNexus" && export PATH="/d/Develop/nodejs:/c/Users/ZYWUD/AppData/Roaming/npm:$PATH" && pnpm build
```

Expected: `tsc && vite build` 全绿，产出 `dist/`。（App.tsx 中对模板 logo svg 的引用已随重写移除；若 tsc 报未使用文件不算错误。）

- [ ] **Step 4: 提交**

```bash
cd "D:/CodeSpace/ItsukaNexus" && git add src/ && git commit -m "feat(m0): 前端 IPC 封装层与应用信息卡片

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 5: 端到端验证（M0 完成标准逐项打勾）

**Files:**
- Modify: 无（验证任务；若验证暴露问题，修复后回归本清单）

**Interfaces:**
- Consumes: Task 1-4 全部产出
- Produces: M0 完成标准四项全绿的证据（对应设计文档 M0 章节）

- [ ] **Step 1: 启动开发模式（标准①）**

```bash
cd "D:/CodeSpace/ItsukaNexus" && export PATH="/d/Develop/nodejs:/c/Users/ZYWUD/AppData/Roaming/npm:$PATH" && pnpm tauri dev
```

Expected: 首次 Rust 全量编译 3-10 分钟属正常；随后弹出窗口。点击"获取应用信息"按钮，显示 `name: itsukanexus`、`version: 0.1.0`、`platform: windows`。

- [ ] **Step 2: 验证 Rust 热重载（标准③）**

保持 `tauri dev` 运行，修改 `src-tauri/src/app.rs` 中 `version` 一行：

```rust
version: format!("{}-dev", env!("CARGO_PKG_VERSION")),
```

Expected: 数十秒内应用自动重编译并重启，按钮点击显示 `version: 0.1.0-dev`。验证后**改回原样**（`version: env!("CARGO_PKG_VERSION").to_string(),`）。

- [ ] **Step 3: 回归测试与 lint（标准②④）**

```bash
cd "D:/CodeSpace/ItsukaNexus/src-tauri" && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check
```

Expected: 全部通过、零警告。

- [ ] **Step 4: 提交（如 Step 2 有残留变更）并收尾**

```bash
cd "D:/CodeSpace/ItsukaNexus" && git status --short && git add -A && git commit -m "chore(m0): 端到端验证通过（M0 完成）

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

（`git status` 为空则跳过提交，直接汇报。）

---

## Self-Review 记录

- **Spec 覆盖**：设计文档 M0 完成标准 ①（tauri dev + 按钮显示版本）→ Task 5 Step 1；②（cargo test）→ Task 3 Step 4 / Task 5 Step 3；③（热重载）→ Task 5 Step 2；④（clippy 零警告）→ Task 3 Step 5 / Task 5 Step 3。IPC 清单中 M0 行（`app_info` → `{ name, version, platform }`）→ Task 3。✅
- **占位符扫描**：无 TBD/TODO；所有代码步骤含完整代码。✅
- **类型一致性**：`AppInfo` 字段 name/version/platform 在 Rust（Task 3，serde camelCase）与 TS（Task 4 types.ts）一致；`app_info` 命令名在 lib.rs 注册与 commands.ts invoke 一致。✅
