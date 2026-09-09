// SessionManager:会话注册表 + 每会话三线程编排(reader/batcher/wait)。
// 事件缝:所有对外通知走构造注入的 EventSink,manager 不知道 tauri 的存在
// (spec §1.3:nexus-core 不依赖 tauri;M2 这里换成 EventBus)。
use std::collections::HashMap;
use std::io::Read;
use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;

use serde::Serialize;

use crate::error::NexusError;
use crate::ids::SessionId;
use crate::pty::batcher::{next_frame, FRAME_MAX_BYTES, FRAME_WINDOW};
use crate::pty::session::PtySession;

/// 读线程每次 read 的缓冲大小(spec §1.4)
const READ_CHUNK: usize = 8 * 1024;
/// 有界队列深度(spec §1.4):满则 reader 阻塞 = 背压
const QUEUE_DEPTH: usize = 64;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum SessionEvent {
    Output { id: SessionId, data: String },
    Exit { id: SessionId, code: i32 },
}

pub type EventSink = Arc<dyn Fn(SessionEvent) + Send + Sync>;

pub struct SessionManager {
    sink: EventSink,
    sessions: Mutex<HashMap<SessionId, PtySession>>,
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
            sink,
            sessions: Mutex::new(HashMap::new()),
        }
    }

    pub fn create(&self, provider_id: &str, cols: u16, rows: u16) -> Result<SessionId, NexusError> {
        if provider_id != "shell" {
            return Err(NexusError::UnsupportedProvider(provider_id.to_string()));
        }
        let id = SessionId::new();
        let shell = default_shell();
        let program: String = shell;
        // take_reader 内部的 expect(见 PtySession)是 panic 路径,前提是 master 存活——
        // 这里 session 刚 spawn、master 尚未 drop/关闭,前提成立(Task 2 遗留复查项,维持现状)。
        let (session, mut child) = PtySession::spawn(id, &program, &[], cols, rows)?;

        let reader = session.take_reader();
        let (tx, rx) = mpsc::sync_channel::<Vec<u8>>(QUEUE_DEPTH);

        // 线程 1/3:reader——阻塞读 master,8KB 一块塞进有界队列
        std::thread::spawn(move || {
            let mut reader = reader;
            let mut buf = vec![0u8; READ_CHUNK];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break, // EOF 或设备错误:会话输出结束
                    Ok(n) => {
                        if tx.send(buf[..n].to_vec()).is_err() {
                            break; // batcher 已退出,无人消费
                        }
                    }
                }
            }
        }); // tx 在此 drop → 通道关闭 → batcher 的 next_frame 返 None

        // 线程 2/3:batcher——合帧后转 String 经 sink 通知
        let sink_output = self.sink.clone();
        let batcher: JoinHandle<()> = std::thread::spawn(move || {
            while let Some(frame) = next_frame(&rx, FRAME_WINDOW, FRAME_MAX_BYTES) {
                // M1 临时方案(spec 明示):lossy 解码,跨 chunk 多字节字符可能出替换符,M2 换增量解码器
                let data = String::from_utf8_lossy(&frame).into_owned();
                sink_output(SessionEvent::Output { id, data });
            }
        });

        // 线程 3/3:wait——收割子进程;先 join batcher 再发 Exit,
        // 保证退出事件不抢在最后几帧输出之前
        let sink_exit = self.sink.clone();
        std::thread::spawn(move || {
            // portable-pty 0.9 的 ExitStatus::exit_code() 是 u32;事件契约用 i32
            // (位宽转换保留原 bit pattern,Windows NTSTATUS 呈现为惯用的负值)
            let code = match child.wait() {
                Ok(status) => status.exit_code() as i32,
                Err(_) => -1,
            };
            let _ = batcher.join();
            sink_exit(SessionEvent::Exit { id, code });
        });

        self.sessions
            .lock()
            .expect("会话表锁被毒化")
            .insert(id, session);
        Ok(id)
    }

    pub fn send_input(&self, id: SessionId, data: &str) -> Result<(), NexusError> {
        // lookup 返回的是整表 guard,必须先 get 再调方法(链式写法过不了借用检查)
        let guard = self.lookup(id)?;
        let sess = guard
            .get(&id)
            .ok_or_else(|| NexusError::SessionNotFound(id.to_string()))?;
        sess.write_all(data.as_bytes())
    }

    pub fn resize(&self, id: SessionId, cols: u16, rows: u16) -> Result<(), NexusError> {
        let guard = self.lookup(id)?;
        let sess = guard
            .get(&id)
            .ok_or_else(|| NexusError::SessionNotFound(id.to_string()))?;
        sess.resize(cols, rows)
    }

    /// 停止会话:M1 一律强杀(killer.kill)。
    /// force=false 的优雅关停(先发 Ctrl-C、宽限、再杀)留给 M2 的 CancellationToken。
    pub fn stop(&self, id: SessionId) -> Result<(), NexusError> {
        let session = self.sessions.lock().expect("会话表锁被毒化").remove(&id);
        match session {
            Some(s) => {
                // 上游 portable-pty 0.9.0 WinChildKiller::kill 成败判定反转(见 ledger P6):
                // kill 结果不可信,退出的真相以 wait 线程的 Exit 事件为准
                let _ = s.kill();
                Ok(())
            }
            None => Err(NexusError::SessionNotFound(id.to_string())),
        }
    }

    fn lookup(
        &self,
        id: SessionId,
    ) -> Result<MutexGuard<'_, HashMap<SessionId, PtySession>>, NexusError> {
        let guard = self.sessions.lock().expect("会话表锁被毒化");
        if guard.contains_key(&id) {
            Ok(guard)
        } else {
            Err(NexusError::SessionNotFound(id.to_string()))
        }
    }
}
