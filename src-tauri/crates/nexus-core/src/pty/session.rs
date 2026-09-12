// PtySession:一个受管 PTY 会话的读写/控制面。
// 并发模型(M2 起,与 tokio 任务树对接):
//   - reader:take_reader 交出去(阻塞读跑在 spawn_blocking 里)
//   - writer/killer/master:全是 Arc<Mutex<>> 共享形态,into_parts 拆给 manager
//   - child:spawn 后立刻拆走——wait 独占给 wait 任务,kill 用 clone_killer 留在这
use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Mutex};

use portable_pty::{
    Child, ChildKiller, CommandBuilder, MasterPty, NativePtySystem, PtySize, PtySystem,
};

use crate::error::NexusError;

// portable-pty 0.9 的 API 返回 anyhow::Result(而非 io::Result),
// 这里统一折进 NexusError::Pty;不直接依赖 anyhow。manager 的 resize 复用。
pub(crate) fn pty_err<E: std::fmt::Display>(e: E) -> NexusError {
    NexusError::Pty(std::io::Error::other(e.to_string()))
}

pub struct PtySession {
    // master 供 take_reader/resize 用;trait 无 Sync 上界(0.9 只有 Downcast + Send),
    // 用 Mutex 提供共享所需的同步性,而写入/锁竞争只有这两条短小路径。
    // Option:stop 的 Unix 进程组 kill(M3 必办#1)会把 master 消费成 None
    // (drop master → 内核向前台组投 SIGHUP);此后 resize 静默成功,
    // take_reader 不可能再被调(create 时序保证,见方法注记)
    master: Arc<Mutex<Option<Box<dyn MasterPty + Send>>>>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    // ChildKiller::kill 需要 &mut,而 kill 语义上谁都能调(&self)——用 Mutex 提供内部可变性
    killer: Arc<Mutex<Box<dyn ChildKiller + Send + Sync>>>,
}

/// `into_parts` 拆出的共享件:manager 的会话句柄按 Arc 分发给各任务/调用方。
pub struct PtySessionParts {
    pub writer: Arc<Mutex<Box<dyn Write + Send>>>,
    pub killer: Arc<Mutex<Box<dyn ChildKiller + Send + Sync>>>,
    pub master: Arc<Mutex<Option<Box<dyn MasterPty + Send>>>>,
}

impl PtySession {
    /// spawn 一个跑在 PTY 里的进程。
    /// 返回 (会话句柄, child):child 必须立刻交给专属任务调 wait(),
    /// 否则进程退出后无人收割(Windows 上即"僵尸句柄",spec M1 完成标准③)。
    /// spawn 一个跑在 PTY 里的进程。
    /// 返回 (会话句柄, child):child 必须立刻交给专属任务调 wait(),
    /// 否则进程退出后无人收割(Windows 上即"僵尸句柄",spec M1 完成标准③)。
    /// cwd = None 时继承本进程工作目录(M2 行为);Some 则作为子进程的
    /// 工作目录(worktree 集成点,spec §1.4 session_create 的落点)。
    pub fn spawn(
        program: &str,
        args: &[&str],
        cols: u16,
        rows: u16,
        cwd: Option<&Path>,
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
        if let Some(c) = cwd {
            cmd.cwd(c);
        }
        // 颜色/终端能力注入(spec §1.3):让 TUI 程序输出真彩 ANSI
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");

        let child = pair.slave.spawn_command(cmd).map_err(pty_err)?;
        let writer = pair.master.take_writer().map_err(pty_err)?;
        let killer = Arc::new(Mutex::new(child.clone_killer()));
        let master = Arc::new(Mutex::new(Some(pair.master)));

        let sess = Self {
            master,
            writer: Arc::new(Mutex::new(writer)),
            killer,
        };
        Ok((sess, child))
    }

    /// 取走 reader 的克隆(master 允许多次 clone reader;M2 只需要一个)。
    /// 拿到它的任务独占进行阻塞读,直到 EOF/错误。
    /// None(master 已被 stop 的进程组 kill 消费)不可能发生:take_reader
    /// 只在 create 时序调用,那时 master 刚 spawn、尚无人消费。
    pub fn take_reader(&self) -> Box<dyn std::io::Read + Send> {
        let master = self.master.lock().expect("master 锁被毒化");
        master
            .as_ref()
            .expect("take_reader 只在 create 时序调用,master 不可能已被消费")
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
        let master = self.master.lock().expect("master 锁被毒化");
        if let Some(m) = master.as_ref() {
            m.resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(pty_err)?;
        }
        Ok(())
    }

    /// 强杀子进程。M1 的 stop 一律走这里;
    /// M2 的 stop:force=false 先 \x03、宽限,再落到这里的等价路径(killer.kill)。
    pub fn kill(&self) -> Result<(), NexusError> {
        self.killer
            .lock()
            .map_err(|e| NexusError::Pty(std::io::Error::other(format!("killer 被毒化: {e}"))))?
            .kill()?;
        Ok(())
    }

    /// 拆出 Arc 形态的共享件(writer/killer/master)供 manager 任务树分发。
    /// self 被消费:reader 此前已由 take_reader 交出,child 由 spawn 返回值给出。
    pub fn into_parts(self) -> PtySessionParts {
        PtySessionParts {
            writer: self.writer,
            killer: self.killer,
            master: self.master,
        }
    }
}
