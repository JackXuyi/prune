//! 网速采样（M10）：1 s 轮询 `nettop` 一次性采样，累计字节差分。
//!
//! 与 M7 端口同一套纪律：**可见性门控**。`active == false` 时连子进程都不 spawn，
//! 常驻开销为 0（切走「应用资源」tab 后 `pgrep nettop` 必须为空）。
//!
//! 为什么不用流式子进程（方案 §5.1 的原设计，实施时改掉）：
//! 实测 `nettop -L 0` 在管道上是 4 KB 块缓冲，4.5 s 才吐出第一批样本；
//! 要行缓冲必须 `script` 包 pty，为一个读数引入 pty 包装不值。
//! 一次性 `-l 1` 实测 14.5 ms/次，随起随灭，没有子进程跑飞的风险。
//!
//! 基线重置：切出 tab 时清空上一轮累计值。否则切回来第一次差分算的是
//! 「离开这段时间的平均速率」，语义上已经不是「当前网速」了。

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use prune_core::net::netspeed::{self, NetSpeedSample, NetSpeedStatus, RawBytes};
use tauri::{AppHandle, Emitter, Manager, State};

pub const EVENT_NET_SPEED: &str = "net://speed";
/// 采样间隔。1 s 是「数字看起来在跳」与「nettop 自耗可忽略」的折中
/// （14.5 ms/次 → 约 1.5% 单核占用，只在 tab 可见时跑）
pub const INTERVAL_MS: u64 = 1000;
/// 连续失败后的重试间隔（仍然只在 tab 可见时）
const FAIL_RETRY_MS: u64 = 5000;
/// 连续多少个空样本判定为「不可读」（nettop 在但拿不到东西）
const FAIL_THRESHOLD: u32 = 3;

pub struct NetSpeedState {
    sample: Mutex<Option<NetSpeedSample>>,
    /// 上一轮累计值 + 采样时刻（差分基线）
    prev: Mutex<Option<(HashMap<i32, RawBytes>, Instant)>>,
    active: AtomicBool,
    failures: AtomicU32,
    last_attempt: Mutex<Option<Instant>>,
}

impl Default for NetSpeedState {
    fn default() -> Self {
        Self::new()
    }
}

impl NetSpeedState {
    pub fn new() -> Self {
        Self {
            sample: Mutex::new(None),
            prev: Mutex::new(None),
            active: AtomicBool::new(false),
            failures: AtomicU32::new(0),
            last_attempt: Mutex::new(None),
        }
    }

    pub fn sample(&self) -> Option<NetSpeedSample> {
        self.sample
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn set_active(&self, active: bool) {
        self.active.store(active, Ordering::Relaxed);
        if !active {
            self.reset();
        } else {
            // 重新进来：先给一个「采样中」，避免 UI 拿着上一次离开前的旧速率当现状
            *self.sample.lock().unwrap_or_else(|e| e.into_inner()) = None;
        }
        eprintln!("[m10-net] 可见性 → {active}（false 时不 spawn nettop）");
    }

    fn is_active(&self) -> bool {
        self.active.load(Ordering::Relaxed)
    }

    /// 清空基线：断点恢复后第一次差分不该跨越「离开的这段时间」
    fn reset(&self) {
        *self.prev.lock().unwrap_or_else(|e| e.into_inner()) = None;
        *self.sample.lock().unwrap_or_else(|e| e.into_inner()) = None;
        self.failures.store(0, Ordering::Relaxed);
    }

    /// 失败退避：连续读不到时别每秒都 spawn 一个必然失败的进程
    fn in_backoff(&self) -> bool {
        if self.failures.load(Ordering::Relaxed) < FAIL_THRESHOLD {
            return false;
        }
        let last = *self.last_attempt.lock().unwrap_or_else(|e| e.into_inner());
        last.map(|t| t.elapsed() < Duration::from_millis(FAIL_RETRY_MS))
            .unwrap_or(false)
    }
}

/// 1 s 采样循环（仅在「应用资源」tab 可见时推进）
pub async fn run_net_speed(app: AppHandle) {
    let mut tick = tokio::time::interval(Duration::from_millis(INTERVAL_MS));
    tick.tick().await;

    loop {
        tick.tick().await;
        let state = app.state::<Arc<NetSpeedState>>().inner().clone();
        if !state.is_active() {
            continue;
        }
        if state.in_backoff() {
            continue;
        }
        if let Err(e) = round(&app, &state).await {
            eprintln!("[m10-net] 采样失败: {e}");
        }
    }
}

/// 一轮采样：跑 nettop → 解析 → 与上一轮累计值差分 → 广播。
async fn round(app: &AppHandle, state: &Arc<NetSpeedState>) -> Result<(), String> {
    let prev = state.prev.lock().unwrap_or_else(|e| e.into_inner()).clone();

    *state.last_attempt.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());

    let read = tauri::async_runtime::spawn_blocking(read_nettop).await;
    let text = match read {
        Ok(Ok(text)) => text,
        Ok(Err(e)) => {
            return fail(app, state, &e);
        }
        Err(e) => {
            return fail(app, state, &format!("采样线程失败：{e}"));
        }
    };

    let cur = netspeed::parse_sample(&text);
    if cur.is_empty() {
        return fail(app, state, "nettop 输出解析为空（可能被系统版本改了格式）");
    }

    let now = Instant::now();
    let sample = match prev {
        Some((before, at)) => {
            let secs = now.duration_since(at).as_secs_f64();
            if secs <= 0.0 {
                NetSpeedSample::warmup(0)
            } else {
                let per_pid = netspeed::diff(&before, &cur, secs);
                let (total_down_bps, total_up_bps) = per_pid
                    .values()
                    .fold((0.0, 0.0), |(d, u), s| (d + s.down_bps, u + s.up_bps));
                NetSpeedSample {
                    at: now_secs(),
                    status: NetSpeedStatus::Ok,
                    note: None,
                    per_pid,
                    total_down_bps,
                    total_up_bps,
                    interval_ms: (secs * 1000.0).round() as u64,
                }
            }
        }
        // 第一次采样只有起点没有终点：给 warmup，绝不给 0 B/s
        None => NetSpeedSample::warmup(INTERVAL_MS),
    };

    *state.prev.lock().unwrap_or_else(|e| e.into_inner()) = Some((cur, now));
    state.failures.store(0, Ordering::Relaxed);
    *state.sample.lock().unwrap_or_else(|e| e.into_inner()) = Some(sample.clone());

    let _ = app.emit(EVENT_NET_SPEED, &sample);
    Ok(())
}

/// 失败：连续 3 次才置 unavailable（偶发一次解析失败不该让整列变灰）
fn fail(app: &AppHandle, state: &Arc<NetSpeedState>, note: &str) -> Result<(), String> {
    let n = state.failures.fetch_add(1, Ordering::Relaxed) + 1;
    eprintln!("[m10-net] 第 {n} 次失败：{note}");
    if n < FAIL_THRESHOLD {
        return Ok(());
    }
    // 基线作废：恢复后不能拿故障前的老累计值做差分
    *state.prev.lock().unwrap_or_else(|e| e.into_inner()) = None;
    let sample = NetSpeedSample::unavailable(note);
    *state.sample.lock().unwrap_or_else(|e| e.into_inner()) = Some(sample.clone());
    let _ = app.emit(EVENT_NET_SPEED, &sample);
    Ok(())
}

#[cfg(target_os = "macos")]
fn read_nettop() -> Result<String, String> {
    use std::process::Command;
    let out = Command::new("nettop")
        .args([
            "-P", // 按进程聚合（不按 socket）
            "-x", // 不解析端口/域名，快
            "-l",
            "1", // 只采一个样本
            "-t",
            "external", // 排除 loopback：这是「网速」，不是本机回环流量
            "-J",
            "bytes_in,bytes_out",
        ])
        .output()
        .map_err(|e| format!("nettop 不可用：{e}"))?;
    if !out.status.success() {
        return Err(format!(
            "nettop 退出码 {:?}",
            out.status.code().unwrap_or(-1)
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

#[cfg(not(target_os = "macos"))]
fn read_nettop() -> Result<String, String> {
    // Windows 走 ETW 实时会话才有 per-process 字节，成本与权限都超出本里程碑
    Err("当前平台不支持按进程网速（Windows 需要 ETW 实时会话）".to_string())
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------- IPC

#[tauri::command]
pub fn net_speed_snapshot(state: State<'_, Arc<NetSpeedState>>) -> Option<NetSpeedSample> {
    state.sample()
}

/// 可见性门控：进入「应用资源」tab 传 true，离开传 false（连带清空基线）
#[tauri::command]
pub fn net_speed_set_active(state: State<'_, Arc<NetSpeedState>>, active: bool) {
    state.set_active(active);
}
