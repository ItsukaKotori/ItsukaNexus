// PtySession:一个受管 PTY 会话的读写/控制面。
// 并发模型(M1,同步线程):
//   - reader:单线程独占(take_reader 交出去,本结构不再持有)
//   - writer:Arc<Mutex<>> 共享(IPC 命令线程写,会话表持有)
//   - child:spawn 后立刻拆走——wait 独占给 wait 线程,kill 用 clone_killer 留在这
use std::io::Write;
use std::sync::{Arc, Mutex};

use portable_pty::{
    Child, ChildKiller, CommandBuilder, MasterPty, NativePtySystem, PtySize, PtySystem,
};

use crate::error::NexusError;
use crate::ids::SessionId;

pub struct PtySession {
    #[allow(dead_code)] // M1 尚未读 session_id;manager(Task 4)会用
    session_id: SessionId,
    master: Box<dyn MasterPty + Send>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    // ChildKiller::kill 需要 &mut,而 kill 语义上谁都能调(&self)——用 Mutex 提供内部可变性
    killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
}

// portable-pty 0.9 的 API 返回 anyhow::Result(而非 io::Result),
// 这里统一折进 NexusError::Pty;不直接依赖 anyhow,保持 NexusError 三个变体不变。
fn pty_err<E: std::fmt::Display>(e: E) -> NexusError {
    NexusError::Pty(std::io::Error::other(e.to_string()))
}

impl PtySession {
    /// spawn 一个跑在 PTY 里的进程。
    /// 返回 (会话句柄, child):child 必须立刻交给专属线程调 wait(),
    /// 否则进程退出后无人收割(Windows 上即"僵尸句柄",spec M1 完成标准③)。
    pub fn spawn(
        session_id: SessionId,
        program: &str,
        args: &[&str],
        cols: u16,
        rows: u16,
    ) -> Result<(Self, Box<dyn Child + Send>), NexusError> {
        let pty_system = NativePtySystem::default();
        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(pty_err)?;

        let mut cmd = CommandBuilder::new(program);
        cmd.args(args);
        // 颜色/终端能力注入(spec §1.3):让 TUI 程序输出真彩 ANSI
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");

        let child = pair.slave.spawn_command(cmd).map_err(pty_err)?;
        let writer = pair.master.take_writer().map_err(pty_err)?;
        let killer = Mutex::new(child.clone_killer());

        let sess = Self {
            session_id,
            master: pair.master,
            writer: Arc::new(Mutex::new(writer)),
            killer,
        };
        Ok((sess, child))
    }

    /// 取走 reader 的克隆(master 允许多次 clone reader;M1 只需要一个)。
    /// 拿到它的线程独占进行阻塞读,直到 EOF/错误。
    pub fn take_reader(&self) -> Box<dyn std::io::Read + Send> {
        self.master
            .try_clone_reader()
            .expect("master 存活期内 clone reader 不会失败")
    }

    pub fn write_all(&self, bytes: &[u8]) -> Result<(), NexusError> {
        let mut w = self
            .writer
            .lock()
            .map_err(|e| NexusError::Pty(std::io::Error::other(format!("writer 被毒化: {e}"))))?;
        w.write_all(bytes)?;
        w.flush()?;
        Ok(())
    }

    pub fn resize(&self, cols: u16, rows: u16) -> Result<(), NexusError> {
        self.master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(pty_err)?;
        Ok(())
    }

    /// 强杀子进程。M1 的 stop 一律走这里;
    /// force=false 的优雅关停(先 Ctrl-C、再等待、最后 kill)是 M2 CancellationToken 的活。
    pub fn kill(&self) -> Result<(), NexusError> {
        self.killer
            .lock()
            .map_err(|e| NexusError::Pty(std::io::Error::other(format!("killer 被毒化: {e}"))))?
            .kill()?;
        Ok(())
    }
}
