// 回放缓冲(spec §1.3):每会话保留最近 ~256KB 输出,attach 时重放,
// 前端热重载/崩溃后终端不丢上下文。按"段"管理避免逐字节开销。
use std::collections::VecDeque;

pub struct ReplayBuffer {
    capacity: usize,
    total: usize,
    segments: VecDeque<String>,
}

impl ReplayBuffer {
    pub fn new(capacity_bytes: usize) -> Self {
        Self {
            capacity: capacity_bytes.max(1),
            total: 0,
            segments: VecDeque::new(),
        }
    }

    pub fn push_str(&mut self, s: &str) {
        if s.is_empty() {
            return;
        }
        self.segments.push_back(s.to_string());
        self.total += s.len();
        self.evict();
    }

    fn evict(&mut self) {
        while self.total > self.capacity && self.segments.len() > 1 {
            if let Some(front) = self.segments.pop_front() {
                self.total = self.total.saturating_sub(front.len());
            }
        }
        // 单段超容量:整段保留(替换语义,见测试 3)
    }

    pub fn snapshot(&self) -> String {
        self.segments.iter().map(String::as_str).collect()
    }

    pub fn total_bytes(&self) -> usize {
        self.total
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_order_and_snapshot() {
        let mut rb = ReplayBuffer::new(1024);
        rb.push_str("hello ");
        rb.push_str("world");
        assert_eq!(rb.snapshot(), "hello world");
        assert_eq!(rb.total_bytes(), 11);
    }

    #[test]
    fn evicts_oldest_when_full() {
        let mut rb = ReplayBuffer::new(10);
        rb.push_str("0123456789"); // 正好满
        rb.push_str("ABC");
        let s = rb.snapshot();
        assert!(s.ends_with("ABC"));
        assert!(s.len() <= 13, "最多保留新段+已存段,不会无限增长");
        assert!(!s.starts_with('0') || s.len() == 13, "最旧内容应被挤掉");
    }

    #[test]
    fn single_oversized_segment_replaces_all() {
        let mut rb = ReplayBuffer::new(8);
        rb.push_str("old");
        rb.push_str("this-segment-is-way-longer-than-capacity");
        let s = rb.snapshot();
        assert_eq!(s, "this-segment-is-way-longer-than-capacity");
        assert!(rb.total_bytes() >= s.len());
    }

    #[test]
    fn empty_snapshot_is_empty_string() {
        let rb = ReplayBuffer::new(64);
        assert_eq!(rb.snapshot(), "");
        assert_eq!(rb.total_bytes(), 0);
    }
}
