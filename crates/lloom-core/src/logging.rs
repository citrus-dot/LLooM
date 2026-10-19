//! P1-12：统一内部日志门面。
//!
//! 之前各模块散落 `eprintln!`/`println!`：无时间戳、无级别、热路径日志无法
//! 关闭。本模块给 lloom-core / lloom-server 一个统一出口：
//! - 行格式：`<ISO8601 UTC 时间> <LEVEL> [<target>] <消息>`
//! - 级别门槛由 `LLOOM_LOG` 控制（`error` / `warn` / `info`，默认 `info`；
//!   `LLOOM_LOG=error` 时 info 静默，运维可只看异常）。
//! - `info` 走 stdout（生命周期/状态类），`warn`/`error` 走 stderr。
//!
//! 保留旧 `[core]`/`[cache]` 等 target 前缀，兼容现有 grep 习惯。

use std::sync::{Mutex, OnceLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Error = 1,
    Warn = 2,
    Info = 3,
}

/// `LLOOM_LOG` 值 → 级别门槛。无法识别的值回退 Info。
pub fn level_from_env(value: &str) -> Level {
    match value.trim().to_ascii_lowercase().as_str() {
        "error" => Level::Error,
        "warn" => Level::Warn,
        _ => Level::Info,
    }
}

fn threshold() -> Level {
    static CELLS: OnceLock<Mutex<Option<Level>>> = OnceLock::new();
    let cell = CELLS.get_or_init(|| Mutex::new(None));
    let mut cached = cell.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(level) = *cached {
        return level;
    }
    let level = level_from_env(&std::env::var("LLOOM_LOG").unwrap_or_default());
    *cached = Some(level);
    level
}

/// Proleptic-Gregorian civil_from_days（UTC）→ (y, m, d)。
/// 供时间戳换算使用；与 usage 侧 UTC CURRENT_TIMESTAMP 语义一致。
pub fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m as u32, d as u32)
}

fn timestamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let (y, m, d) = civil_from_days(secs.div_euclid(86_400));
    let rem = secs.rem_euclid(86_400);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3_600,
        rem % 3_600 / 60,
        rem % 60
    )
}

/// info 级（生命周期/后台 job 进度）。默认输出；`LLOOM_LOG=error|warn` 时静默。
pub fn info(target: &str, message: &str) {
    if threshold() < Level::Info {
        return;
    }
    println!("{} INFO [{}] {message}", timestamp(), target);
}

/// warn 级。始终输出（stderr）。
pub fn warn(target: &str, message: &str) {
    if threshold() < Level::Warn {
        return;
    }
    eprintln!("{} WARN [{}] {message}", timestamp(), target);
}

/// error 级。始终输出（stderr）。
pub fn error(target: &str, message: &str) {
    eprintln!("{} ERROR [{}] {message}", timestamp(), target);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamp_shape() {
        let ts = timestamp();
        assert_eq!(ts.len(), 20);
        assert!(ts.ends_with('Z'));
        assert_eq!(&ts[4..5], "-");
        assert_eq!(&ts[7..8], "-");
        assert_eq!(&ts[10..11], "T");
    }

    #[test]
    fn civil_from_days_matches_known_dates() {
        // 1970-01-01
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        // 2026-10-19
        assert_eq!(civil_from_days(20_745), (2026, 10, 19));
        // 世纪闰日：2000-02-29 = 11016
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
        // 非世纪闰日：1996-02-29 = 9555
        assert_eq!(civil_from_days(9_555), (1996, 2, 29));
        // 负日期：1969-12-31 = -1
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
    }

    #[test]
    fn env_level_parsing() {
        assert_eq!(level_from_env("error"), Level::Error);
        assert_eq!(level_from_env(" WARN "), Level::Warn);
        assert_eq!(level_from_env("info"), Level::Info);
        // 无法识别/为空 → 默认 Info
        assert_eq!(level_from_env(""), Level::Info);
        assert_eq!(level_from_env("trace"), Level::Info);
    }
}
