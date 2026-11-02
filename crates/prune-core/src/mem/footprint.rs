//! 深探（B1）：macOS 调 `/usr/bin/footprint`，Windows 走 `GetProcessMemoryInfo`。
//!
//! **降级链（docs/m2-memory-plan.md §7，本期最关键的正确性要求）**
//!
//! | 情况 | 判定 |
//! |---|---|
//! | 命令不存在 / 无法 spawn | `ProbeError::Spawn` → 全局降级为 RSS 口径 |
//! | 退出码 0 但 JSON 文件不存在 | `ProbeError::NoOutput`（**实测 pid 1 走这条，绝不能当成功**） |
//! | JSON 解析失败 / 无 processes[] | `ProbeError::Parse` → 单进程降级 RSS |
//! | 超过 3 s | `ProbeError::Timeout` → 单进程降级 RSS |
//!
//! 唯一成功判据是「JSON 文件产出 + 必需字段完整」，**不看退出码**。
//!
//! Windows 没有 footprint 也没有「换出量」口径，改用 **Private Bytes**
//! （`K32GetProcessMemoryInfo` 的 `PrivateUsage`）：语义最接近 footprint 的
//! 「真实占用」，但**不含被换出到 pagefile 的部分**，因此 `swapped = None`，
//! UI 隐藏换出相关读数，而不是显示 0。

use std::collections::HashMap;
#[cfg(target_os = "macos")]
use std::io::ErrorKind;
#[cfg(target_os = "macos")]
use std::path::PathBuf;
#[cfg(target_os = "macos")]
use std::process::{Command, Stdio};
#[cfg(target_os = "macos")]
use std::sync::atomic::AtomicU64;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
#[cfg(target_os = "macos")]
use std::time::Instant;

/// 单次深探超时（3 s：实测 0.09 s，留足异常余量）
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(3);
/// footprint 并发上限（Semaphore(8)；实测 12 路 0.27 s）
pub const DEFAULT_CONCURRENCY: usize = 8;

/// 一次成功深探的结果
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FootprintSample {
    pub pid: i32,
    /// macOS：phys_footprint（含被换出部分）；Windows：Private Bytes
    pub footprint: u64,
    /// `categories[*].swapped` 求和。None = 该平台没有换出口径（Windows）
    pub swapped: Option<u64>,
}

/// 深探失败原因（全部意味着「该进程退回 RSS 口径」）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeError {
    /// 命令不存在 / 无法启动
    Spawn(String),
    /// 超过超时时间
    Timeout,
    /// 进程未退出成功、或没有产出 JSON 文件（退出码 0 也算失败）
    NoOutput,
    /// JSON 解析失败或关键字段缺失
    Parse(String),
}

impl std::fmt::Display for ProbeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProbeError::Spawn(e) => write!(f, "footprint 启动失败: {e}"),
            ProbeError::Timeout => write!(f, "footprint 超时"),
            ProbeError::NoOutput => write!(f, "footprint 未产出 JSON（多为无权限读取该进程）"),
            ProbeError::Parse(e) => write!(f, "footprint JSON 解析失败: {e}"),
        }
    }
}

impl std::error::Error for ProbeError {}

/// 临时 JSON 文件名序号（仅 macOS 的 footprint 深探使用）
#[cfg(target_os = "macos")]
static SEQ: AtomicU64 = AtomicU64::new(0);

#[cfg(target_os = "macos")]
fn temp_json_path(pid: i32) -> PathBuf {
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join("prune-footprint");
    let _ = std::fs::create_dir_all(&dir);
    dir.join(format!("{pid}-{seq}-{nanos}.json"))
}

/// 深探单个进程（默认 3 s 超时）
pub fn probe(pid: i32) -> Result<FootprintSample, ProbeError> {
    probe_with_timeout(pid, DEFAULT_TIMEOUT)
}

/// 深探单个进程，可指定超时
pub fn probe_with_timeout(pid: i32, timeout: Duration) -> Result<FootprintSample, ProbeError> {
    #[cfg(target_os = "macos")]
    {
        probe_macos(pid, timeout)
    }
    #[cfg(windows)]
    {
        let _ = timeout;
        probe_windows(pid)
    }
    #[cfg(all(not(target_os = "macos"), not(windows)))]
    {
        let _ = (pid, timeout);
        Err(ProbeError::Spawn("当前平台没有深探实现".to_string()))
    }
}

/// Windows：Private Bytes。
///
/// `OpenProcess` 打不开（进程已退出 / 权限不足 / 受保护进程）一律算「无产出」，
/// 由调用方把该进程降级成 RSS 口径——和 macOS 的 `NoOutput` 语义保持一致。
#[cfg(windows)]
fn probe_windows(pid: i32) -> Result<FootprintSample, ProbeError> {
    match crate::win::private_bytes(pid) {
        Some(bytes) => Ok(FootprintSample {
            pid,
            footprint: bytes,
            // 换出量在 Windows 上没有可读口径：None 而不是 0
            swapped: None,
        }),
        None => Err(ProbeError::NoOutput),
    }
}

#[cfg(target_os = "macos")]
fn probe_macos(pid: i32, timeout: Duration) -> Result<FootprintSample, ProbeError> {
    const FOOTPRINT: &str = "/usr/bin/footprint";
    let path = temp_json_path(pid);

    let mut child = Command::new(FOOTPRINT)
        .arg("-p")
        .arg(pid.to_string())
        .arg("--swapped")
        .arg("-j")
        .arg(&path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| {
            let _ = std::fs::remove_file(&path);
            ProbeError::Spawn(e.to_string())
        })?;

    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_status)) => break,
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = std::fs::remove_file(&path);
                    return Err(ProbeError::Timeout);
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = std::fs::remove_file(&path);
                return Err(ProbeError::Spawn(e.to_string()));
            }
        }
    }

    // 关键：不看退出码（实测读不到进程时退出码仍为 0），只看产物
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == ErrorKind::NotFound => return Err(ProbeError::NoOutput),
        Err(e) => {
            let _ = std::fs::remove_file(&path);
            return Err(ProbeError::Parse(e.to_string()));
        }
    };
    let _ = std::fs::remove_file(&path);
    parse(&text)
}

/// 解析 footprint JSON，只依赖 `footprint` 与 `categories[*].swapped` 两个字段。
pub fn parse(json: &str) -> Result<FootprintSample, ProbeError> {
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| ProbeError::Parse(e.to_string()))?;

    let processes = v
        .get("processes")
        .and_then(|p| p.as_array())
        .ok_or_else(|| ProbeError::Parse("缺少 processes[]".to_string()))?;
    let p = processes
        .first()
        .ok_or_else(|| ProbeError::Parse("processes[] 为空".to_string()))?;

    let pid = p
        .get("pid")
        .and_then(|x| x.as_i64())
        .ok_or_else(|| ProbeError::Parse("缺少 pid".to_string()))? as i32;

    let footprint = p
        .get("footprint")
        .and_then(|x| x.as_u64())
        .ok_or_else(|| ProbeError::Parse("缺少 footprint".to_string()))?;

    let mut swapped: u64 = 0;
    if let Some(cats) = p.get("categories").and_then(|c| c.as_object()) {
        for cat in cats.values() {
            if let Some(s) = cat.get("swapped").and_then(|x| x.as_u64()) {
                swapped = swapped.saturating_add(s);
            }
        }
    }

    Ok(FootprintSample {
        pid,
        footprint,
        // macOS 有换出口径：即使 categories 缺失也返回 Some(0)（真实的「没换出」）
        swapped: Some(swapped),
    })
}

/// 批量深探：有界并发（默认 8 路），失败的进程不会出现在结果里 → 由调用方降级。
pub fn probe_many(pids: &[i32], concurrency: usize) -> HashMap<i32, FootprintSample> {
    if pids.is_empty() {
        return HashMap::new();
    }
    let pids = Arc::new(pids.to_vec());
    let results = Arc::new(Mutex::new(HashMap::with_capacity(pids.len())));
    let cursor = Arc::new(AtomicUsize::new(0));
    let workers = concurrency.max(1).min(pids.len());

    let mut handles = Vec::with_capacity(workers);
    for _ in 0..workers {
        let pids = Arc::clone(&pids);
        let results = Arc::clone(&results);
        let cursor = Arc::clone(&cursor);
        handles.push(std::thread::spawn(move || loop {
            let idx = cursor.fetch_add(1, Ordering::Relaxed);
            if idx >= pids.len() {
                break;
            }
            let pid = pids[idx];
            if let Ok(sample) = probe(pid) {
                results.lock().unwrap_or_else(|e| e.into_inner()).insert(pid, sample);
            }
        }));
    }
    for h in handles {
        let _ = h.join();
    }

    Arc::try_unwrap(results)
        .map(|m| m.into_inner().unwrap_or_else(|e| e.into_inner()))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_real_shape_sums_swapped() {
        // 取自真实 footprint 输出的字段子集
        let json = r#"{
            "processes": [{
                "pid": 4242,
                "name": "next-server",
                "footprint": 3355443200,
                "categories": {
                    "Dirty": {"dirty": 10, "swapped": 2000000000},
                    "Swapped": {"dirty": 0, "swapped": 1200000000},
                    "Clean": {"dirty": 0}
                }
            }]
        }"#;
        let s = parse(json).expect("应解析成功");
        assert_eq!(s.pid, 4242);
        assert_eq!(s.footprint, 3_355_443_200);
        assert_eq!(s.swapped, Some(3_200_000_000));
    }

    #[test]
    fn parse_missing_footprint_is_error() {
        let json = r#"{"processes":[{"pid":1,"name":"launchd"}]}"#;
        assert!(matches!(parse(json), Err(ProbeError::Parse(_))));
    }

    #[test]
    fn parse_empty_processes_is_error() {
        assert!(matches!(
            parse(r#"{"processes":[]}"#),
            Err(ProbeError::Parse(_))
        ));
    }

    #[test]
    fn parse_garbage_is_error() {
        assert!(matches!(parse("not json"), Err(ProbeError::Parse(_))));
    }

    #[test]
    fn parse_without_categories_yields_zero_swapped() {
        let json = r#"{"processes":[{"pid":7,"footprint":1024}]}"#;
        let s = parse(json).unwrap();
        assert_eq!(s.swapped, Some(0));
        assert_eq!(s.footprint, 1024);
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn probe_nonexistent_pid_has_no_output() {
        // pid 999999 必然不存在：footprint 仍会以退出码 0 结束，但无 JSON 产出
        let err = probe_with_timeout(999_999, Duration::from_secs(2)).unwrap_err();
        assert!(
            matches!(err, ProbeError::NoOutput | ProbeError::Parse(_)),
            "应为无产出或解析失败，实际: {err:?}"
        );
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn probe_self_returns_footprint_larger_than_zero() {
        let me = std::process::id() as i32;
        match probe_with_timeout(me, Duration::from_secs(3)) {
            Ok(s) => {
                assert_eq!(s.pid, me);
                assert!(s.footprint > 0);
            }
            Err(e) => panic!("自身进程应可深探，实际失败: {e}"),
        }
    }

    #[test]
    fn probe_many_empty_input() {
        assert!(probe_many(&[], 8).is_empty());
    }
}
