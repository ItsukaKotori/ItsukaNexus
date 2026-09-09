// 领域错误:M1 只需要三个变体;随里程碑推进再分层(spec M4 提到 Config/Git/Pty/Spawn 分层)
use thiserror::Error;

#[derive(Debug, Error)]
pub enum NexusError {
    /// PTY/IO 层错误(spawn 失败、write 失败等,std::io::Error 自动转换)
    #[error("PTY 错误: {0}")]
    Pty(#[from] std::io::Error),
    /// 会话 id 查不到(已停止或从未创建)
    #[error("会话不存在: {0}")]
    SessionNotFound(String),
    /// provider_id 不被支持(M1 只认 "shell")
    #[error("不支持的 provider: {0}")]
    UnsupportedProvider(String),
}
