// newtype 防字符串滥用(spec M3 学习主题的提前落地):
// SessionId::new() 生成新 id;外部输入也可经 parse 为 Uuid 后 From 构造——
// 所以"防伪造"的防线不在类型本身,而在 manager 注册表:表里没有就是 SessionNotFound。
use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(uuid::Uuid);

impl SessionId {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4())
    }
}

impl Default for SessionId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<uuid::Uuid> for SessionId {
    fn from(u: uuid::Uuid) -> Self {
        Self(u)
    }
}

impl std::str::FromStr for SessionId {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        uuid::Uuid::parse_str(s)
            .map(SessionId)
            .map_err(|e| format!("非法 session id {s:?}: {e}"))
    }
}

/// worktree 名 == 分支名 == 目录名(spec §1.3):`nexus/<provider>-<yyMMdd-HHmmss>-<rand4>`。
/// 时间段来自 `gitx::timefmt::ymd_hms`(UTC),rand4 取 uuid v4 的前 4 位 hex。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WorktreeName(String);

impl WorktreeName {
    pub fn generate(provider: &str, epoch_secs: u64) -> Self {
        // sanitize 到 [a-z0-9-]:分支/目录名都要能直接吃这个结果
        let p: String = provider
            .chars()
            .map(|c| {
                if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' {
                    c
                } else {
                    '-'
                }
            })
            .collect();
        let ts = crate::gitx::timefmt::ymd_hms(epoch_secs);
        let uuid_simple = uuid::Uuid::new_v4().simple().to_string();
        let rand = &uuid_simple[..4];
        Self(format!("nexus/{p}-{ts}-{rand}"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for WorktreeName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// 校验 `nexus/` 前缀 + 段结构:`<provider…>-<yyMMdd>-<HHmmss>-<rand4>`。
/// provider 自身可含连字符(如 claude-code),所以段数从尾部数:
/// 末段 4 位 hex,其前两段各 6 位数字,其余全是 provider。
impl std::str::FromStr for WorktreeName {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let tail = s
            .strip_prefix("nexus/")
            .ok_or_else(|| format!("缺 nexus/ 前缀: {s:?}"))?;
        let is_digits = |v: &str| v.len() == 6 && v.bytes().all(|b| b.is_ascii_digit());
        let is_hex4 = |v: &str| v.len() == 4 && v.bytes().all(|b| b.is_ascii_hexdigit());
        // 段数从尾部数(provider 可含连字符):末段 rand4,其前两段时间戳
        let segs: Vec<&str> = tail.split('-').collect();
        if segs.len() < 4 {
            return Err(format!("段数不足: {s:?}"));
        }
        let n = segs.len();
        let (rand, time, date) = (segs[n - 1], segs[n - 2], segs[n - 3]);
        let provider = &segs[..n - 3];
        if !is_hex4(rand) {
            return Err(format!("rand4 应为 4 位 hex: {s:?}"));
        }
        if !is_digits(time) || !is_digits(date) {
            return Err(format!("时间戳两段应各为 6 位数字: {s:?}"));
        }
        if provider.is_empty() {
            return Err(format!("provider 段为空: {s:?}"));
        }
        Ok(Self(s.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::WorktreeName;
    use std::str::FromStr;

    #[test]
    fn generate_is_parseable_even_with_dashed_provider() {
        // provider 自带连字符:段校验从尾部数,不能误拒
        let n = WorktreeName::generate("claude-code", 1_700_000_000);
        assert!(n.as_str().starts_with("nexus/claude-code-231114-221320-"));
        assert_eq!(WorktreeName::from_str(n.as_str()).unwrap(), n);
    }

    #[test]
    fn sanitize_maps_invalid_chars_to_dash() {
        let n = WorktreeName::generate("Shell Claude v1.2", 0);
        // 大写→'-'、空格→'-'、'.'→'-':"-hell--laude-v1-2"
        assert!(
            n.as_str().starts_with("nexus/-hell--laude-v1-2-700101-"),
            "实际: {}",
            n.as_str()
        );
        // sanitize 结果仍是合法名
        assert!(WorktreeName::from_str(n.as_str()).is_ok());
    }

    #[test]
    fn from_str_rejects_bad_shapes() {
        assert!(WorktreeName::from_str("main").is_err(), "缺前缀");
        assert!(WorktreeName::from_str("nexus/only").is_err(), "段数不足");
        assert!(
            WorktreeName::from_str("nexus/p-231114-2213-abcd").is_err(),
            "时间戳非 6 位"
        );
        assert!(
            WorktreeName::from_str("nexus/p-231114-221320-zzzz").is_err(),
            "rand4 非 hex"
        );
    }
}
