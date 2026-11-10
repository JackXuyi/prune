//! swap 仪表：macOS 走 `sysctl vm.swapusage` + `/System/Volumes/VM/` swapfile 统计，
//! Windows 走 pagefile（`GetPerformanceInfo`，经 sysinfo 封装）。
//!
//! 两个 macOS 坑（docs/m2-memory-plan.md §2）：
//! 1. swapfile **编号不连续**（回收后不重用）→ 只能数文件个数，不能按最大编号推算；
//! 2. 「今日新增」按文件 mtime 落在本地当日统计，并用「与上次数量的差值」兜底（D-M6）。
//!
//! Windows 没有 swapfile 这一层概念（pagefile 是系统托管的单个/多个 .sys 文件，
//! 大小与增长由系统决定），因此 `file_count` / `new_today` 为 `None`——UI 隐藏，
//! 不用 0 冒充「没有新增」。

use std::path::Path;

use serde::{Deserialize, Serialize};

/// swap 使用情况（字节）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SwapInfo {
    pub total: u64,
    pub used: u64,
    pub free: u64,
    /// macOS：`/System/Volumes/VM/swapfile*` 文件个数；None = 该平台无此口径
    pub file_count: Option<usize>,
    /// macOS：今日新增（mtime 当日 + 数量差值兜底）；None = 该平台无此口径
    pub new_today: Option<usize>,
}

impl SwapInfo {
    /// 使用率 0~1；total 为 0 时返回 0
    pub fn used_ratio(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            self.used as f64 / self.total as f64
        }
    }
}

/// swap 目录（macOS 固定位置）
pub const SWAP_DIR: &str = "/System/Volumes/VM";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwapFile {
    pub name: String,
    /// 修改时间（Unix 秒）
    pub mtime: u64,
}

/// 解析 `sysctl vm.swapusage` 输出，返回 (total, used, free) 字节数。
///
/// 形如：`total = 11264.00M  used = 10482.50M  free = 781.50M  (encrypted)`
pub fn parse_swapusage(text: &str) -> Result<(u64, u64, u64), String> {
    let padded = text.replace('=', " = ");
    let tokens: Vec<&str> = padded.split_whitespace().collect();

    let mut total = None;
    let mut used = None;
    let mut free = None;

    let mut i = 0;
    while i + 2 < tokens.len() {
        if tokens[i + 1] == "=" {
            if let Some(bytes) = parse_size(tokens[i + 2]) {
                match tokens[i] {
                    "total" => total = Some(bytes),
                    "used" => used = Some(bytes),
                    "free" => free = Some(bytes),
                    _ => {}
                }
            }
            i += 3;
        } else {
            i += 1;
        }
    }

    match (total, used, free) {
        (Some(t), Some(u), Some(f)) => Ok((t, u, f)),
        _ => Err(format!("swapusage 解析失败: {text}")),
    }
}

/// `11264.00M` / `1.5G` / `512K` → 字节
fn parse_size(token: &str) -> Option<u64> {
    let token = token.trim();
    let (num_part, unit) = match token.chars().last()? {
        'K' | 'k' => (&token[..token.len() - 1], 1024f64),
        'M' | 'm' => (&token[..token.len() - 1], 1024f64 * 1024.0),
        'G' | 'g' => (&token[..token.len() - 1], 1024f64 * 1024.0 * 1024.0),
        'T' | 't' => (
            &token[..token.len() - 1],
            1024f64 * 1024.0 * 1024.0 * 1024.0,
        ),
        'B' | 'b' => (&token[..token.len() - 1], 1f64),
        _ => (token, 1f64),
    };
    let value: f64 = num_part.parse().ok()?;
    Some((value * unit) as u64)
}

/// 读取 swap 用量；失败时返回全 0（UI 会显示「不可用」而不是假数字）
pub fn read_swapusage() -> Option<(u64, u64, u64)> {
    let out = std::process::Command::new("/usr/sbin/sysctl")
        .arg("-n")
        .arg("vm.swapusage")
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    parse_swapusage(&String::from_utf8_lossy(&out.stdout)).ok()
}

/// 列出 swapfile 及其 mtime
pub fn list_swapfiles() -> Vec<SwapFile> {
    let Ok(entries) = std::fs::read_dir(Path::new(SWAP_DIR)) else {
        return Vec::new();
    };
    let mut files = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with("swapfile") {
            continue;
        }
        let mtime = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        files.push(SwapFile { name, mtime });
    }
    files.sort_by(|a, b| a.name.cmp(&b.name));
    files
}

/// 统计 mtime >= since 的 swapfile 个数（纯函数，便于单测）
pub fn count_new_since(files: &[SwapFile], since: u64) -> usize {
    files.iter().filter(|f| f.mtime >= since).count()
}

/// 采集一次 swap 状态。
///
/// `prev_count` 为上一轮的文件数（仅 macOS 有意义）：mtime 统计为 0 但数量增加时，
/// 用差值兜底（防止刚写完的 swapfile mtime 精度/时区导致的漏计）。
pub fn collect(prev_count: Option<usize>) -> SwapInfo {
    #[cfg(target_os = "macos")]
    {
        collect_macos(prev_count)
    }
    #[cfg(windows)]
    {
        let _ = prev_count;
        collect_windows()
    }
    #[cfg(all(not(target_os = "macos"), not(windows)))]
    {
        let _ = prev_count;
        SwapInfo::default()
    }
}

#[cfg(target_os = "macos")]
fn collect_macos(prev_count: Option<usize>) -> SwapInfo {
    let (total, used, free) = read_swapusage().unwrap_or((0, 0, 0));
    let files = list_swapfiles();
    let file_count = files.len();
    let since = local_day_start(now_secs());
    let by_mtime = count_new_since(&files, since);
    let by_delta = prev_count
        .map(|p| file_count.saturating_sub(p))
        .unwrap_or(0);

    SwapInfo {
        total,
        used,
        free,
        file_count: Some(file_count),
        new_today: Some(by_mtime.max(by_delta)),
    }
}

/// Windows：pagefile 口径。
///
/// 走 sysinfo 的 `total_swap` / `used_swap`（内部是 `GetPerformanceInfo` 的
/// commit 额度减去物理内存），不自己再写一遍 FFI——
/// 这个换算已经有一次实现，重复实现只会多一个出错点。
#[cfg(windows)]
fn collect_windows() -> SwapInfo {
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    let total = sys.total_swap();
    let used = sys.used_swap().min(total);
    SwapInfo {
        total,
        used,
        free: total.saturating_sub(used),
        file_count: None,
        new_today: None,
    }
}

pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 本地时区当日 00:00 的 Unix 秒
///
/// 实现在 `crate::localtime`（M14 起与「今日流量」共用同一处 FFI）——
/// Windows 分支顺带从 UTC 口径改成真正的本地口径。
pub fn local_day_start(now: u64) -> u64 {
    crate::localtime::day_start(now)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_swapusage_megabytes() {
        let (t, u, f) =
            parse_swapusage("total = 11264.00M  used = 10482.50M  free = 781.50M  (encrypted)")
                .unwrap();
        assert_eq!(t, 11_264 * 1024 * 1024);
        assert_eq!(u, (10_482.5 * 1024.0 * 1024.0) as u64);
        assert_eq!(f, (781.5 * 1024.0 * 1024.0) as u64);
    }

    #[test]
    fn parses_swapusage_without_spaces_and_gigabytes() {
        let (t, u, f) = parse_swapusage("total=2.00G used=1.25G free=0.75G").unwrap();
        assert_eq!(t, 2 * 1024 * 1024 * 1024);
        assert_eq!(u, (1.25 * 1024.0 * 1024.0 * 1024.0) as u64);
        assert_eq!(f, (0.75 * 1024.0 * 1024.0 * 1024.0) as u64);
    }

    #[test]
    fn rejects_garbage_swapusage() {
        assert!(parse_swapusage("total = abc used = 1M free = 1M").is_err());
        assert!(parse_swapusage("no data").is_err());
    }

    #[test]
    fn counts_new_files_since_midnight() {
        let files = vec![
            SwapFile {
                name: "swapfile0".into(),
                mtime: 100,
            },
            SwapFile {
                name: "swapfile1".into(),
                mtime: 2_000,
            },
            SwapFile {
                name: "swapfile2".into(),
                mtime: 3_000,
            },
        ];
        assert_eq!(count_new_since(&files, 2_000), 2);
        assert_eq!(count_new_since(&files, 9_999), 0);
    }

    #[test]
    fn used_ratio_is_zero_safe() {
        let s = SwapInfo::default();
        assert_eq!(s.used_ratio(), 0.0);
        let s = SwapInfo {
            total: 100,
            used: 96,
            ..Default::default()
        };
        assert!((s.used_ratio() - 0.96).abs() < 1e-9);
    }

    #[test]
    fn local_day_start_is_within_today() {
        let now = now_secs();
        let start = local_day_start(now);
        assert!(start <= now);
        assert!(now - start < 86_400 + 1);
        // 本地当日 00:00 必然对齐到分钟
        assert_eq!(start % 60, 0);
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn reads_live_swap_and_files() {
        let (total, used, _free) = read_swapusage().expect("本机应可读 vm.swapusage");
        assert!(total > 0);
        assert!(used <= total);
        assert!(!list_swapfiles().is_empty());
    }
}
