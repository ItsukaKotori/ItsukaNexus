// 合帧:reader 线程的 8KB 原始块 → UI 可消化的帧(spec §1.3 背压链中间级)。
// 纯函数设计:不拥有线程、不碰 Tauri,时间行为可被单元测试钉死。
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

/// 时间窗:第一块到手后,窗口内的后续块并进同一帧
pub const FRAME_WINDOW: Duration = Duration::from_millis(16);
/// 单帧大小上限:达到即交帧(帧可略超——数据流不可截断,见测试 stops_at_max_bytes)
pub const FRAME_MAX_BYTES: usize = 32 * 1024;

/// 从通道取一帧。阻塞等第一块;窗口内继续并块;通道关闭且无数据 → None(调用方线程退出)。
pub fn next_frame(rx: &Receiver<Vec<u8>>, window: Duration, max_bytes: usize) -> Option<Vec<u8>> {
    let mut frame = rx.recv().ok()?;
    let deadline = Instant::now() + window;
    loop {
        if frame.len() >= max_bytes {
            break;
        }
        let now = Instant::now();
        if now >= deadline {
            break;
        }
        match rx.recv_timeout(deadline - now) {
            Ok(chunk) => frame.extend_from_slice(&chunk),
            Err(_) => break, // 超时或通道关闭:把手头的交出去
        }
    }
    Some(frame)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn coalesces_rapid_chunks_into_one_frame() {
        let (tx, rx) = mpsc::sync_channel::<Vec<u8>>(64);
        for i in 0..3 {
            tx.send(format!("chunk-{i}").into_bytes()).unwrap();
        }
        drop(tx); // 发完了
        let frame = next_frame(&rx, Duration::from_millis(16), 32 * 1024).unwrap();
        let text = String::from_utf8_lossy(&frame).into_owned();
        assert_eq!(text, "chunk-0chunk-1chunk-2");
    }

    #[test]
    fn returns_none_when_channel_closed_and_empty() {
        let (tx, rx) = mpsc::sync_channel::<Vec<u8>>(64);
        drop(tx);
        assert!(next_frame(&rx, Duration::from_millis(16), 32 * 1024).is_none());
    }

    #[test]
    fn stops_at_max_bytes() {
        let (tx, rx) = mpsc::sync_channel::<Vec<u8>>(64);
        let big = vec![b'x'; 40 * 1024]; // 40KB,单块即超 32KB 上限
        tx.send(big).unwrap();
        drop(tx);
        let frame = next_frame(&rx, Duration::from_millis(16), 32 * 1024).unwrap();
        assert!(
            frame.len() >= 40 * 1024,
            "超限块不截断数据(帧可略超上限,保流完整性)"
        );
    }

    #[test]
    fn slow_producer_expires_window_with_what_it_has() {
        // 最小偏差(相对 brief 逐字版):首对通道的 rx 未使用 → _rx;tx 已被 move 进
        // 子线程,函数尾 drop(tx) 是 E0382 硬错误 → 删除(tx 随子线程退出释放,语义不变)。
        let (tx, _rx) = mpsc::sync_channel::<Vec<u8>>(64);
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(50)); // 比窗口慢
            let _ = tx.send(b"late".to_vec());
        });
        let start = std::time::Instant::now();
        // 先塞一块"开场"数据,让 next_frame 进入窗口期
        let (tx2, rx2) = mpsc::sync_channel::<Vec<u8>>(64);
        tx2.send(b"first".to_vec()).unwrap();
        drop(tx2);
        let frame = next_frame(&rx2, Duration::from_millis(10), 32 * 1024).unwrap();
        assert_eq!(frame, b"first".to_vec());
        assert!(
            start.elapsed() < Duration::from_millis(45),
            "窗口到期要立刻返回,不能陪慢生产者等"
        );
    }
}
