//! 本地时间口径（M14）：把 Unix 秒拆成本地日期 / 当日 00:00。
//!
//! 为什么不为「今天是几号」引 `chrono`：一个日期库换两行 FFI 不划算，而且
//! `libc` 与 `windows-sys` 都已经是 core 的依赖（不是新增依赖）。
//!
//! 这里是**唯一**的 FFI 落点：`swap`（今日新增 swapfile）与 `net::traffic`
//! （今日流量）都从 [`day_string`] / [`day_start`] 取值。同一件事实现两次，
//! 迟早出现「内存页说今天、流量说昨天」的错位。
//!
//! 已知限制：时区取**进程当前**的本地时区（含 DST 规则）；跨时区旅行会把一天
//! 切成两段，不做补偿（方案 §3.5）。

/// 本地时间分解（只取日期与日内偏移需要的字段）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalTime {
    pub year: i64,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

impl LocalTime {
    /// 日内偏移（秒）。用于把「现在」折回「今天 00:00」
    pub fn secs_of_day(self) -> i64 {
        self.hour as i64 * 3600 + self.minute as i64 * 60 + self.second as i64
    }
}

/// 本地日期 `YYYY-MM-DD`（字典序即时间序，可直接进 SQL 比较）
pub fn day_string(ts: i64) -> String {
    match local_time(ts) {
        Some(t) => format!("{:04}-{:02}-{:02}", t.year, t.month, t.day),
        // 极端脏数据（时间戳超出可表示范围）：给一个稳定值，不 panic
        None => "1970-01-01".to_string(),
    }
}

/// 本地时区当日 00:00 的 Unix 秒
pub fn day_start(now: u64) -> u64 {
    let ts = now.min(i64::MAX as u64) as i64;
    match local_time(ts) {
        Some(t) => (ts - t.secs_of_day()).max(0) as u64,
        None => now.saturating_sub(now % 86_400),
    }
}

#[cfg(unix)]
fn local_time(ts: i64) -> Option<LocalTime> {
    // SAFETY: localtime_r 是线程安全版本（结果写进我们提供的 tm），
    // 返回空指针表示时间戳不可表示。
    unsafe {
        let t = ts as libc::time_t;
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&t, &mut tm).is_null() {
            return None;
        }
        Some(LocalTime {
            year: tm.tm_year as i64 + 1900,
            month: (tm.tm_mon + 1).clamp(1, 12) as u32,
            day: tm.tm_mday.clamp(1, 31) as u32,
            hour: tm.tm_hour.clamp(0, 23) as u32,
            minute: tm.tm_min.clamp(0, 59) as u32,
            second: tm.tm_sec.clamp(0, 60) as u32,
        })
    }
}

#[cfg(windows)]
fn local_time(ts: i64) -> Option<LocalTime> {
    use windows_sys::Win32::Foundation::{FILETIME, SYSTEMTIME};
    use windows_sys::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};

    // Unix 纪元 → FILETIME：1601-01-01 起的 100 ns 计数
    const EPOCH_DIFF_SECS: i64 = 11_644_473_600;
    let ticks = (ts.saturating_add(EPOCH_DIFF_SECS)).max(0) as u64 * 10_000_000;
    let ft = FILETIME {
        dwLowDateTime: (ticks & 0xffff_ffff) as u32,
        dwHighDateTime: (ticks >> 32) as u32,
    };
    // windows-sys 的结构没实现 Default，只能零初始化
    let mut utc: SYSTEMTIME = unsafe { std::mem::zeroed() };
    let mut local: SYSTEMTIME = unsafe { std::mem::zeroed() };
    // SAFETY: 指针都指向本栈上的有效结构；null 时区 = 当前时区（含 DST 规则）
    let ok = unsafe {
        FileTimeToSystemTime(&ft, &mut utc) != 0
            && SystemTimeToTzSpecificLocalTime(std::ptr::null(), &utc, &mut local) != 0
    };
    if !ok {
        return None;
    }
    Some(LocalTime {
        year: local.wYear as i64,
        month: local.wMonth as u32,
        day: local.wDay as u32,
        hour: local.wHour as u32,
        minute: local.wMinute as u32,
        second: local.wSecond as u32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_day_format(s: &str) -> bool {
        let parts: Vec<&str> = s.split('-').collect();
        if parts.len() != 3 || parts[0].len() != 4 || parts[1].len() != 2 || parts[2].len() != 2 {
            return false;
        }
        if parts.iter().any(|p| !p.chars().all(|c| c.is_ascii_digit())) {
            return false;
        }
        let month: u32 = parts[1].parse().unwrap_or(0);
        let day: u32 = parts[2].parse().unwrap_or(0);
        (1..=12).contains(&month) && (1..=31).contains(&day)
    }

    /// 只断言**格式**，不硬编码时区期望：CI 跑在不同时区上，写死 `2026-11-05`
    /// 会让测试在 UTC-8 的机器上红一次（这类测试没人愿意修第二次）
    #[test]
    fn day_string_has_expected_format() {
        assert!(is_day_format(&day_string(1_762_343_000)));
        assert!(is_day_format(&day_string(0)));
        assert_eq!(day_string(0), day_string(0), "同一秒必须稳定");
    }

    /// 一天的跨度必须换一天（地球上没有 ±24h 的时区，这条与时区无关）
    #[test]
    fn day_string_advances_by_one_day() {
        let now = 1_762_343_000;
        assert_ne!(day_string(now), day_string(now + 86_400));
        assert_ne!(day_string(now), day_string(now - 86_400));
    }

    #[test]
    fn day_start_is_within_today() {
        let now = 1_762_343_000u64;
        let start = day_start(now);
        assert!(start <= now);
        assert!(now - start < 86_400);
        // 00:00 必然对齐到分钟（有些时区带 30/45 min 偏移，不能断言整点）
        assert_eq!(start % 60, 0);
        // 当日 00:00 的日期串与「现在」相同
        assert_eq!(day_string(start as i64), day_string(now as i64));
    }
}
