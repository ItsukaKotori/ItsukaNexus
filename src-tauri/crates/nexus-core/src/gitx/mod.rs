// gitx:git 集成域。选型(spec §3):CLI 子进程 + porcelain 解析,
// trait 缝留给未来的 git2 读实现。
pub mod cli;
pub mod ops;
/// epoch 秒 → "yyMMdd-HHmmss"(UTC):worktree 命名的时间段,无依赖手写。
pub mod timefmt;
/// WorktreeManager:命名规范 + 路径决策 + 增删查编排 + worktree 变更事件缝。
pub mod worktree;
