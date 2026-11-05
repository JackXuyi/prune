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
//!
//! ---- M13 追加：菜单栏数字的总网速（[`TotalNetState`]）----
//!
//! 菜单栏数字要**常驻**，不能复用上面的 tab 门控 + nettop 设计
//! （nettop 一次性采样 14.5 ms/次 ≈ 1.5% 单核，常驻不可接受）。
//! 改用 `sysinfo::Networks` 纯内存读计数器（<1 ms/次，跨平台），
//! 与按进程采样完全解耦：tab 采样随时可停，菜单栏数字不受影响。

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

// ---- M13 · 菜单栏数字：常驻总网速 ----

/// 2 s：sysinfo 读数的折中——数字在跳、又不至于频繁到浪费
pub const TOTAL_INTERVAL_MS: u64 = 2000;

/// 排除的网卡名（小写匹配）：本机回环不是「网速」，口径与 nettop `-t external` 对齐
fn is_loopback_iface(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n == "lo" || n == "lo0" || n.starts_with("loopback")
}

/// 菜单栏数字的总网速状态：最近一次 (下载 B/s, 上传 B/s)。None = 尚无读数
pub struct TotalNetState {
    pub(crate) total: Mutex<Option<(f64, f64)>>,
}

impl Default for TotalNetState {
    fn default() -> Self {
        Self::new()
    }
}

impl TotalNetState {
    pub fn new() -> Self {
        Self {
            total: Mutex::new(None),
        }
    }
}

/// 汇总一轮 sysinfo 读数（纯函数，可测）：排除回环，(字节差, 采样秒数) → (down/up B/s)。
/// `elapsed_secs <= 0` 返回 None（时钟回拨等异常，宁可缺一拍也不冒充 0）。
fn sum_total_speed(
    nets: &sysinfo::Networks,
    elapsed_secs: f64,
) -> Option<(f64, f64)> {
    if elapsed_secs <= 0.0 {
        return None;
    }
    let (mut down, mut up) = (0.0f64, 0.0f64);
    for (name, data) in nets {
        if is_loopback_iface(name) {
            continue;
        }
        down += data.received() as f64;
        up += data.transmitted() as f64;
    }
    Some((down / elapsed_secs, up / elapsed_secs))
}

/// 2 s 常驻循环（setup 时 spawn 一次）：读总网速 → 存状态 → 通知 tray 刷新菜单栏标题。
/// 失败静默降级为「本拍不更新」，绝不 panic 打断循环。
pub async fn run_total_speed(app: AppHandle) {
    let mut nets = sysinfo::Networks::new_with_refreshed_list();
    let mut tick = tokio::time::interval(Duration::from_millis(TOTAL_INTERVAL_MS));
    tick.tick().await; // 消费掉 interval 的立即触发

    // 差分窗口起点：received()/transmitted() 是「自上次 refresh 以来的字节」，
    // 分母必须是两次 refresh 之间的真实时长（≈ 一个 interval 周期）。
    // ⚠️ 不能用 refresh() 调用本身的耗时当分母——那是亚毫秒级，会把速率放大上千倍
    // （实测 2.0 MB/s 被算成 2914 MB/s，就是踩了这个坑）。
    let mut window_started = std::time::Instant::now();

    loop {
        tick.tick().await;
        let elapsed_secs = window_started.elapsed().as_secs_f64();
        window_started = std::time::Instant::now();
        nets.refresh(true); // 移除已消失的网卡（拔网线 / 切 VPN）
        let Some((down, up)) = sum_total_speed(&nets, elapsed_secs) else {
            continue;
        };
        {
            let state = app.state::<Arc<TotalNetState>>();
            *state.total.lock().unwrap_or_else(|e| e.into_inner()) = Some((down, up));
        }
        crate::tray::update_title_network(&app, Some((down, up)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 回环过滤：macOS lo0 / Windows Loopback / Linux lo 都要滤掉，
    /// 真实网卡（en0 / Ethernet）必须保留——滤错最多把 127.0.0.1 算进网速
    #[test]
    fn loopback_interfaces_are_filtered() {
        assert!(is_loopback_iface("lo0"));
        assert!(is_loopback_iface("lo"));
        assert!(is_loopback_iface("Loopback Pseudo-Interface 1"));
        assert!(!is_loopback_iface("en0"));
        assert!(!is_loopback_iface("Ethernet"));
    }

    /// 异常时长（时钟回拨 → <= 0）宁可缺一拍，不能除零 / 冒充 0 速率
    #[test]
    fn non_positive_elapsed_gives_none() {
        let nets = sysinfo::Networks::new();
        assert!(sum_total_speed(&nets, 0.0).is_none());
        assert!(sum_total_speed(&nets, -1.0).is_none());
    }

    /// 空表（尚无网卡读数）+ 正常时长 = 速率 0，而不是 None——
    /// 菜单栏显示 `0K` 比「字段消失」更符合直觉
    #[test]
    fn empty_interfaces_yield_zero_rate() {
        let nets = sysinfo::Networks::new();
        let (down, up) = sum_total_speed(&nets, 2.0).expect("正常时长必须有读数");
        assert_eq!((down, up), (0.0, 0.0));
    }
}
