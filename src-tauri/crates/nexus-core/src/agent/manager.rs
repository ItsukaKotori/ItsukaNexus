// SessionManager(M2 async 版):会话注册表(瘦句柄)+ 每会话任务树。
//
// 编排(M1 三线程 → M2 三任务):reader 在 spawn_blocking 里做阻塞读,
// 原始字节经有界通道(64)进入 batcher 任务(async 合帧 + 增量解码 +
// replay/订阅分发),wait 任务在 spawn_blocking 里收割子进程。
//
// 事件缝:所有低频通知(State/Exit)走构造注入的 EventSink,manager 不知道
// tauri 的存在(nexus-core 不依赖 tauri,spec §1.3);Output 是高频流,不再走
// 全局 sink——经 per-session subscribe 的有界通道(128)下发,满则背压向
// reader 传导(M1 遗留 #2/#5 的最终解)。
//
// 锁序约定(全文件,防死锁):sessions 表锁 → (subscriber | replay | writer |
// killer | master)句柄内锁;其中 subscriber→replay 必须按此序成对嵌套:batcher
// 的 push+clone 与 subscribe 的 snapshot+替换都以订阅锁包住 replay 锁,两个临界
// 段互斥即接缝原子化(I-1:replay 与实时流不重复不丢失)。任何 std 锁都绝不
// 跨越 .await / sink 回调(需要跨点的值先 clone 出来再放锁)。
use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use portable_pty::{ChildKiller, MasterPty, PtySize};
use serde::Serialize;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::agent::state::{now_ms, SessionSnapshot, SessionState, StateChange};
use crate::error::NexusError;
use crate::ids::SessionId;
use crate::pty::batcher::{next_frame_async, FRAME_MAX_BYTES, FRAME_WINDOW};
use crate::pty::decode::Decoder;
use crate::pty::replay::ReplayBuffer;
use crate::pty::session::{pty_err, PtySession};

/// 读任务每次 read 的缓冲大小(spec §1.4)
const READ_CHUNK: usize = 8 * 1024;
/// reader→batcher 有界队列深度:满则 reader 阻塞 = 背压第一级(spec §1.4)
const QUEUE_DEPTH: usize = 64;
/// 订阅流容量:满则 batcher 挂起 = 背压第二级(经 raw 通道向 reader 传导)
const SUBSCRIBER_DEPTH: usize = 128;
/// 每会话回放缓冲容量(spec §1.3:最近 ~256KB)
const REPLAY_CAP: usize = 256 * 1024;
/// wait 任务 join 输出管线的上限(P7):超时放弃 join,Exit 不再被卡死
const JOIN_TIMEOUT: Duration = Duration::from_secs(5);
/// 优雅关停宽限期:发 \x03 后等子进程自行退出的上限
const GRACE_PERIOD: Duration = Duration::from_secs(2);
/// \x03 写入超时(I1):PTY 输入缓冲满时 write_all 可无限期阻塞,必须限时。
/// 写超时不直接 kill,而是放弃这次写入、照常走宽限轮询:子进程若在写超时
/// 窗口内自行退出则免杀更优;全程最坏 ~2s(写)+ ~2s(宽限)有界
const CTRL_C_WRITE_TIMEOUT: Duration = Duration::from_secs(2);
/// send_input 的写超时(必办#3):大粘贴 + 子进程不读 → write 阻塞。
/// 与 stop 的 Ctrl-C 写超时同型:超时放弃等待,spawn_blocking 线程随写入
/// 最终完成/失败自然收场(最多泄漏一个,进程退出兜底)。
const SEND_INPUT_TIMEOUT: Duration = Duration::from_secs(2);
/// 优雅关停轮询间隔(状态迁移的真正通知由 wait 任务驱动,这里只是等它)
const POLL_INTERVAL: Duration = Duration::from_millis(200);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum SessionEvent {
    Output {
        session_id: SessionId,
        data: String,
        seq: u64,
    },
    State(crate::agent::state::StateChange),
    Exit {
        session_id: SessionId,
        code: i32,
    },
}

pub type EventSink = Arc<dyn Fn(SessionEvent) + Send + Sync>;

/// 每会话输出帧:订阅流的载荷(OutputFrame 留在 core 侧,IPC 层映射自己的
/// wire 类型——保 nexus-core 无 tauri 的缝,Ruling M2-P2)
#[derive(Debug, Clone)]
pub struct OutputFrame {
    pub seq: u64,
    pub data: String,
}

/// subscribe 返回:历史回放 + 实时流接收端。
/// 重复 subscribe 替换旧订阅:旧发送端被 drop,旧接收端在存量耗尽后收到关闭。
pub struct Subscription {
    pub replay: String,
    pub rx: mpsc::Receiver<OutputFrame>,
}

/// 会话落点(spec §1.4 session_create 的 repo_path/worktree_name 消费端)。
/// cwd 是 PTY 子进程的工作目录(worktree 目录即落这里);repo_path/
/// worktree_name 原样透传进快照,前端据它标记 tab 的 worktree 归属。
#[derive(Debug, Clone)]
pub struct LaunchSpec {
    pub cwd: PathBuf,
    pub repo_path: Option<String>,
    pub worktree_name: Option<String>,
}

/// 瘦句柄:表里存它,任务树/调用方共享其中的 Arc。Clone 便宜(Arc + 快照)。
#[derive(Clone)]
struct SessionHandle {
    writer: Arc<Mutex<Box<dyn std::io::Write + Send>>>,
    killer: Arc<Mutex<Box<dyn ChildKiller + Send + Sync>>>,
    /// None = master 已被 stop 的进程组 kill 消费(drop → 内核向前台组投 SIGHUP)
    master: Arc<Mutex<Option<Box<dyn MasterPty + Send>>>>,
    snapshot: SessionSnapshot,
    replay: Arc<Mutex<ReplayBuffer>>,
    /// 当前订阅发送端;None = 无人订阅(replay 照常积累)
    subscriber: Arc<Mutex<Option<mpsc::Sender<OutputFrame>>>>,
    /// 协作取消:kill 之后取消,输出任务树自行收尾,不等可能永不到来的 EOF
    /// (孙进程持 slave fd 时 reader 的 EOF 不会来)
    cancel: CancellationToken,
}

/// 共享内核:表 + sink。wait 任务持 Arc<Inner> 完成状态写回(Ruling M2-P1:
/// 实现自由度里选了"Arc 共享",因为退出后条目本就要保留,强引用无碍)。
struct Inner {
    sink: EventSink,
    sessions: Mutex<HashMap<SessionId, SessionHandle>>,
}

impl Inner {
    /// 状态写回的唯一入口:表锁更新快照 + can_transition_to 校验 +
    /// 恰好一次 State 事件(锁外发,防 sink 回调重入 manager 死锁)。
    /// 非法迁移(如终态再迁移)记日志跳过,事件不发。
    fn set_state(
        &self,
        id: SessionId,
        next: SessionState,
        exit_code: Option<i32>,
        detail: Option<String>,
    ) {
        let prev = {
            let mut guard = self.sessions.lock().expect("会话表锁被毒化");
            let Some(handle) = guard.get_mut(&id) else {
                return; // 条目已不在:无从谈起状态(M2 条目永不删除,防御性分支)
            };
            let prev = handle.snapshot.state;
            if !prev.can_transition_to(next) {
                log::warn!("非法状态迁移 session={id} {prev:?} -> {next:?},已跳过");
                return;
            }
            handle.snapshot.state = next;
            if exit_code.is_some() {
                handle.snapshot.exit_code = exit_code;
            }
            prev
        }; // 表锁在此释放
        (self.sink)(SessionEvent::State(StateChange {
            session_id: id,
            prev,
            next,
            at_ms: now_ms(),
            detail,
        }));
    }

    /// 只读快照状态(stop 的宽限期轮询用;None = 条目不在,视同已退出)
    fn snapshot_state(&self, id: SessionId) -> Option<SessionState> {
        self.sessions
            .lock()
            .expect("会话表锁被毒化")
            .get(&id)
            .map(|h| h.snapshot.state)
    }
}

pub struct SessionManager {
    inner: Arc<Inner>,
}

/// 带超时的写路径(必办#3,send_input 复用):write_all/flush 丢进阻塞线程
/// 池别扣住执行器;超时只是"放弃等待并报错"——被弃置的 spawn_blocking 线程
/// 仍在写(线程无法安全杀),持有 writer 锁直到写入最终完成/失败,锁竞争由
/// stop 路径自己的写超时兜底。超时后最多滞留一个阻塞线程,进程退出兜底。
async fn write_with_timeout(
    writer: Arc<Mutex<Box<dyn std::io::Write + Send>>>,
    bytes: Vec<u8>,
    timeout: Duration,
) -> Result<(), NexusError> {
    match tokio::time::timeout(
        timeout,
        tokio::task::spawn_blocking(move || -> Result<(), NexusError> {
            let mut w = writer.lock().map_err(|e| {
                NexusError::Pty(std::io::Error::other(format!("writer 被毒化: {e}")))
            })?;
            w.write_all(&bytes)?;
            w.flush()?;
            Ok(())
        }),
    )
    .await
    {
        Ok(Ok(res)) => res,
        Ok(Err(e)) => Err(NexusError::Pty(std::io::Error::other(format!(
            "写任务失败: {e}"
        )))),
        Err(_) => Err(NexusError::Pty(std::io::Error::other(format!(
            "写超时({timeout:?}):子进程未消费输入(可能已停止读取)"
        )))),
    }
}

/// 默认 shell:优先 $SHELL,逐级 fallback。
/// M4 由 AgentProfile/配置驱动,此函数退化为兜底默认值。
pub fn default_shell() -> String {
    if cfg!(windows) {
        "powershell.exe".into()
    } else {
        std::env::var("SHELL")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "/bin/zsh".into())
    }
}

impl SessionManager {
    pub fn new(sink: EventSink) -> Self {
        Self {
            inner: Arc::new(Inner {
                sink,
                sessions: Mutex::new(HashMap::new()),
            }),
        }
    }

    /// spawn 一个 shell 会话,拉起三段任务树,入表后发 State{Running}。
    /// launch = Some 时子进程 cwd 落在该目录(worktree 集成点),repo_path/
    /// worktree_name 透传进快照;None = M2 行为(继承本进程 cwd,快照两字段
    /// 为 None)。注意:本函数不 await 任何东西,async 是与 IPC 层(Task 6)
    /// 的签名契约。
    pub async fn create(
        &self,
        provider_id: &str,
        cols: u16,
        rows: u16,
        launch: Option<&LaunchSpec>,
    ) -> Result<SessionId, NexusError> {
        if provider_id != "shell" {
            return Err(NexusError::UnsupportedProvider(provider_id.to_string()));
        }
        let id = SessionId::new();
        // take_reader 内部的 expect 是 panic 路径,前提是 master 存活——
        // 这里 session 刚 spawn、master 尚未 drop/关闭,前提成立(继承 M1 注记)。
        let (session, child) = PtySession::spawn(
            &default_shell(),
            &[],
            cols,
            rows,
            launch.map(|l| l.cwd.as_path()),
        )?;
        let reader = session.take_reader();
        let parts = session.into_parts();
        let pid = child.process_id();

        let (raw_tx, raw_rx) = mpsc::channel::<Vec<u8>>(QUEUE_DEPTH);
        // 初始订阅端按骨架给 Some:订阅前帧只进 replay(接收端无人持有,send 即 err)
        let (out_tx, _out_rx) = mpsc::channel::<OutputFrame>(SUBSCRIBER_DEPTH);

        let cancel = CancellationToken::new();
        let replay = Arc::new(Mutex::new(ReplayBuffer::new(REPLAY_CAP)));
        let subscriber = Arc::new(Mutex::new(Some(out_tx)));

        // 任务 1/3:reader——spawn_blocking 桥接阻塞读;"阻塞 IO 的边界"就是
        // 这条通道:队列满时 blocking_send 阻塞本线程 = 背压(M1 语义同构)。
        // reader 退出由 EOF/通道关闭驱动,不需要取消令牌;令牌用于输出侧。
        tokio::task::spawn_blocking(move || {
            let mut reader = reader;
            let mut buf = vec![0u8; READ_CHUNK];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break, // EOF 或设备错误:会话输出结束
                    Ok(n) => {
                        if raw_tx.blocking_send(buf[..n].to_vec()).is_err() {
                            break; // batcher 已退出,无人消费
                        }
                    }
                }
            }
        });

        // 任务 2/3:batcher——async 合帧 + 增量解码 + replay/订阅分发
        let mut decoder = Decoder::new();
        let replay_b = replay.clone();
        let sub_b = subscriber.clone();
        let cancel_b = cancel.clone();
        let batcher = tokio::spawn(async move {
            let mut rx = raw_rx;
            let mut seq: u64 = 0;
            loop {
                // 令牌是给我们自己的任务树的:kill 后立即收尾,不等永不到来的 EOF
                let frame = tokio::select! {
                    frame = next_frame_async(&mut rx, FRAME_WINDOW, FRAME_MAX_BYTES) => {
                        match frame {
                            Some(f) => f,
                            None => break, // raw 通道关闭:输出结束
                        }
                    }
                    _ = cancel_b.cancelled() => break,
                };
                let text = decoder.feed(&frame);
                if text.is_empty() {
                    continue; // 本帧只是不完整的多字节尾部:扣留在 decoder 内
                }
                seq += 1;
                // 接缝原子化(I-1):持订阅锁的临界段内完成 replay push + 发送端
                // 克隆,锁序 subscriber→replay,与 subscribe 的 snapshot+替换
                // 互斥——帧要么只落 replay(成为下一订阅的快照),要么只走实时
                // 流。锁不跨 await:clone 出发送端后放锁再慢递(订阅被替换时
                // 旧克隆自然耗尽)
                let tx = {
                    let sub_guard = sub_b.lock().expect("订阅锁");
                    replay_b.lock().expect("replay 锁").push_str(&text);
                    sub_guard.clone()
                };
                let of = OutputFrame { seq, data: text };
                if let Some(tx) = tx {
                    // 订阅通道满 = 前端消费慢:在此挂起,背压经 raw 通道向 reader
                    // 传导。send 必须与 cancelled 竞争(I2):否则强杀后任务树被
                    // 停滞的订阅者钉住,cancel 收不了尾
                    tokio::select! {
                        r = tx.send(of) => {
                            if r.is_err() {
                                // 订阅者已 drop(被替换/关闭):只留 replay
                            }
                        }
                        // cancel 分支 break:残余帧弃置——强杀语义(见偏差①)
                        _ = cancel_b.cancelled() => break,
                    }
                }
            }
        });

        // 任务 3/3:wait——收割子进程;join 输出管线带超时(P7),先写状态再发 Exit
        let inner = self.inner.clone();
        let cancel_w = cancel.clone();
        tokio::spawn(async move {
            // child.wait 是阻塞调用:丢进阻塞线程池,async 侧只等结果
            let code = tokio::task::spawn_blocking(move || {
                let mut child = child;
                child.wait().ok().map(|s| s.exit_code() as i32)
            })
            .await
            .ok() // join 失败(任务 panic 等)视同 wait 失败
            .flatten(); // wait 本身出错 → None → Failed
            let (next, code) = match code {
                Some(c) => (SessionState::Exited, c),
                None => (SessionState::Failed, -1),
            };
            // join 输出管线(给最后几帧让路),超时放弃——Exit 不再被卡死(P7)
            let detail = match tokio::time::timeout(JOIN_TIMEOUT, batcher).await {
                Ok(Ok(())) => None,
                Ok(Err(e)) => Some(format!("output task failed: {e}")),
                Err(_) => {
                    cancel_w.cancel(); // 卡死的任务树自行收尾,只泄漏一个任务
                    log::warn!("输出管线 join 超时 session={id},已取消任务树(stalled)");
                    Some("output pipeline stalled".into())
                }
            };
            // 先写状态再发 Exit:收到 Exit 时 list() 必已可见终态(测试钉住)
            inner.set_state(id, next, Some(code), detail);
            // 终态断订阅(必办#2):常驻转发任务在存量耗尽后自然结束,
            // 退出会话不再占订阅通道;条目与 replay 保留(刷新恢复仍可看)。
            // 锁序 表锁→订阅锁,持锁只做置 None,不跨 await/sink(文件头约定)
            if let Some(h) = inner.sessions.lock().expect("会话表锁被毒化").get_mut(&id) {
                *h.subscriber.lock().expect("订阅锁") = None;
            }
            (inner.sink)(SessionEvent::Exit {
                session_id: id,
                code,
            });
        });

        let handle = SessionHandle {
            writer: parts.writer,
            killer: parts.killer,
            master: parts.master,
            snapshot: SessionSnapshot {
                session_id: id,
                state: SessionState::Running,
                started_at_ms: now_ms(),
                exit_code: None,
                pid,
                repo_path: launch.and_then(|l| l.repo_path.clone()),
                worktree_name: launch.and_then(|l| l.worktree_name.clone()),
            },
            replay,
            subscriber,
            cancel,
        };
        self.inner
            .sessions
            .lock()
            .expect("会话表锁被毒化")
            .insert(id, handle);

        // 诞生即 Running:M2 无 Created 前驱状态,这是一次"状态播报"而非迁移
        // (Running→Running 过不了 can_transition_to,故不走 set_state,直接播报)
        (self.inner.sink)(SessionEvent::State(StateChange {
            session_id: id,
            prev: SessionState::Running,
            next: SessionState::Running,
            at_ms: now_ms(),
            detail: None,
        }));
        Ok(id)
    }

    /// 订阅会话输出:返回订阅前的历史回放 + 实时流接收端。
    /// 重复 subscribe 替换旧订阅(M1 遗留 #2):旧发送端 drop,旧接收端在
    /// 存量帧耗尽后收到关闭;seq 单会话单调,跨订阅不回退。
    /// 接缝原子化(I-1):snapshot+替换与 batcher 的 push+clone 互斥(同锁序
    /// subscriber→replay),replay 与实时流不重复不丢失。
    pub fn subscribe(&self, id: SessionId) -> Result<Subscription, NexusError> {
        let mut guard = self.inner.sessions.lock().expect("会话表锁被毒化");
        let handle = guard
            .get_mut(&id)
            .ok_or_else(|| NexusError::SessionNotFound(id.to_string()))?;
        // 终态:batcher 已退出,不会再有帧。返回已关闭的 rx(drop 发送端),
        // IPC 转发任务 recv 即 None 自然收尾——刷新恢复时 attach 已退出会话
        // 不再泄漏常驻转发任务;replay 照常重放历史。
        if matches!(
            handle.snapshot.state,
            SessionState::Exited | SessionState::Failed
        ) {
            let replay = handle.replay.lock().expect("replay 锁").snapshot();
            let (_tx, rx) = mpsc::channel(1);
            drop(_tx);
            return Ok(Subscription { replay, rx });
        }
        let (tx, rx) = mpsc::channel(SUBSCRIBER_DEPTH);
        // 接缝原子化(I-1):持订阅锁期间完成 snapshot+替换(锁序 subscriber→
        // replay),与 batcher 的 push+clone 临界段互斥——snapshot 只含已 push
        // 的帧,替换后的新帧只走新通道,两种交错退化为二选一
        let replay = {
            let mut sub_guard = handle.subscriber.lock().expect("订阅锁");
            let snap = handle.replay.lock().expect("replay 锁").snapshot();
            *sub_guard = Some(tx);
            snap
        };
        Ok(Subscription { replay, rx })
    }

    /// 写入用户输入:只查状态、借走 writer 克隆,绝不持表锁做 I/O(风险 #5)。
    pub async fn send_input(&self, id: SessionId, data: &str) -> Result<(), NexusError> {
        let writer = {
            let guard = self.inner.sessions.lock().expect("会话表锁被毒化");
            let handle = guard
                .get(&id)
                .ok_or_else(|| NexusError::SessionNotFound(id.to_string()))?;
            if !matches!(handle.snapshot.state, SessionState::Running) {
                return Err(NexusError::SessionNotRunning(id.to_string()));
            }
            handle.writer.clone()
        }; // 表锁在此释放
        let bytes = data.as_bytes().to_vec();
        // 大粘贴 + 子进程不读 = write 可能长时间阻塞:丢进阻塞线程池,别扣住执行器
        write_with_timeout(writer, bytes, SEND_INPUT_TIMEOUT).await
    }

    pub fn resize(&self, id: SessionId, cols: u16, rows: u16) -> Result<(), NexusError> {
        let master = {
            let guard = self.inner.sessions.lock().expect("会话表锁被毒化");
            let handle = guard
                .get(&id)
                .ok_or_else(|| NexusError::SessionNotFound(id.to_string()))?;
            handle.master.clone()
        }; // 表锁在此释放
        let master = master
            .lock()
            .map_err(|e| NexusError::Pty(std::io::Error::other(format!("master 被毒化: {e}"))))?;
        // master 已被 stop 消费(None):会话必死,resize 无意义,静默成功
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

    /// 停止会话。force=true 直接 kill;false 先发 \x03(Ctrl-C 字节进 PTY,是给
    /// 子进程的信号)、宽限 2s 等它自行退出、超时再 kill。
    /// 条目永不删除:退出后仍在表里,状态 Exited/Failed(风险 #6 的最终解)。
    pub async fn stop(&self, id: SessionId, force: bool) -> Result<(), NexusError> {
        let handle = {
            self.inner
                .sessions
                .lock()
                .expect("会话表锁被毒化")
                .get(&id)
                .cloned()
        }
        .ok_or_else(|| NexusError::SessionNotFound(id.to_string()))?;
        if !matches!(handle.snapshot.state, SessionState::Running) {
            return Err(NexusError::SessionNotRunning(id.to_string()));
        }
        self.inner.set_state(id, SessionState::Stopping, None, None);

        if !force {
            let writer = handle.writer.clone();
            // \x03 写包超时(I1):缓冲满时 write_all 永久阻塞会击穿宽限设计。
            // 写超时只是放弃这次写入,随后照常宽限轮询(最坏 ~2s 写 + ~2s
            // 宽限有界,窗口内自行退出则免杀更优),仍未退才落 kill 路径。
            // 被超时弃置的 spawn_blocking 线程随写入最终完成/失败自然收场
            // (泄漏上限一个,进程退出兜底)
            let _ = tokio::time::timeout(
                CTRL_C_WRITE_TIMEOUT,
                tokio::task::spawn_blocking(move || {
                    let mut w = writer.lock().expect("writer 锁被毒化");
                    let _ = w.write_all(b"\x03");
                    let _ = w.flush();
                }),
            )
            .await;
            // 轮询快照等退出(200ms 间隔;真正驱动迁移的是 wait 任务)
            let inner = self.inner.clone();
            let exited = tokio::time::timeout(GRACE_PERIOD, async move {
                loop {
                    tokio::time::sleep(POLL_INTERVAL).await;
                    if !matches!(
                        inner.snapshot_state(id),
                        Some(SessionState::Running | SessionState::Stopping)
                    ) {
                        return;
                    }
                }
            })
            .await
            .is_ok();
            if exited {
                return Ok(());
            }
        }
        {
            // Unix 进程组语义(M3 必办#1):子进程开 controlling terminal 即
            // session leader(pgid == pid),killpg 一次杀掉 shell 组;
            // 前台 busy job 在另一进程组——由 drop master 触发内核 SIGHUP。
            #[cfg(unix)]
            if let Some(pid) = handle.snapshot.pid {
                // ESRCH(组已死)等 errno 一概忽略:退出真相以 Exit 事件为准
                unsafe {
                    libc::killpg(pid as libc::pid_t, libc::SIGKILL);
                }
            }
            // 上游 portable-pty 0.9.0 WinChildKiller::kill 成败判定反转:
            // Err 不代表失败。Windows 主路径;Unix 兜底(killpg 之外的保险)
            let mut killer = handle.killer.lock().expect("killer 锁被毒化");
            if let Err(e) = killer.kill() {
                #[cfg(windows)]
                log::debug!("kill 返回 Err session={id}(Windows 判定反转,以 Exit 事件为真相): {e}");
                #[cfg(not(windows))]
                log::warn!("kill 失败 session={id}: {e}");
            }
        }
        // drop master:内核向前台进程组发 SIGHUP(kill 正忙 shell 的关键一击),
        // slave 全关 → reader EOF;master 已被消费(None)则无事发生
        drop(handle.master.lock().expect("master 锁被毒化").take());
        // 令牌是给我们自己的任务树的:kill 后输出管线无需等 EOF
        handle.cancel.cancel();
        Ok(())
    }

    /// 会话回收(必办#2,"关 tab 即删"):仅终态可删。
    /// drop 条目 = 释放 replay/订阅/句柄/取消令牌的全部 Arc(RAII),
    /// 无需显式 close——所有权即资源管理;运行中 → SessionNotRunning,
    /// 不存在 → SessionNotFound。
    pub fn dispose(&self, id: SessionId) -> Result<(), NexusError> {
        let mut guard = self.inner.sessions.lock().expect("会话表锁被毒化");
        let handle = guard
            .get(&id)
            .ok_or_else(|| NexusError::SessionNotFound(id.to_string()))?;
        if !matches!(
            handle.snapshot.state,
            SessionState::Exited | SessionState::Failed
        ) {
            return Err(NexusError::SessionNotRunning(id.to_string()));
        }
        guard.remove(&id);
        Ok(())
    }

    /// 会话快照列表:含已退出的会话(侧边栏显示 Exited/Failed),
    /// 按 startedAtMs 升序(必办#3:HashMap 无序,刷新后 tab 序要稳定)
    pub fn list(&self) -> Vec<SessionSnapshot> {
        let mut snaps: Vec<SessionSnapshot> = self
            .inner
            .sessions
            .lock()
            .expect("会话表锁被毒化")
            .values()
            .map(|h| h.snapshot.clone())
            .collect();
        snaps.sort_by_key(|s| s.started_at_ms);
        snaps
    }
}

#[cfg(test)]
mod write_timeout_tests {
    use super::*;

    /// write_all 永不完成的写端:模拟"子进程不读、PTY 输入缓冲满"(Linux
    /// n_tty 会阻塞 master 写;macOS xnu 不阻塞,故超时路径用 mock 钉住)。
    /// 阻塞用 channel 门而非 sleep:超时弃置的 spawn_blocking 线程要等
    /// runtime 关停时收割,sleep 型 mock 会把整个测试二进制钉在关停上;
    /// 测试断言完显式放行,写线程即刻自然收场
    struct BlockedWriter(std::sync::mpsc::Receiver<()>);
    impl std::io::Write for BlockedWriter {
        fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
            let _ = self.0.recv(); // 阻塞直到测试放行
            Ok(5)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// 立即完成的写端:守卫"快速写不得误报超时"
    struct InstantWriter;
    impl std::io::Write for InstantWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn write_times_out_when_writer_never_completes() {
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        let writer: Arc<Mutex<Box<dyn std::io::Write + Send>>> =
            Arc::new(Mutex::new(Box::new(BlockedWriter(release_rx))));
        let start = std::time::Instant::now();
        let r = write_with_timeout(writer, b"hello".to_vec(), Duration::from_millis(100)).await;
        let elapsed = start.elapsed();
        let err = r.expect_err("永不完成的写必须在超时窗后报错");
        assert!(
            matches!(err, NexusError::Pty(_)),
            "超时错误应为 Pty: {err:?}"
        );
        assert!(
            elapsed >= Duration::from_millis(100),
            "必须等满超时窗,实际 {elapsed:?}"
        );
        assert!(
            elapsed < Duration::from_secs(5),
            "超时必须及时返回而非挂死,实际 {elapsed:?}"
        );
        // 先放行被弃置的写线程,再让 runtime 关停收割它(顺序即防钉死)
        drop(release_tx);
    }

    #[tokio::test]
    async fn write_succeeds_when_writer_is_fast() {
        let writer: Arc<Mutex<Box<dyn std::io::Write + Send>>> =
            Arc::new(Mutex::new(Box::new(InstantWriter)));
        let r = write_with_timeout(writer, b"hello".to_vec(), Duration::from_millis(100)).await;
        assert!(r.is_ok(), "快速写端不得误报超时: {r:?}");
    }
}
