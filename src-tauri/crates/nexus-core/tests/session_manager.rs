// SessionManager 集成测试:真实 spawn 默认 shell。
// M2 编排重写后的事件缝:sink 只承载低频 State/Exit;Output 走 per-session
// subscribe(带 replay)。交互回显断言只在 unix 可靠(windows 的 powershell
// 交互编码是 M2 议题),windows CI 覆盖靠 Task 2 的 pty_session 一次性测试。
#![cfg(unix)]

mod common;

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use common::{deadline, manager_with_channel, next_event, wait_exit, wait_frame_contains, EventRx};
use nexus_core::agent::manager::{LaunchSpec, SessionEvent, Subscription};
use nexus_core::agent::state::SessionState;
use nexus_core::error::NexusError;
use nexus_core::ids::SessionId;

/// 总 deadline 内等 Exit 事件(有界等待:总 deadline + 分片超时聚合)。
/// 途中 State 等低频事件跳过,继续等 Exit;超时/通道关闭返回 None,
/// 由调用方 expect 承接"Exit 必达"断言(stop 语义不得弱化为超时兜底)。
async fn recv_with_deadline(rx: &mut EventRx, total: Duration) -> Option<SessionEvent> {
    let dl = tokio::time::Instant::now() + total;
    loop {
        let now = tokio::time::Instant::now();
        if now >= dl {
            return None;
        }
        match tokio::time::timeout(dl - now, rx.recv()).await {
            Ok(Some(ev)) => match ev {
                SessionEvent::Exit { .. } => return Some(ev),
                _ => continue, // State 等低频事件:跳过
            },
            Ok(None) => return None, // 事件通道意外关闭
            Err(_) => return None,   // 总 deadline 到
        }
    }
}

/// drain 事件流直到 id 的 Exit(复用 wait_exit 的有界等待:超时即 panic,
/// 断言不弱化);退出码由调用方按场景自行断言
async fn drain_until_exit(rx: &mut EventRx, id: SessionId) {
    wait_exit(rx, id).await;
}

#[tokio::test]
async fn create_shell_send_input_sees_echo() {
    let (mgr, _rx) = manager_with_channel();
    let id = mgr
        .create("shell", 80, 24, None)
        .await
        .expect("create 失败");
    let sub = mgr.subscribe(id).expect("subscribe 失败");
    let mut frames = sub.rx;

    // shell 就绪时间不定:输入停在 PTY 缓冲,shell 起来后照常读到并回显
    mgr.send_input(id, "echo marker-manager-42\n")
        .await
        .expect("send 失败");
    wait_frame_contains(&mut frames, "marker-manager-42").await;
}

#[tokio::test]
async fn stop_kills_session_and_emits_exit() {
    let (mgr, mut rx) = manager_with_channel();
    let id = mgr
        .create("shell", 80, 24, None)
        .await
        .expect("create 失败");
    mgr.stop(id, true).await.expect("stop 失败");

    let code = wait_exit(&mut rx, id).await;
    assert_ne!(code, 0, "被 kill 的 shell 退出码应非 0");

    // M2:退出后条目保留;状态写回先于 Exit 事件,收到 Exit 即可见终态
    let snap = mgr
        .list()
        .into_iter()
        .find(|s| s.session_id == id)
        .expect("退出后条目必须保留");
    assert_eq!(snap.state, SessionState::Exited);

    // 条目还在但不在 Running:明确报"未在运行",而非"不存在"
    let err = mgr.send_input(id, "should fail\n").await.unwrap_err();
    assert!(matches!(err, NexusError::SessionNotRunning(_)));
}

#[tokio::test]
async fn natural_exit_emits_zero_code_and_snapshot() {
    // 自然退出:钉住"join 输出管线后再发 Exit"的不变量 + 退出后快照可查
    let (mgr, mut rx) = manager_with_channel();
    let id = mgr
        .create("shell", 80, 24, None)
        .await
        .expect("create 失败");

    // shell 尚未就绪也没关系:输入停在 PTY 缓冲,shell 起来后照常读到
    mgr.send_input(id, "exit\n").await.expect("send 失败");

    let code = wait_exit(&mut rx, id).await;
    assert_eq!(code, 0, "自然退出的 shell 退出码应为 0");

    let snap = mgr
        .list()
        .into_iter()
        .find(|s| s.session_id == id)
        .expect("退出后条目必须保留");
    assert_eq!(snap.state, SessionState::Exited);
    assert_eq!(snap.exit_code, Some(0));
}

#[tokio::test]
async fn create_rejects_unknown_provider() {
    let (mgr, _rx) = manager_with_channel();
    let err = mgr.create("claude", 80, 24, None).await.unwrap_err();
    assert!(matches!(err, NexusError::UnsupportedProvider(_)));
}

#[tokio::test]
async fn two_sessions_are_independent() {
    let (mgr, _rx) = manager_with_channel();
    let a = mgr.create("shell", 80, 24, None).await.unwrap();
    let b = mgr.create("shell", 80, 24, None).await.unwrap();
    assert_ne!(a, b);

    let sub = mgr.subscribe(b).expect("subscribe b 失败");
    let mut frames = sub.rx;
    mgr.send_input(b, "echo only-in-b\n").await.unwrap();
    wait_frame_contains(&mut frames, "only-in-b").await;

    mgr.stop(a, true).await.expect("stop a 失败");
    mgr.stop(b, true).await.expect("stop b 失败");
}

#[tokio::test]
async fn create_emits_state_running_before_any_output() {
    let (mgr, mut rx) = manager_with_channel();
    let id = mgr
        .create("shell", 80, 24, None)
        .await
        .expect("create 失败");

    // create 返回前 State{Running} 已发出:sink 队列的第一条必是它
    match next_event(&mut rx, deadline()).await {
        SessionEvent::State(change) => {
            assert_eq!(change.session_id, id);
            assert_eq!(change.next, SessionState::Running);
        }
        other => panic!("首个事件应是 State(Running),实际 {other:?}"),
    }

    // 之后的输出经订阅通道到达(Output 不走全局 sink)
    let sub = mgr.subscribe(id).expect("subscribe 失败");
    let mut frames = sub.rx;
    mgr.send_input(id, "echo after-state-1\n")
        .await
        .expect("send 失败");
    wait_frame_contains(&mut frames, "after-state-1").await;
}

#[tokio::test]
async fn subscribe_replays_history_and_keeps_seq_monotonic() {
    let (mgr, _rx) = manager_with_channel();
    let id = mgr
        .create("shell", 80, 24, None)
        .await
        .expect("create 失败");

    // 先订阅、等历史输出落地,再换订阅验证 replay
    let first = mgr.subscribe(id).expect("subscribe 失败");
    let mut first_rx = first.rx;
    mgr.send_input(id, "echo marker-sub-77\n")
        .await
        .expect("send 失败");
    let seen = wait_frame_contains(&mut first_rx, "marker-sub-77").await;
    for pair in seen.windows(2) {
        assert!(pair[0].seq < pair[1].seq, "订阅内 seq 必须严格递增");
    }

    // 重复 subscribe 替换旧订阅:旧通道在存量耗尽后关闭
    let mut second = mgr.subscribe(id).expect("re-subscribe 失败");
    assert!(
        second.replay.contains("marker-sub-77"),
        "replay 应包含订阅前的输出,实际 {:?}",
        second.replay
    );
    // 旧通道 drain 至关闭:负载下 marker 之后的提示符帧可能仍在旧通道,
    // 单次 recv 断言 None 会 flake;deadline 内循环收完存量,通道终将关闭
    // (发送端已被替换,克隆随发送完成耗尽),存量帧内容不 assert
    let dl = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let now = tokio::time::Instant::now();
        assert!(now < dl, "5 秒内旧订阅通道未关闭");
        match tokio::time::timeout(dl - now, first_rx.recv()).await {
            Ok(Some(_leftover)) => {}
            Ok(None) => break, // 发送端已替换:通道关闭,断言成立
            Err(_) => panic!("5 秒内旧订阅通道未关闭"),
        }
    }

    // 后续输出经新通道到达,seq 跨订阅单调不回退
    mgr.send_input(id, "echo after-replay-88\n")
        .await
        .expect("send 失败");
    let tail = wait_frame_contains(&mut second.rx, "after-replay-88").await;
    for pair in tail.windows(2) {
        assert!(pair[0].seq < pair[1].seq, "新订阅内 seq 必须严格递增");
    }
    let last_before = seen.last().expect("至少一帧").seq;
    if let Some(f) = tail.first() {
        assert!(f.seq >= last_before, "seq 必须跨订阅单调不回退");
    }
}

/// M2 完成标准②:慢消费者背压。订阅通道(SUBSCRIBER_DEPTH=128)不被消费
/// → batcher 挂在 send → raw 通道(64×8KB)充满 → reader 的 blocking_send
/// 阻塞 → PTY 缓冲顶住子进程:数据停滞但不丢、内存有界;恢复消费后全部行
/// 完整到达。输出量必须超过订阅+raw 两条队列的总吸收量(≈4MB+512KB),否则
/// 背压链根本不会传导——`seq 1 20000` 仅 ~110KB 会被订阅通道整体吸收,故用
/// awk 在每行数字后垫一条 512B 的 x 行,把总量拉到 ~10MB。失败上界最坏
/// ~60s(wait_stall 30s + drain 30s),超出文件内 10s 惯例,为背压场景有意为之。
#[tokio::test]
async fn slow_consumer_applies_backpressure_without_loss() {
    let (mgr, mut sink_rx) = manager_with_channel();
    let id = mgr
        .create("shell", 200, 50, None)
        .await
        .expect("create 失败");
    let sub = mgr.subscribe(id).expect("subscribe 失败");
    let mut frames = sub.rx; // 持有而不消费:制造慢消费者

    // 20000 行数字(断言对象)+ 每行一条 512B x 垫行 ≈ 10MB;首尾各补一条
    // 空行:zsh 的 ZLE 重绘提示符(`%` + 右提示符填充空格)不带行尾换行,
    // 会与紧随其后的第一行输出粘成一行,空行替数字行挡掉这种粘连;
    // `</dev/null` 让 BEGIN-only 的 awk 不回头读 PTY stdin(否则会吞掉
    // 后续输入并挂住)
    mgr.send_input(
        id,
        "awk 'BEGIN{print \"\";p=\"x\";for(j=0;j<9;j++)p=p p;for(i=1;i<=20000;i++){print i;print p}} END{print \"\"}' </dev/null\n",
    )
    .await
    .expect("send 失败");

    // 暂停消费,等背压传导到位:订阅通道充满(batcher 挂在 send)是链路生效
    // 的稳定标志——充满后恢复消费前不会排空(单调),轮询等待不引入 flake
    let wait_stall = async {
        let dl = tokio::time::Instant::now() + Duration::from_secs(30);
        while frames.capacity() > 0 {
            assert!(
                tokio::time::Instant::now() < dl,
                "30 秒内订阅通道未充满,背压链未生效(remaining={})",
                frames.capacity()
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    };
    wait_stall.await;
    // 停滞窗口内(通道确定全满、恢复消费前)验证管理面不被阻塞(M1 遗留 #5
    // 的行为钉住:管理面只碰快照表锁,不随输出管线一起排队)
    let list_res = tokio::time::timeout(Duration::from_secs(2), async { mgr.list().len() }).await;
    assert!(list_res.is_ok(), "list() 不得被慢消费者阻塞(2s 内未完成)");
    assert_eq!(list_res.unwrap(), 1, "表中应恰有当前会话一条");

    // 恢复消费:单次 500ms 超时循环,总量 deadline 30s(CI 慢时有余量);
    // 空闲节流(500ms 无帧)才对已拼接整体重扫判定完备、凑满 20000 提前收工
    // ——数字行可能被 frame 边界切开,逐帧解析会永不满而空转到 deadline
    let dl = tokio::time::Instant::now() + Duration::from_secs(30);
    let mut collected = String::new();
    while tokio::time::Instant::now() < dl {
        match tokio::time::timeout(Duration::from_millis(500), frames.recv()).await {
            Ok(Some(frame)) => collected.push_str(&frame.data),
            Ok(None) => break, // 通道关闭(会话结束):转终断言
            Err(_) => {
                // 对已拼接整体重扫(PTY 输出侧 \n→\r\n,先剥 \r 再解析)
                let full: HashSet<u32> = collected
                    .split('\n')
                    .map(|l| l.trim_end_matches('\r'))
                    .filter_map(|l| l.parse::<u32>().ok())
                    .filter(|n| (1..=20000).contains(n))
                    .collect();
                if full.len() == 20000 {
                    break;
                }
            }
        }
    }

    // 完整性:拼接流按 '\n' 收进 HashSet,O(1) 存在性查找(禁 O(n²) contains);
    // 行尾 \r 统一剥掉再比。提示符/回显噪声行不影响纯数字行的存在性断言
    let lines: HashSet<&str> = collected
        .split('\n')
        .map(|l| l.trim_end_matches('\r'))
        .collect();
    let missing: Vec<u32> = (1..=20000)
        .filter(|n| !lines.contains(n.to_string().as_str()))
        .collect();
    assert!(
        missing.is_empty(),
        "背压恢复后仍丢失 {} 行,例如 {:?}",
        missing.len(),
        &missing[..missing.len().min(10)]
    );

    // 收尾:强杀 + 等 Exit(防泄漏;强杀后 batcher 经 cancel 分支自行收尾)
    mgr.stop(id, true).await.expect("stop 失败");
    let code = wait_exit(&mut sink_rx, id).await;
    assert_ne!(code, 0, "被强杀的 shell 退出码应非 0");
}

/// I-1 钉住测试:replay/live 接缝原子化(replay 与实时流不重复不丢失)。
/// 定位是概率性回归绊线,不是确定性复现:单次 subscribe 的交错窗口小,机制
/// 正确性由终审的交错代数证明(batcher 的 push+clone 与 subscribe 的
/// snapshot+替换以 subscriber→replay 锁序配对成互斥临界段后,接缝退化为
/// 二选一);这里靠 4 路 × 300 次重订阅在帧级接缝上反复踩线,"重复帧出现即红"。
/// 必须多线程 runtime:交错需要真并行(生产侧 tauri 是多线程 runtime),
/// 单线程 runtime 里同步临界段互不穿插,踩不到接缝竞态。数据量为 300 万行
/// 唯一 marker(~36MB):喂饱整条管线让 batcher 在整个重订阅窗口持续出帧,
/// 并把 replay 顶到 256KB 上限。最坏耗时 ~35s(流动监督 20s 上限 + 1s 空闲
/// 判定 + 收尾自然退出 10s 上限),典型 ~5-8s。
/// 收尾用自然退出而非强杀:上游 clone_killer 只发 SIGHUP(无 SIGKILL 兜底),
/// 前台作业运行中的正忙 shell 不理会,stop 后 Exit 永不到达、状态卡 Stopping
/// ——main 上的既有问题,归 M3 进程组 kill;本测试等流停(shell 回到空闲
/// 提示符)再送 exit 避开。
#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn attach_seam_never_duplicates() {
    const HAMMERS: usize = 4;
    const RESUBSCRIBES: usize = 300;

    let (mgr, mut rx) = manager_with_channel();
    let mgr = Arc::new(mgr);
    let id = mgr
        .create("shell", 200, 50, None)
        .await
        .expect("create 失败");

    // 300 万行唯一 marker(~36MB):喂饱整条管线,让 batcher 在整个重订阅
    // 窗口内持续出帧,并把 replay 顶到 256KB 上限;`</dev/null` 防
    // BEGIN-only awk 回读 PTY stdin 挂住(同背压测试)
    mgr.send_input(
        id,
        "awk 'BEGIN{for(i=1;i<=3000000;i++)print \"SEAM-\" i}' </dev/null\n",
    )
    .await
    .expect("send 失败");

    // 等输出真正开始流动(shell 启动时间不计入重订阅窗口;严格整行匹配
    // marker,命令回显里的 awk 源码文本不算数)
    'flow: {
        let first = mgr.subscribe(id).expect("subscribe 失败");
        let mut first_rx = first.rx;
        let dl = deadline();
        while tokio::time::Instant::now() < dl {
            let now = tokio::time::Instant::now();
            let frame = tokio::time::timeout(dl - now, first_rx.recv())
                .await
                .expect("10 秒内未等到首帧输出")
                .expect("订阅通道意外关闭");
            if frame.data.split('\n').any(|l| {
                l.trim_end_matches('\r')
                    .strip_prefix("SEAM-")
                    .is_some_and(|r| r.parse::<u64>().is_ok())
            }) {
                break 'flow;
            }
        }
        panic!("10 秒内未等到首个 marker 行");
    }

    // 流结束旗标(监督任务):已见帧且计数 1s 无增长 = awk 退场、shell 回到
    // 空闲提示符,此时才可安全送 exit 自然退出(见函数头注释);20s 硬上限兜底
    let frames_seen = Arc::new(AtomicU64::new(0));
    let flow_done = Arc::new(AtomicBool::new(false));
    {
        let frames_seen = Arc::clone(&frames_seen);
        let flow_done = Arc::clone(&flow_done);
        tokio::task::spawn(async move {
            let dl = tokio::time::Instant::now() + Duration::from_secs(20);
            let mut last = 0u64;
            let mut idle_ticks = 0u32;
            while tokio::time::Instant::now() < dl {
                tokio::time::sleep(Duration::from_millis(100)).await;
                let now = frames_seen.load(Ordering::Relaxed);
                if now == 0 {
                    continue; // 还没开始流动
                }
                if now == last {
                    idle_ticks += 1;
                    if idle_ticks >= 10 {
                        break; // 1s 无新帧:流结束
                    }
                } else {
                    idle_ticks = 0;
                    last = now;
                }
            }
            flow_done.store(true, Ordering::Relaxed);
        });
    }

    // 重订阅 hammer × HAMMERS:每路 RESUBSCRIBES 次"订阅 → 30ms 内收 1 帧实时
    // → 就地丢弃 → replay+live 拼接查重"。消费速率天然被生产速率限流,recv
    // 等帧的时间正是 batcher 出帧(=接缝)的时间;多路并发互相替换订阅,
    // 接缝密度被反复踩
    let mut hammers = Vec::new();
    for hammer in 0..HAMMERS {
        let mgr = Arc::clone(&mgr);
        let frames_seen = Arc::clone(&frames_seen);
        hammers.push(tokio::spawn(async move {
            for iter in 0..RESUBSCRIBES {
                let Subscription {
                    replay,
                    rx: mut sub_rx,
                } = mgr.subscribe(id).expect("subscribe 失败");
                // 首批实时帧:收 1 帧即可判定接缝,至多等 30ms(空闲兜底)
                let mut live = String::new();
                if let Ok(Some(frame)) =
                    tokio::time::timeout(Duration::from_millis(30), sub_rx.recv()).await
                {
                    live.push_str(&frame.data);
                    frames_seen.fetch_add(1, Ordering::Relaxed);
                }
                drop(sub_rx); // 旧订阅就地丢弃:下一轮替换语义与真实前端一致

                let joined = format!("{replay}{live}");
                // 只比对完整行:帧界落在任意字节,replay 尾/live 尾都可能停在
                // 某行中间,半截数字("SEAM-5876" 被切成 "SEAM-58")会假阳性
                // 命中更早的真实行号,截到最后一个 '\n' 消除;真接缝重复是
                // 完整行在拼接里出现两次,不受截断影响。严格整行匹配下,命令
                // 回显/提示符噪声行(含 awk 源码)不会命中;帧界切开处由拼接
                // 顺序自然还原
                let complete = match joined.rfind('\n') {
                    Some(i) => &joined[..=i],
                    None => "",
                };
                let mut seen = HashSet::new();
                let mut dups = Vec::new();
                for line in complete.split('\n').map(|l| l.trim_end_matches('\r')) {
                    let Some(n) = line
                        .strip_prefix("SEAM-")
                        .and_then(|r| r.parse::<u64>().ok())
                    else {
                        continue;
                    };
                    if !seen.insert(n) {
                        dups.push(n);
                    }
                }
                if !dups.is_empty() {
                    return Err(format!(
                        "hammer {hammer} 第 {iter} 次订阅:replay+live 拼接出现重复帧\
                         (接缝必须原子化,不重复不丢失): {:?}",
                        &dups[..dups.len().min(8)]
                    ));
                }
            }
            Ok(())
        }));
    }
    for hammer in hammers {
        hammer
            .await
            .expect("重订阅任务失败")
            .expect("接缝检出重复帧");
    }

    // 等流动结束确认(hammer 收满 300 次时流可能尚有余量;上限兜底)
    let dl = tokio::time::Instant::now() + Duration::from_secs(25);
    while !flow_done.load(Ordering::Relaxed) {
        assert!(
            tokio::time::Instant::now() < dl,
            "25 秒内输出流未结束,无法安全收尾"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    // 收尾:自然退出(见函数头注释,SIGHUP 对正忙 shell 不足归 M3),等 Exit
    // 并断言退出码 0,防会话/任务树泄漏
    mgr.send_input(id, "exit\n").await.expect("send 失败");
    let code = wait_exit(&mut rx, id).await;
    assert_eq!(code, 0, "自然退出的 shell 退出码应为 0");
}

/// 必办#1-a:孙进程持 slave fd 时,force stop 仍应让 Exit 事件及时到达
/// (不靠 JOIN_TIMEOUT 超时兜底,detail 不得出现 stalled)。
#[tokio::test]
async fn force_stop_reaches_exit_with_grandchild_holding_slave() {
    let (mgr, mut sink_rx) = manager_with_channel();
    let id = mgr.create("shell", 120, 30, None).await.unwrap();
    // 后台孙进程:sleep 持有 slave fd,shell 退出后它还活着
    mgr.send_input(id, "sleep 1000 &\n").await.unwrap();
    tokio::time::sleep(Duration::from_millis(800)).await;
    mgr.stop(id, true).await.unwrap();
    // Exit 必须在 JOIN_TIMEOUT(5s) 的正常路径内到达:给 4s 上限,留余量
    let ev = recv_with_deadline(&mut sink_rx, Duration::from_secs(4))
        .await
        .expect("Exit 事件应及时到达");
    match ev {
        SessionEvent::Exit { session_id, code } => {
            assert_eq!(session_id, id);
            assert_ne!(code, 0, "强杀的退出码应非 0");
        }
        other => panic!("期望 Exit 事件,得到 {other:?}"),
    }
    // 状态应已迁移到终态(list 可见)
    let snap = mgr.list().into_iter().find(|s| s.session_id == id).unwrap();
    assert!(matches!(
        snap.state,
        nexus_core::agent::state::SessionState::Exited
            | nexus_core::agent::state::SessionState::Failed
    ));
}

/// 必办#1-b:kill 正忙 shell(前台死循环 job 在独立进程组)——M2 实测
/// SIGHUP 不足导致 Exit 永不到达;killpg + drop master 后必须收敛。
#[tokio::test]
async fn force_stop_kills_busy_shell_foreground_job() {
    let (mgr, mut sink_rx) = manager_with_channel();
    let id = mgr.create("shell", 120, 30, None).await.unwrap();
    mgr.send_input(id, "while :; do :; done\n").await.unwrap();
    tokio::time::sleep(Duration::from_millis(800)).await; // 等 job 进前台
    mgr.stop(id, true).await.unwrap();
    let ev = recv_with_deadline(&mut sink_rx, Duration::from_secs(4))
        .await
        .expect("busy shell 场景 Exit 也必须到达");
    assert!(matches!(ev, SessionEvent::Exit { .. }));
}

/// 必办#2:dispose 关 tab 即删——仅终态(Exited/Failed)可删、运行中拒绝、
/// 删后条目消失(后续 send_input/dispose 报 SessionNotFound)
#[tokio::test]
async fn dispose_removes_terminal_session_only() {
    let (mgr, mut sink_rx) = manager_with_channel();
    let id = mgr.create("shell", 80, 24, None).await.unwrap();
    assert!(
        matches!(mgr.dispose(id), Err(NexusError::SessionNotRunning(_))),
        "运行中的会话不可删"
    );
    // 自然退出
    mgr.send_input(id, "exit\n").await.unwrap();
    drain_until_exit(&mut sink_rx, id).await;
    mgr.dispose(id).unwrap();
    assert!(mgr.list().is_empty(), "dispose 后 list 应为空");
    assert!(
        matches!(
            mgr.send_input(id, "x").await,
            Err(NexusError::SessionNotFound(_))
        ),
        "dispose 后 send_input 应报 SessionNotFound"
    );
    assert!(
        matches!(mgr.dispose(id), Err(NexusError::SessionNotFound(_))),
        "dispose 后再 dispose 应报 SessionNotFound"
    );
}

/// 必办#3-a:list 按 startedAtMs 升序(刷新后 tab 序稳定)。HashMap 无序,
/// 不排序时三条快照的相对次序随每次运行漂移;5ms 间隔保证 started_at_ms
/// 严格递增,逐位断言 snaps[0]=a、snaps[1]=b、snaps[2]=c
#[tokio::test]
async fn list_is_sorted_by_started_at() {
    let (mgr, _sink_rx) = manager_with_channel();
    let a = mgr.create("shell", 80, 24, None).await.unwrap();
    tokio::time::sleep(Duration::from_millis(5)).await;
    let b = mgr.create("shell", 80, 24, None).await.unwrap();
    tokio::time::sleep(Duration::from_millis(5)).await;
    let c = mgr.create("shell", 80, 24, None).await.unwrap();
    let snaps = mgr.list();
    assert_eq!(snaps.len(), 3, "应恰有当前三个会话");
    let times: Vec<u64> = snaps.iter().map(|s| s.started_at_ms).collect();
    let mut sorted = times.clone();
    sorted.sort_unstable();
    assert_eq!(times, sorted, "list 必须按 startedAtMs 升序");
    assert_eq!(snaps[0].session_id, a, "最早的会话排最前");
    assert_eq!(snaps[1].session_id, b, "中间建的排中间");
    assert_eq!(snaps[2].session_id, c, "最晚的会话排最后(修正断言)");
}

/// 必办#3-b(集成面):巨量输入写给不读输入的子进程,send_input 必须有界
/// 返回、绝不挂死 invoke。就绪判定用"算术 marker"(`$((41+1))` → 输出
/// READY-42)而非固定 sleep:输入回显是逐字的命令原文(不含 READY-42),
/// 匹配到 READY-42 即证明 ZLE 已读走并执行了命令行;`exec sleep 1000` 后
/// shell 已被替换、前台 sleep 不读 stdin——此后 tty 输入队列再无读者。
/// 结果值不断言 Err:macOS xnu 不因输入队列满阻塞 master 写(实测 16MB
/// 即时成功),只有 Linux n_tty 阻塞写端走 2s 超时报错——错误路径由
/// manager 的 NeverWriter 单测钉住,这里钉平台无关的"及时返回"属性
#[tokio::test]
async fn send_input_to_never_reading_child_returns_promptly() {
    let (mgr, mut sink_rx) = manager_with_channel();
    let id = mgr.create("shell", 80, 24, None).await.unwrap();
    let sub = mgr.subscribe(id).expect("subscribe 失败");
    let mut frames = sub.rx;
    mgr.send_input(id, "echo READY-$((41+1)) && exec sleep 1000\n")
        .await
        .unwrap();
    wait_frame_contains(&mut frames, "READY-42").await;
    drop(frames); // 就绪后不再消费:避免后续输出(若有)把订阅通道灌满
    let big = "x".repeat(512 * 1024); // 512KB,远超 PTY 输入缓冲
    let start = std::time::Instant::now();
    let r = mgr.send_input(id, &big).await;
    assert!(
        start.elapsed() < Duration::from_secs(6),
        "必须及时返回,不能挂死"
    );
    // Ok(macOS 吸收输入)与 Err(Linux 写超时)都是合法结局
    let _ = r;
    // 收尾:强杀 + 等 Exit(防泄漏;被超时弃置的阻塞写线程随进程退出兜底)
    mgr.stop(id, true).await.unwrap();
    let code = wait_exit(&mut sink_rx, id).await;
    assert_ne!(code, 0, "被强杀的 shell 退出码应非 0");
}

/// 必办#2:终态会话 subscribe——replay 照常重放历史,实时流 rx 立即关闭
/// (已关闭 channel:drop 发送端,IPC 转发任务 recv 即 None 自然收尾不泄漏)
#[tokio::test]
async fn subscribe_terminal_session_replays_history_and_closes_stream() {
    let (mgr, mut sink_rx) = manager_with_channel();
    let id = mgr.create("shell", 80, 24, None).await.unwrap();
    mgr.send_input(id, "echo marker-term-sub-9\n")
        .await
        .unwrap();
    mgr.send_input(id, "exit\n").await.unwrap();
    drain_until_exit(&mut sink_rx, id).await;

    let sub = mgr.subscribe(id).expect("终态 subscribe 应成功");
    assert!(
        sub.replay.contains("marker-term-sub-9"),
        "终态 replay 应照常重放历史,实际 {:?}",
        sub.replay
    );
    // rx 已关闭:recv 立即返回 None(mpsc 优雅关闭语义;500ms 上限防挂)
    let mut rx = sub.rx;
    let closed = tokio::time::timeout(Duration::from_millis(500), rx.recv()).await;
    assert!(
        matches!(closed, Ok(None)),
        "终态订阅的实时流必须立即关闭,实际 {closed:?}"
    );
}

/// M3 Task 9:LaunchSpec 的 cwd 落点——子进程必须真的 spawn 在给定目录里
/// (worktree 集成的全部秘密就是 CommandBuilder 的那一行 cwd)。launch 未带
/// repo_path/worktree_name 时快照两字段为 None(纯 shell 落点)。
#[tokio::test]
async fn create_with_cwd_spawns_in_worktree_like_dir() {
    let (mgr, mut sink_rx) = manager_with_channel();
    let dir = tempfile::tempdir().unwrap();
    let spec = LaunchSpec {
        cwd: dir.path().to_path_buf(),
        repo_path: None,
        worktree_name: None,
    };
    let id = mgr.create("shell", 80, 24, Some(&spec)).await.unwrap();
    let sub = mgr.subscribe(id).expect("subscribe 失败");
    let mut frames = sub.rx;

    // shell 就绪时间不定:输入停在 PTY 缓冲,起来后照常读到;canonicalize
    // 两侧归一(macOS /var ↔ /private/var)
    mgr.send_input(id, "pwd\n").await.expect("send 失败");
    let expected = dir
        .path()
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    wait_frame_contains(&mut frames, &expected).await;

    let snap = mgr
        .list()
        .into_iter()
        .find(|s| s.session_id == id)
        .expect("会话应在表中");
    assert!(
        snap.repo_path.is_none() && snap.worktree_name.is_none(),
        "launch 未带 repo/worktree 时快照两字段应为 None,实际 {snap:?}"
    );

    // 收尾:强杀 + 等 Exit(防泄漏)
    mgr.stop(id, true).await.expect("stop 失败");
    let code = wait_exit(&mut sink_rx, id).await;
    assert_ne!(code, 0, "被强杀的 shell 退出码应非 0");
}
