# ItsukaNexus

面向开发者的桌面 Agent Development Environment（ADE）：聚合多个编程 agent，支持并行编排与 git worktree 隔离，让多个 agent 在同一仓库的不同工作树上互不干扰地协作。技术栈为 Tauri 2 + React + TypeScript（后端 Rust）。本项目同时是一个 Rust 学习项目，代码会附带原理解释。

## 开发指南

```bash
# 启动桌面应用（开发模式，热重载）
pnpm tauri dev

# Rust 测试（在 src-tauri 目录下执行）
cd src-tauri && cargo test

# Rust 静态检查（在 src-tauri 目录下执行，警告视为错误）
cd src-tauri && cargo clippy --all-targets -- -D warnings
```

## M5 打包前置清单

以下事项在最终打包发布（M5）前需逐一确认，提前记录防止遗忘：

- [ ] 收紧 `src-tauri/tauri.conf.json` 的 CSP（当前为 `null`，开发期宽松配置）
- [ ] 版本号单一来源：`tauri.conf.json` 可省略 `version`，自动回落到 `Cargo.toml`
- [ ] Windows 发布前完成代码签名（否则安装时会触发 SmartScreen 警告）
- [ ] 开发期建议将 `src-tauri/target` 加入 Windows Defender 排除目录（频繁编译可显著提速）

## 里程碑状态

- **M0** — 已完成（环境就绪、脚手架、app_info TDD、前端接入、E2E 验证）
- **M1 - M5** — 见 `docs/superpowers/specs/` 下的设计文档
