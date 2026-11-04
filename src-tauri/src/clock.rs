//! 时间：只用到 UTC 时间戳、日期和备份文件名里的时间，不引入日期库。

use std::time::{SystemTime, UNIX_EPOCH};

/// 现在的 Unix 秒数。
pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or(0)
}

/// `2026-11-04`（UTC）。
pub fn date(secs: u64) -> String {
    let (year, month, day) = civil_from_days((secs / 86_400) as i64);
    format!("{year:04}-{month:02}-{day:02}")
}

/// `20261104-053012`（UTC），按字典序排就是按时间排。
pub fn stamp(secs: u64) -> String {
    let (year, month, day) = civil_from_days((secs / 86_400) as i64);
    let rest = secs % 86_400;
    format!(
        "{year:04}{month:02}{day:02}-{:02}{:02}{:02}",
        rest / 3_600,
        rest % 3_600 / 60,
        rest % 60
    )
}

/// 公历换算（Howard Hinnant 的 civil_from_days）。
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_dates_and_stamps() {
        // 2026-11-04 05:30:12 UTC
        let secs = 1_793_770_212;
        assert_eq!(date(secs), "2026-11-04");
        assert_eq!(stamp(secs), "20261104-053012");
        assert_eq!(date(0), "1970-01-01");
        // 闰年。
        assert_eq!(date(951_782_400), "2000-02-29");
    }
}
