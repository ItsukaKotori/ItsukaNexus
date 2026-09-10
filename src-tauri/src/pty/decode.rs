// 增量 UTF-8 解码:替代 from_utf8_lossy 的跨 chunk 安全方案(spec §1.2 pty/decode.rs)。
// 每会话一个实例,feed 合帧后的字节块;不完整的多字节尾部扣留在实例内,
// 下一块到达时续上——转义序列与文本都不会因 chunk 边界撕裂。
pub struct Decoder {
    /// 上一块遗留的不完整多字节前缀(已确认合法首字节 + 部分延续字节)
    pending: Vec<u8>,
}

impl Decoder {
    pub fn new() -> Self {
        Self {
            pending: Vec::new(),
        }
    }

    /// 喂入一块字节,返回此刻可安全显示的完整文本。
    pub fn feed(&mut self, bytes: &[u8]) -> String {
        // 拼上遗留前缀再整体解码(遗留最多 3 字节,拷贝开销可忽略)
        let mut buf = std::mem::take(&mut self.pending);
        buf.extend_from_slice(bytes);

        match std::str::from_utf8(&buf) {
            Ok(s) => s.to_string(),
            Err(e) => {
                let valid_up_to = e.valid_up_to();
                let mut out = String::with_capacity(buf.len());
                // 1) 完整前缀直接收录(前缀已被 from_utf8 确认合法,unwrap 不会触发)
                let valid = std::str::from_utf8(&buf[..valid_up_to]).unwrap();
                out.push_str(valid);
                match e.error_len() {
                    // 2) 尾部只是不完整:扣留,等下一块
                    None => {
                        self.pending = buf[valid_up_to..].to_vec();
                    }
                    // 3) 确实非法:替换符顶替坏字节,从下一字节重同步继续解
                    Some(bad_len) => {
                        out.push('\u{FFFD}');
                        let rest = &buf[valid_up_to + bad_len..];
                        // 剩余部分递归式处理(一次 feed 理论上可能多个坏字节)
                        let tail = self.feed(rest);
                        out.push_str(&tail);
                    }
                }
                out
            }
        }
    }

    /// 当前扣留的不完整字节数(测试与诊断用)
    pub fn pending(&self) -> usize {
        self.pending.len()
    }
}

impl Default for Decoder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 把 bytes 按给定切分逐块 feed,返回拼接结果——模拟跨 chunk 边界
    fn feed_chunks(dec: &mut Decoder, bytes: &[u8], splits: &[usize]) -> String {
        let mut out = String::new();
        let mut start = 0usize;
        for &end in splits {
            out.push_str(&dec.feed(&bytes[start..end]));
            start = end;
        }
        out.push_str(&dec.feed(&bytes[start..]));
        out
    }

    #[test]
    fn ascii_passthrough() {
        let mut d = Decoder::new();
        assert_eq!(d.feed(b"hello"), "hello");
        assert_eq!(d.pending(), 0);
    }

    #[test]
    fn chinese_split_across_chunks() {
        // "你好世界" = 4 × 3 字节;在每个字节边界都切一遍
        let bytes = "你好世界".as_bytes();
        for k in 1..bytes.len() {
            let mut d = Decoder::new();
            let out = feed_chunks(&mut d, bytes, &[k]);
            assert_eq!(out, "你好世界", "切分点 {k} 不应产生替换符");
        }
    }

    #[test]
    fn emoji_4byte_split() {
        // 🚀 = F0 9F 9A 80(4 字节);拆成 1+3 与 2+2
        let bytes = "🚀".as_bytes();
        assert_eq!(bytes.len(), 4);
        let mut d = Decoder::new();
        assert_eq!(feed_chunks(&mut d, bytes, &[1]), "🚀");
        let mut d = Decoder::new();
        assert_eq!(feed_chunks(&mut d, bytes, &[2]), "🚀");
    }

    #[test]
    fn incomplete_tail_withheld_then_completed() {
        let mut d = Decoder::new();
        let bytes = "中".as_bytes(); // E4 BD AD
        let first = d.feed(&bytes[..2]);
        assert_eq!(first, "", "不完整尾部应扣留,不产替换符");
        assert_eq!(d.pending(), 2);
        let second = d.feed(&bytes[2..]);
        assert_eq!(second, "中");
    }

    #[test]
    fn invalid_byte_becomes_replacement_and_resyncs() {
        let mut d = Decoder::new();
        // 0xFF 是非法 UTF-8 首字节:替换符处理后,后续正常文本应恢复
        let out = d.feed(&[0xFF, b'o', b'k']);
        assert_eq!(out, "\u{FFFD}ok");
        assert_eq!(d.pending(), 0);
    }

    #[test]
    fn mixed_multibyte_stream_split_at_every_point() {
        // 中文 + emoji + ASCII 混排,滑窗切分(覆盖多字符同时跨界的组合)
        let text = "a你b🚀c好d🎉e";
        let bytes = text.as_bytes();
        for k in 1..bytes.len() {
            let mut d = Decoder::new();
            assert_eq!(feed_chunks(&mut d, bytes, &[k]), text, "切分点 {k}");
        }
    }
}
