//! tokio 概念验证(spec 风险 #1 缓解):在重构主线前,亲手感受
//! 有界通道背压 / timeout / CancellationToken / spawn_blocking 四件事。
//! 运行:cargo run --example tokio_basics
use std::time::Duration;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() {
    // 1) 有界通道 + send().await 背压:容量 2,生产 5 条,消费前 2 条后停 200ms
    let (tx, mut rx) = mpsc::channel::<u32>(2);
    let producer = tokio::spawn(async move {
        for i in 0..5u32 {
            // 队列满时这里挂起——生产者被消费速度约束,这就是背压
            tx.send(i).await.expect("consumer alive");
            println!("produced {i}");
        }
    });
    let mut got = Vec::new();
    for _ in 0..2 {
        got.push(rx.recv().await.unwrap());
    }
    // 已消费 2 条释放了 2 个 permit:暂停期内 send(2)/send(3) 可完成入队,
    // 真正被背压挂起的是 send(4)——直到 drain 恢复消费(2026-09-10 审查勘误)
    println!("consumed {:?}, pausing (producer parks on send #4)", got);
    tokio::time::sleep(Duration::from_millis(200)).await;
    while let Some(v) = rx.recv().await {
        got.push(v);
    }
    producer.await.unwrap();
    assert_eq!(got, vec![0, 1, 2, 3, 4]);

    // 2) timeout:给慢 await 加期限,超时返回 Err 而不是永远等
    let slow = tokio::time::sleep(Duration::from_secs(10));
    let r = tokio::time::timeout(Duration::from_millis(50), slow).await;
    assert!(r.is_err(), "10s 的 sleep 在 50ms 处被打断");

    // 3) CancellationToken:select! 里正常工作与关停信号并排
    let token = CancellationToken::new();
    let child = token.clone();
    let worker = tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_millis(50)) => println!("tick"),
                _ = child.cancelled() => {
                    println!("cancelled, draining");
                    return;
                }
            }
        }
    });
    tokio::time::sleep(Duration::from_millis(120)).await;
    token.cancel();
    worker.await.unwrap();

    // 4) spawn_blocking:阻塞 IO 的桥(打印线程名证明不在 async 线程)
    let pid = tokio::task::spawn_blocking(|| {
        println!("blocking pool thread: {:?}", std::thread::current().id());
        std::process::id()
    })
    .await
    .unwrap();
    println!("pid {pid} — all four concepts verified");
}
