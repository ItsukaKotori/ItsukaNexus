// 领域错误:M1 只需要三个变体;随里程碑推进再分层(spec M4 提到 Config/Git/Pty/Spawn 分层)
use thiserror::Error;

#[derive(Debug, Error)]
pub enum NexusError {
    /// PTY/IO 层错误(spawn 失败、write 失败等,std::io::Error 自动转换)
    #[error("PTY 错误: {0}")]
    Pty(#[from] std::io::Error),
    /// 会话 id 从未存在(注册表里查无此项)
    #[error("会话不存在: {0}")]
    SessionNotFound(String),
    /// 会话存在但不在 Running(Stopping/Exited/Failed):拒绝输入/再次停止
    #[error("会话未在运行: {0}")]
    SessionNotRunning(String),
    /// provider_id 不被支持(M1 只认 "shell")
    #[error("不支持的 provider: {0}")]
    UnsupportedProvider(String),
    /// 系统无 git 或版本低到不可用(check 已探测,功能入口应先 gate)
    #[error("git 不可用: {0}")]
    GitUnavailable(String),
    /// 给定路径不是 git 仓库(rev-parse 失败)
    #[error("不是 git 仓库: {0}")]
    NotARepo(String),
    /// git 命令执行失败(stderr 透传给 UI)
    #[error("git {cmd} 失败: {stderr}")]
    GitCommand { cmd: String, stderr: String },
    /// 命令参数组合非法(如 worktree_name 没配 repo_path、worktree 不存在)
    #[error("参数无效: {0}")]
    InvalidInput(String),
}
