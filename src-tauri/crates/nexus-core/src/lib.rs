// nexus-core:领域核心(会话/PTY/配置/gitx),不依赖 tauri——
// 可以脱离 GUI 用纯 cargo test 验证(spec §1.1 拆分动机)。
pub mod agent;
pub mod config;
pub mod error;
pub mod gitx;
pub mod ids;
pub mod pty;

pub use error::NexusError;
