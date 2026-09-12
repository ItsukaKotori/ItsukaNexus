// epoch 秒 → "yyMMdd-HHmmss"(UTC)。手写格里历换算(Hinnant civil_from_days),
// 避免为日期格式化引 chrono/time 依赖(worktree 命名是唯一用途)。
pub fn ymd_hms(epoch_secs: u64) -> String {
    let days = (epoch_secs / 86_400) as i64;
    let secs_of_day = epoch_secs % 86_400;
    let (y, m, d) = civil_from_days(days);
    let yy = y % 100;
    format!(
        "{:02}{:02}{:02}-{:02}{:02}{:02}",
        yy,
        m,
        d,
        secs_of_day / 3600,
        (secs_of_day % 3600) / 60,
        secs_of_day % 60
    )
}

/// 天数(自 1970-01-01)→ (年, 月, 日),格里历。算法:Howard Hinnant 的
/// civil_from_days(http://howardhinnant.github.io/date_algorithms.html)。
/// 按 400 年格里历周期(146097 天)定位 era,再从周期内天数反解 y/m/d。
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468; // 1970-01-01 → 0000-03-01 起的天数偏移
    let era = (if z >= 0 { z } else { z - 146_096 }) / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096] era 内第几天
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11](3 月起算的月份)
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::ymd_hms;

    #[test]
    fn known_epoch_matches_utc() {
        // 2023-11-14T22:13:20Z。钉死前已用 `date -u -r 1700000000 +%y%m%d-%H%M%S`
        // 核对(brief 示例值 224048 为笔误,以此为准)。
        assert_eq!(ymd_hms(1_700_000_000), "231114-221320");
    }

    #[test]
    fn unix_epoch_is_700101() {
        // 1970-01-01T00:00:00Z:yy 两位直接是 "70",不补零到四位。
        assert_eq!(ymd_hms(0), "700101-000000");
    }
}
