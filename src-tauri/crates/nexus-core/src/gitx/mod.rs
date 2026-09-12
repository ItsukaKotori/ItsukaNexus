// gitx:git 集成域。选型(spec §3):CLI 子进程 + porcelain 解析,
// trait 缝留给未来的 git2 读实现。
pub mod cli;
pub mod ops;
// pub mod worktree; // Task 8 落地:WorktreeManager + worktree 三方法实现;届时打开此行
