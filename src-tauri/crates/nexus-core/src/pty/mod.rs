// PTY 子系统:对 portable-pty 的封装。
// 模块边界即未来 crate 边界(spec §1.2 布局):M3 拆分时整体搬入 nexus-core。
pub mod batcher;
pub mod decode;
pub mod replay;
pub mod session;
