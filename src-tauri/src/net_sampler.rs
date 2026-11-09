//! 网速采样。
//!
//! # M10：按进程网速（[`NetSpeedState`]）
//!
//! 1 s 轮询 `nettop` 一次性采样，累计字节差分。
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
//! # M13/M14：系统口径总网速（[`TotalNetState`]）
//!
//! 菜单栏数字要**常驻**，不能复用上面的 tab 门控 + nettop 设计
//! （nettop 一次性采样 14.5 ms/次 ≈ 1.5% 单核，常驻不可接受）。
//! 改用 `sysinfo::Networks` 纯内存读计数器（<1 ms/次，跨平台），
//! 与按进程采样完全解耦：tab 采样随时可停，菜单栏数字不受影响。
//!
//! ## P0 修复（M14）：分母曾经是 `refresh()` 的耗时
//!
//! M13 那版写成 `sum_total_speed(&nets, started.elapsed())`，而
//! `NetworkData::received()` 的语义是「**自上次 refresh 起**的增量」——
//! 分母本该是两次采样的真实间隔（1–2 s），实际却是 refresh 自身的
//! 0.1–1 ms。实测后果：**2.0 MB/s 被算成 2914 MB/s（约 1457 倍）**，
//! 菜单栏那些 `2.1M` 很可能其实是 `2 KB/s`。
//!
//! 现在改用 `total_received()` / `total_transmitted()`（自开机累计）自行差分：
//! 分母 = `now - prev_at`，漏拍、延迟 tick、睡眠唤醒都不会失真；
//! 负增量（网卡重置 / 计数器回绕）记 0 并重置该口基线，不取绝对值。
//!
//! ## 今日流量
//!
//! 内存逐拍累加（UI 每秒准），每 60 s + 跨天时把「上次之后的新增」增量 UPSERT
//! 进 `net_daily`（见 `prune_core::db::Store::net_add`）。写的是增量相加而不是
//! 覆盖，所以重启不清零；也正因为是相加，只允许本线程提交（重复提交会双计）。

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use prune_core::db::Store;
use prune_core::net::netspeed::{self, NetSpeedSample, NetSpeedStatus, RawBytes};
use prune_core::net::traffic::{
    self, DayTraffic, IfaceKind, IfaceRate, NetTotalPoint, NetTotalSample, NetTotalStatus,
};
use tauri::{AppHandle, Emitter, Manager, State};

pub const EVENT_NET_SPEED: &str = "net://speed";
/// 采样间隔。1 s 是「数字看起来在跳」与「nettop 自耗可忽略」的折中
/// （14.5 ms/次 → 约 1.5% 单核占用，只在 tab 可见时跑）
pub const INTERVAL_MS: u64 = 1000;
/// 连续失败后的重试间隔（仍然只在 tab 可见时）
const FAIL_RETRY_MS: u64 = 5000;

/// D3：后台低频按应用网速采样间隔。
///
/// 1 s 采样只在 tab 可见时跑；历史要的是「这段时间各应用用了多少流量」，
/// 60 s 一轮就够（nettop 一次 ≈ 15 ms，折合约 0.03% 单核），且**不推 UI 事件**。
pub const APP_NET_HISTORY_INTERVAL_SECS: u64 = 60;
/// 单轮预算：低电量模式下 nettop 变慢时直接跳过，不堆积
const APP_NET_ROUND_TIMEOUT_SECS: u64 = 2;
/// 历史累计的开关（settings.json；默认开）
pub const SETTING_APP_HISTORY_NET: &str = "appHistoryNet";
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

// ---------------------------------------------------------------- D3：后台流量历史

/// 后台低频轮（60 s）：按家族把流量增量累加进小时历史。
///
/// 与 1 s 轮**完全解耦**：不读不写 `NetSpeedState`，也不广播事件。
/// 关掉 `appHistoryNet` 时基线作废——重开后第一次差分不该把停掉的这段时间
/// 算成一波暴涨。
pub async fn run_app_net_history(app: AppHandle) {
    let mut tick = tokio::time::interval(Duration::from_secs(APP_NET_HISTORY_INTERVAL_SECS));
    tick.tick().await;
    let mut prev: Option<(HashMap<i32, RawBytes>, Instant)> = None;

    loop {
        tick.tick().await;
        if !crate::settings::get_bool(&app, SETTING_APP_HISTORY_NET).unwrap_or(true) {
            prev = None;
            continue;
        }
        if let Err(e) = app_net_round(&app, &mut prev).await {
            eprintln!("[d3-history] 应用流量采样失败: {e}");
        }
    }
}

async fn app_net_round(
    app: &AppHandle,
    prev: &mut Option<(HashMap<i32, RawBytes>, Instant)>,
) -> Result<(), String> {
    let read = tokio::time::timeout(
        Duration::from_secs(APP_NET_ROUND_TIMEOUT_SECS),
        tauri::async_runtime::spawn_blocking(read_nettop),
    )
    .await;

    let text = match read {
        Ok(Ok(Ok(text))) => text,
        Ok(Ok(Err(e))) => return Err(e),
        Ok(Err(e)) => return Err(format!("采样线程失败：{e}")),
        Err(_) => {
            return Err(format!(
                "nettop 超过 {APP_NET_ROUND_TIMEOUT_SECS} s，跳过本轮"
            ))
        }
    };

    let cur = netspeed::parse_sample(&text);
    if cur.is_empty() {
        return Err("nettop 输出解析为空".into());
    }

    let now = Instant::now();
    let Some((before, at)) = prev.take() else {
        *prev = Some((cur, now));
        return Ok(()); // 首轮只建基线
    };
    let secs = now.duration_since(at).as_secs_f64();
    if secs <= 0.0 {
        *prev = Some((cur, now));
        return Ok(());
    }

    let per_pid = netspeed::diff(&before, &cur, secs);
    *prev = Some((cur, now));

    // pid → family：复用上一轮快照的进程表，不额外枚举进程
    let mem = app
        .state::<Arc<crate::sampler::MemState>>()
        .inner()
        .clone();
    let families: HashMap<i32, String> = mem
        .processes
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .map(|p| (p.pid, p.family.clone()))
        .collect();

    let ts = now_secs();
    let mut touched = 0usize;
    for (pid, speed) in per_pid {
        let Some(family) = families.get(&pid) else {
            continue;
        };
        let down = (speed.down_bps.max(0.0) * secs) as u64;
        let up = (speed.up_bps.max(0.0) * secs) as u64;
        if down == 0 && up == 0 {
            continue;
        }
        mem.add_app_net(family, down, up, ts);
        touched += 1;
    }

    if touched > 0 {
        eprintln!("[d3-history] 应用流量累加 {touched} 个进程（{secs:.1} s）");
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn read_nettop() -> Result<String, String> {    use std::process::Command;
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

// ================================================================
// M14 · 网络 tab：系统口径总网速 + 每日流量
// ================================================================

pub const EVENT_NET_TOTAL: &str = "net://total";

/// 常驻节拍（网络 tab 不可见）：2 s。与 M13 一致，菜单栏数字够用
pub const TOTAL_INTERVAL_MS: u64 = 2000;
/// 网络 tab 可见时提速到 1 s：主数字看起来在跳
const TOTAL_ACTIVE_INTERVAL_MS: u64 = 1000;
/// 内部 tick 粒度。500 ms 才能既支持 1 s 也不漂成 3 s
const TICK_MS: u64 = 500;
/// 分母下限：小于这个间隔的拍丢弃（重复 tick / 时钟异常），不除零
const MIN_INTERVAL_SECS: f64 = 0.2;
/// 环形缓冲容量：1 s 一点 ≈ 15 min
const RING_CAP: usize = 900;
/// IPC 返回的走势点数上限（900 s 窗口降采样到 120 点，前端画得动）
const HISTORY_MAX_POINTS: usize = 120;
/// 今日流量落库间隔
const FLUSH_INTERVAL_SECS: i64 = 60;
/// net_daily 保留天数
const RETAIN_DAYS: i64 = 90;
/// 连续多少拍读不到接口才判定 unavailable
const READ_FAIL_THRESHOLD: u32 = 3;

/// 单张网卡的累计计数（差分基线 + 本次会话增量）
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Totals {
    rx: u64,
    tx: u64,
    /// 本次会话该网卡的正增量累计
    sess_rx: u64,
    sess_tx: u64,
}

/// 一拍差分的产物（纯函数返回值，方便单测）
#[derive(Debug, Clone)]
struct TickDiff {
    /// 下一拍的基线
    next: HashMap<String, Totals>,
    ifaces: Vec<IfaceRate>,
    down_bps: f64,
    up_bps: f64,
    /// 物理口本拍的字节增量（会话 / 今日累计用）
    down_bytes: u64,
    up_bytes: u64,
}

/// 系统口径总网速状态。一个实例被 `Arc` 管起来，采样循环与 IPC 共读。
pub struct TotalNetState {
    sample: Mutex<Option<NetTotalSample>>,
    /// 走势环形缓冲（warmup 也记 0 点，曲线从第一秒开始长）
    ring: Mutex<VecDeque<NetTotalPoint>>,
    /// 上一拍各网卡的累计值。None = 还没有基线（首拍 / 刚从不可用恢复）
    prev: Mutex<Option<HashMap<String, Totals>>>,
    /// 本次会话累计（仅物理口）
    session: Mutex<(u64, u64)>,
    /// 本次会话峰值 (down, up, at)。与走势窗口解耦，否则「峰值」名不副实
    peak: Mutex<(f64, f64, i64)>,
    /// 网络 tab 是否可见（只改节拍，不清基线）
    active: AtomicBool,
    /// 今日流量。None = 没有库（今日不可用）——**不是 0**
    today: Mutex<Option<DayTraffic>>,
    /// 上次落库之后的新增（尚未写进 SQLite 的部分）
    pending: Mutex<(u64, u64)>,
    /// 上次成功落库时刻（0 = 还没落过）
    persisted_at: Mutex<i64>,
    /// 上次尝试落库时刻（失败后按 60 s 退避，而不是每秒重试）
    flush_attempt_at: Mutex<i64>,
    flush_failures: AtomicU32,
    flush_err: Mutex<Option<String>>,
    last_prune_day: Mutex<Option<String>>,
    /// 序列库句柄。None = 库打不开 → 今日流量显示「不可用」，实时与会话照常
    store: Option<Arc<Store>>,
}

impl TotalNetState {
    pub fn new(store: Option<Arc<Store>>) -> Self {
        let today = match &store {
            // 没有库：今日就是「不可用」，不是 0
            None => None,
            Some(s) => {
                let day = traffic::local_day(now_secs());
                match s.net_day(&day) {
                    // 当天还没落过库 → 从 0 起（这是「今天确实还没有流量」的正确表达）
                    Ok(row) => Some(row.unwrap_or_else(|| DayTraffic::zero(day))),
                    Err(e) => {
                        eprintln!("[m14-net] 读今日流量失败（先按 0 起算，落库会继续重试）: {e}");
                        Some(DayTraffic::zero(day))
                    }
                }
            }
        };
        Self {
            sample: Mutex::new(None),
            ring: Mutex::new(VecDeque::with_capacity(RING_CAP)),
            prev: Mutex::new(None),
            session: Mutex::new((0, 0)),
            peak: Mutex::new((0.0, 0.0, 0)),
            active: AtomicBool::new(false),
            today: Mutex::new(today),
            pending: Mutex::new((0, 0)),
            persisted_at: Mutex::new(0),
            flush_attempt_at: Mutex::new(0),
            flush_failures: AtomicU32::new(0),
            flush_err: Mutex::new(None),
            last_prune_day: Mutex::new(None),
            store,
        }
    }

    pub fn snapshot(&self) -> Option<NetTotalSample> {
        self.sample.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// 网络 tab 还开着才提速到 1 s；切走只降节拍，**不清基线**
    /// （与 `net_speed_set_active` 语义不同：那个是「不 spawn 子进程」的开关）
    pub fn set_active(&self, active: bool) {
        self.active.store(active, Ordering::Relaxed);
        eprintln!(
            "[m14-net] 网络 tab 可见性 → {active}（节拍 {} ms）",
            if active {
                TOTAL_ACTIVE_INTERVAL_MS
            } else {
                TOTAL_INTERVAL_MS
            }
        );
    }

    fn is_active(&self) -> bool {
        self.active.load(Ordering::Relaxed)
    }

    /// 走势窗口：`seconds` 内降采样到 ≤ 120 点（桶内**均值**，不是最大值）
    pub fn history(&self, seconds: u32) -> Vec<NetTotalPoint> {
        let cutoff = now_secs() - seconds.clamp(1, 900) as i64;
        let ring = self.ring.lock().unwrap_or_else(|e| e.into_inner());
        let pts: Vec<NetTotalPoint> = ring.iter().copied().filter(|p| p.at >= cutoff).collect();
        downsample(&pts, HISTORY_MAX_POINTS)
    }

    /// 近 N 天每日流量（升序，只含有记录的日子）。
    ///
    /// 今日以**内存**为准：库里最多滞后 60 s，不覆盖会让「今日」与 7 日柱对不上。
    pub fn recent(&self, days: u32) -> Vec<DayTraffic> {
        let Some(store) = &self.store else {
            return Vec::new();
        };
        let days = days.clamp(1, RETAIN_DAYS as u32) as i64;
        let from = traffic::local_day(now_secs() - (days - 1) * 86_400);
        let mut rows = match store.net_recent(&from) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[m14-net] 读每日流量失败: {e}");
                Vec::new()
            }
        };
        if let Some(today) = self.today.lock().unwrap_or_else(|e| e.into_inner()).clone() {
            match rows.iter_mut().find(|d| d.day == today.day) {
                Some(row) => *row = today,
                None => rows.push(today),
            }
        }
        rows.sort_by(|a, b| a.day.cmp(&b.day));
        rows
    }

    /// 退出前 best-effort 落库（`RunEvent::Exit` 调，最多丢 60 s）
    pub fn flush_on_exit(&self) {
        let today = self.today.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let Some(today) = today else { return };
        if self.flush(&today.day, now_secs()) {
            eprintln!("[m14-net] 退出前已落库：{}", today.day);
        }
    }

    /// 采一拍。返回可直接广播的样本（纯内存操作 + 至多一次单行 UPSERT）。
    fn record(&self, raw: HashMap<String, Totals>, dt: f64, now: i64) -> NetTotalSample {
        // 1) 跨天判定必须在累加之前：切天那一拍的增量归**新日**（误差 ≤ 1 拍）
        self.roll_day(now);

        let prev = self.prev.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let warmup = prev.is_none();
        let tick = tick_diff(prev.as_ref(), &raw, dt);
        *self.prev.lock().unwrap_or_else(|e| e.into_inner()) = Some(tick.next);

        // 2) 会话累计 + 峰值（首拍增量为 0，天然安全）
        {
            let mut s = self.session.lock().unwrap_or_else(|e| e.into_inner());
            s.0 = s.0.saturating_add(tick.down_bytes);
            s.1 = s.1.saturating_add(tick.up_bytes);
        }
        {
            let mut p = self.peak.lock().unwrap_or_else(|e| e.into_inner());
            if tick.down_bps > p.0 {
                p.0 = tick.down_bps;
                p.2 = now;
            }
            if tick.up_bps > p.1 {
                p.1 = tick.up_bps;
                p.2 = now;
            }
        }

        // 3) 今日累加（仅物理口；无库时不维护 pending）
        if self.store.is_some() {
            {
                let mut guard = self.today.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(t) = guard.as_mut() {
                    t.add(tick.down_bytes, tick.up_bytes);
                }
            }
            if tick.down_bytes > 0 || tick.up_bytes > 0 {
                let mut p = self.pending.lock().unwrap_or_else(|e| e.into_inner());
                p.0 = p.0.saturating_add(tick.down_bytes);
                p.1 = p.1.saturating_add(tick.up_bytes);
            }
        }
        self.maybe_flush(now);

        // 4) 组装样本。首拍只推进基线：给 warmup，绝不把「开机累计 / dt」当速率
        let (status, down_bps, up_bps) = if warmup {
            (NetTotalStatus::Warmup, 0.0, 0.0)
        } else {
            (NetTotalStatus::Ok, tick.down_bps, tick.up_bps)
        };
        // note 只承载「必须让用户知道」的原因：连续 2 次落库失败才提示
        let note = if self.flush_failures.load(Ordering::Relaxed) >= 2 {
            self.flush_err.lock().unwrap_or_else(|e| e.into_inner()).clone()
        } else {
            None
        };
        let (session_down, session_up) = *self.session.lock().unwrap_or_else(|e| e.into_inner());
        let (peak_down, peak_up, peak_at) = *self.peak.lock().unwrap_or_else(|e| e.into_inner());
        let sample = NetTotalSample {
            at: now,
            status,
            note,
            down_bps,
            up_bps,
            interval_ms: (dt * 1000.0).round() as u64,
            ifaces: tick.ifaces,
            session_down_bytes: session_down,
            session_up_bytes: session_up,
            peak_down_bps: peak_down,
            peak_up_bps: peak_up,
            peak_at,
            today: self.today.lock().unwrap_or_else(|e| e.into_inner()).clone(),
            today_persisted_at: *self.persisted_at.lock().unwrap_or_else(|e| e.into_inner()),
        };

        {
            let mut ring = self.ring.lock().unwrap_or_else(|e| e.into_inner());
            while ring.len() >= RING_CAP {
                ring.pop_front();
            }
            ring.push_back(NetTotalPoint {
                at: now,
                down_bps,
                up_bps,
            });
        }
        *self.sample.lock().unwrap_or_else(|e| e.into_inner()) = Some(sample.clone());
        sample
    }

    /// 连续读不到接口：置 unavailable 并作废基线（恢复后第一拍是 warmup，不是 0）
    fn mark_unavailable(&self, note: &str, now: i64) -> NetTotalSample {
        *self.prev.lock().unwrap_or_else(|e| e.into_inner()) = None;
        let (session_down, session_up) = *self.session.lock().unwrap_or_else(|e| e.into_inner());
        let (peak_down, peak_up, peak_at) = *self.peak.lock().unwrap_or_else(|e| e.into_inner());
        let sample = NetTotalSample {
            at: now,
            status: NetTotalStatus::Unavailable,
            note: Some(note.to_string()),
            down_bps: 0.0,
            up_bps: 0.0,
            interval_ms: 0,
            ifaces: Vec::new(),
            session_down_bytes: session_down,
            session_up_bytes: session_up,
            peak_down_bps: peak_down,
            peak_up_bps: peak_up,
            peak_at,
            today: self.today.lock().unwrap_or_else(|e| e.into_inner()).clone(),
            today_persisted_at: *self.persisted_at.lock().unwrap_or_else(|e| e.into_inner()),
        };
        *self.sample.lock().unwrap_or_else(|e| e.into_inner()) = Some(sample.clone());
        sample
    }

    /// 跨天：先把旧日的 pending 落库，再切到新日从 0 计。返回是否发生了切天。
    ///
    /// 落库失败时**不清 pending**（数据比归日更重要）：那批增量会被记到新的一天，
    /// 误差上限同样是 60 s，且 `note` 会持续提示「未落库」。
    fn roll_day(&self, now: i64) -> bool {
        let day = traffic::local_day(now);
        let old = {
            let guard = self.today.lock().unwrap_or_else(|e| e.into_inner());
            match guard.as_ref() {
                Some(t) if t.day != day => Some(t.day.clone()),
                _ => None,
            }
        };
        let Some(old) = old else { return false };

        let flushed = self.flush(&old, now);
        *self.today.lock().unwrap_or_else(|e| e.into_inner()) = Some(DayTraffic::zero(day.clone()));
        *self.persisted_at.lock().unwrap_or_else(|e| e.into_inner()) = if flushed { now } else { 0 };
        eprintln!("[m14-net] 跨天：{old} → {day}（旧日已落库 = {flushed}）");
        true
    }

    /// 落库节流：每 60 s 把「上次之后的新增」UPSERT 一次
    fn maybe_flush(&self, now: i64) {
        let pending = *self.pending.lock().unwrap_or_else(|e| e.into_inner());
        if pending == (0, 0) {
            return;
        }
        let attempted = *self.flush_attempt_at.lock().unwrap_or_else(|e| e.into_inner());
        if now - attempted < FLUSH_INTERVAL_SECS {
            return;
        }
        let day = match self.today.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            Some(t) => t.day.clone(),
            None => return,
        };
        self.flush(&day, now);
    }

    /// 把 pending 写进 `day` 这一行。返回是否成功（无库 / 无增量时按「没有可失败的东西」处理）。
    fn flush(&self, day: &str, now: i64) -> bool {
        let Some(store) = &self.store else {
            return false;
        };
        let (down, up) = *self.pending.lock().unwrap_or_else(|e| e.into_inner());
        if down == 0 && up == 0 {
            return true;
        }
        *self.flush_attempt_at.lock().unwrap_or_else(|e| e.into_inner()) = now;
        let res = store.net_add(day, down, up, now).map_err(|e| e.to_string());
        self.apply_flush(day, now, down, up, res)
    }

    /// 落库结果记账（抽出来是为了能在没有「真实坏库」的情况下单测失败路径）
    fn apply_flush(
        &self,
        day: &str,
        now: i64,
        down: u64,
        up: u64,
        res: Result<(), String>,
    ) -> bool {
        match res {
            Ok(()) => {
                {
                    let mut p = self.pending.lock().unwrap_or_else(|e| e.into_inner());
                    p.0 = p.0.saturating_sub(down);
                    p.1 = p.1.saturating_sub(up);
                }
                *self.persisted_at.lock().unwrap_or_else(|e| e.into_inner()) = now;
                *self.flush_err.lock().unwrap_or_else(|e| e.into_inner()) = None;
                self.flush_failures.store(0, Ordering::Relaxed);
                self.prune_once(day, now);
                true
            }
            Err(e) => {
                let n = self.flush_failures.fetch_add(1, Ordering::Relaxed) + 1;
                eprintln!("[m14-net] 每日流量落库失败（第 {n} 次）: {e}");
                *self.flush_err.lock().unwrap_or_else(|e| e.into_inner()) =
                    Some(format!("今日流量未落库：{e}"));
                false
            }
        }
    }

    /// 保留期清理：每天最多跑一次（flush 路径顺带）
    fn prune_once(&self, day: &str, now: i64) {
        let Some(store) = &self.store else { return };
        {
            let mut last = self.last_prune_day.lock().unwrap_or_else(|e| e.into_inner());
            if last.as_deref() == Some(day) {
                return;
            }
            *last = Some(day.to_string());
        }
        let cutoff = traffic::local_day(now - RETAIN_DAYS * 86_400);
        if let Err(e) = store.net_prune_before(&cutoff) {
            eprintln!("[m14-net] 清理过期流量失败: {e}");
        }
    }
}

impl Default for TotalNetState {
    fn default() -> Self {
        Self::new(None)
    }
}

/// 从 `sysinfo` 读各网卡的**自开机累计**字节（回环直接丢弃：不是「网速」）
fn read_totals(nets: &sysinfo::Networks) -> HashMap<String, Totals> {
    let mut out = HashMap::with_capacity(nets.len());
    for (name, data) in nets {
        if traffic::classify_iface(name) == IfaceKind::Loopback {
            continue;
        }
        out.insert(
            name.clone(),
            Totals {
                rx: data.total_received(),
                tx: data.total_transmitted(),
                sess_rx: 0,
                sess_tx: 0,
            },
        );
    }
    out
}

/// 累计计数差分（纯函数，可测）。
///
/// - 新出现的接口首拍没有基线 → 该口记 0（**不拿开机累计当速率**）
/// - 负增量（网卡重置 / 计数器回绕）→ 记 0，并把基线重置为当前值
/// - 只有物理口进 `down_bps` / `down_bytes`
fn tick_diff(
    prev: Option<&HashMap<String, Totals>>,
    raw: &HashMap<String, Totals>,
    dt: f64,
) -> TickDiff {
    let mut next = HashMap::with_capacity(raw.len());
    let mut ifaces = Vec::with_capacity(raw.len());
    let (mut down_bps, mut up_bps) = (0.0f64, 0.0f64);
    let (mut down_bytes, mut up_bytes) = (0u64, 0u64);

    for (name, cur) in raw {
        let base = prev.and_then(|p| p.get(name));
        let drx = base.map(|b| cur.rx.saturating_sub(b.rx)).unwrap_or(0);
        let dtx = base.map(|b| cur.tx.saturating_sub(b.tx)).unwrap_or(0);
        let sess_rx = base.map(|b| b.sess_rx).unwrap_or(0).saturating_add(drx);
        let sess_tx = base.map(|b| b.sess_tx).unwrap_or(0).saturating_add(dtx);
        next.insert(
            name.clone(),
            Totals {
                rx: cur.rx,
                tx: cur.tx,
                sess_rx,
                sess_tx,
            },
        );

        let kind = traffic::classify_iface(name);
        if !kind.visible() {
            continue;
        }
        let (d, u) = if dt > 0.0 {
            (drx as f64 / dt, dtx as f64 / dt)
        } else {
            (0.0, 0.0)
        };
        if kind.counted() {
            down_bps += d;
            up_bps += u;
            down_bytes = down_bytes.saturating_add(drx);
            up_bytes = up_bytes.saturating_add(dtx);
        }
        ifaces.push(IfaceRate {
            name: name.clone(),
            kind,
            down_bps: d,
            up_bps: u,
            session_down_bytes: sess_rx,
            session_up_bytes: sess_tx,
        });
    }

    // 物理口在前，同类按速率降序，最后按名称保证稳定顺序
    ifaces.sort_by(|a, b| {
        let rank = |k: IfaceKind| if k == IfaceKind::Physical { 0u8 } else { 1u8 };
        rank(a.kind)
            .cmp(&rank(b.kind))
            .then(
                b.total_bps()
                    .partial_cmp(&a.total_bps())
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
            .then_with(|| a.name.cmp(&b.name))
    });

    TickDiff {
        next,
        ifaces,
        down_bps,
        up_bps,
        down_bytes,
        up_bytes,
    }
}

/// 降采样到 ≤ `max` 点，桶内取**均值**（取最大值会把尖峰放大成「一直很快」）
fn downsample(points: &[NetTotalPoint], max: usize) -> Vec<NetTotalPoint> {
    if max == 0 {
        return Vec::new();
    }
    if points.len() <= max {
        return points.to_vec();
    }
    let bucket = points.len().div_ceil(max);
    points
        .chunks(bucket)
        .map(|chunk| {
            let n = chunk.len() as f64;
            NetTotalPoint {
                // 取桶内最后一个时刻：曲线右端才对得上「现在」
                at: chunk[chunk.len() - 1].at,
                down_bps: chunk.iter().map(|p| p.down_bps).sum::<f64>() / n,
                up_bps: chunk.iter().map(|p| p.up_bps).sum::<f64>() / n,
            }
        })
        .collect()
}

/// 常驻循环（setup 时 spawn 一次）：读计数器 → 差分 → 存状态 → 广播 → 刷新菜单栏标题。
///
/// 走 500 ms 的内部 tick，按 `active` 决定 1 s / 2 s 的**目标间隔**：
/// 切 tab 只改节拍，不新起第二个循环，也不会出现「两套循环各算一遍、
/// 菜单栏数字和页面数字不一样」。
pub async fn run_total_speed(app: AppHandle) {
    let mut nets = sysinfo::Networks::new_with_refreshed_list();
    let mut tick = tokio::time::interval(Duration::from_millis(TICK_MS));
    tick.tick().await; // 消费掉 interval 的立即触发

    let mut last_at = Instant::now();
    let mut read_failures = 0u32;

    loop {
        tick.tick().await;
        let state = app.state::<Arc<TotalNetState>>().inner().clone();
        let target = Duration::from_millis(if state.is_active() {
            TOTAL_ACTIVE_INTERVAL_MS
        } else {
            TOTAL_INTERVAL_MS
        });
        if last_at.elapsed() < target {
            continue;
        }

        nets.refresh(true); // 移除已消失的网卡（拔网线 / 切 VPN）
        let now_at = Instant::now();
        let dt = now_at.duration_since(last_at).as_secs_f64();
        if dt < MIN_INTERVAL_SECS {
            // 重复 tick / 时钟异常：丢弃这一拍，但**不推进基线时刻**
            continue;
        }
        last_at = now_at;

        let raw = read_totals(&nets);
        if raw.is_empty() {
            read_failures += 1;
            if read_failures >= READ_FAIL_THRESHOLD {
                let sample = state.mark_unavailable("读不到网卡计数器（接口列表为空）", now_secs());
                let _ = app.emit(EVENT_NET_TOTAL, &sample);
                crate::tray::update_title_network(&app, None);
            }
            continue;
        }
        read_failures = 0;

        let sample = state.record(raw, dt, now_secs());
        let _ = app.emit(EVENT_NET_TOTAL, &sample);
        // 菜单栏与页面共用同一拍数字：warmup / unavailable 时不给数字，
        // 免得菜单栏显示一个页面不承认的速率
        let tray = if sample.status == NetTotalStatus::Ok {
            Some((sample.down_bps, sample.up_bps))
        } else {
            None
        };
        crate::tray::update_title_network(&app, tray);
    }
}

// ---------------------------------------------------------------- IPC（M14）

#[tauri::command]
pub fn net_total_snapshot(state: State<'_, Arc<TotalNetState>>) -> Option<NetTotalSample> {
    state.snapshot()
}

#[tauri::command]
pub fn net_total_history(state: State<'_, Arc<TotalNetState>>, seconds: u32) -> Vec<NetTotalPoint> {
    state.history(seconds)
}

#[tauri::command]
pub fn net_traffic_recent(state: State<'_, Arc<TotalNetState>>, days: u32) -> Vec<DayTraffic> {
    state.recent(days)
}

/// 只改节拍（1 s ↔ 2 s），不清基线——注意与 `net_speed_set_active` 的区别
#[tauri::command]
pub fn net_total_set_active(state: State<'_, Arc<TotalNetState>>, active: bool) {
    state.set_active(active);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn totals(rx: u64, tx: u64) -> Totals {
        Totals {
            rx,
            tx,
            sess_rx: 0,
            sess_tx: 0,
        }
    }

    fn raw(items: &[(&str, u64, u64)]) -> HashMap<String, Totals> {
        items
            .iter()
            .map(|(n, rx, tx)| ((*n).to_string(), totals(*rx, *tx)))
            .collect()
    }

    /// 首拍只有基线：必须给 0 而不是「开机至今的累计 / 1 秒」这种天文数字
    #[test]
    fn first_sight_has_no_rate() {
        let cur = raw(&[("en0", 1_000_000, 200_000)]);
        let t = tick_diff(None, &cur, 1.0);
        assert_eq!((t.down_bps, t.up_bps), (0.0, 0.0));
        assert_eq!((t.down_bytes, t.up_bytes), (0, 0));
        assert_eq!(t.next["en0"].rx, 1_000_000, "基线必须推进");
    }

    /// 第二拍：速率 = 增量 / 真实间隔（这正是 P0 修复的核心）
    #[test]
    fn second_tick_computes_rate_from_real_interval() {
        let first = raw(&[("en0", 1_000, 100)]);
        let prev = tick_diff(None, &first, 1.0).next;
        let second = raw(&[("en0", 3_000, 300)]);

        let t = tick_diff(Some(&prev), &second, 2.0);
        assert_eq!(t.down_bps, 1_000.0, "2000 B / 2 s");
        assert_eq!(t.up_bps, 100.0);
        assert_eq!((t.down_bytes, t.up_bytes), (2_000, 200));
        assert_eq!(t.ifaces[0].session_down_bytes, 2_000);
    }

    /// 计数器回绕 / 网卡重置：负增量记 0，绝不取绝对值变成超大速率
    #[test]
    fn counter_reset_is_zero_not_wrapped() {
        let first = raw(&[("en0", 5_000, 5_000)]);
        let prev = tick_diff(None, &first, 1.0).next;
        let reset = raw(&[("en0", 10, 10)]);

        let t = tick_diff(Some(&prev), &reset, 1.0);
        assert_eq!((t.down_bps, t.up_bps), (0.0, 0.0));
        assert_eq!(t.next["en0"].rx, 10, "基线重置为当前值");
    }

    /// 虚拟口展示但不计入主数字：否则开 VPN 时主数字接近翻倍
    #[test]
    fn tunnel_and_virtual_are_shown_but_not_counted() {
        let first = raw(&[("en0", 0, 0), ("utun3", 0, 0), ("awdl0", 0, 0)]);
        let prev = tick_diff(None, &first, 1.0).next;
        let second = raw(&[
            ("en0", 1_000, 100),
            ("utun3", 1_000, 100),
            ("awdl0", 500, 50),
        ]);

        let t = tick_diff(Some(&prev), &second, 1.0);
        assert_eq!(t.down_bps, 1_000.0, "只有 en0 计入");
        assert_eq!(t.ifaces.len(), 3);
        assert_eq!(t.ifaces[0].name, "en0", "物理口排最前");
        assert!(t
            .ifaces
            .iter()
            .any(|i| i.name == "utun3" && i.kind == IfaceKind::Tunnel));
    }

    /// 回环既不展示也不计入（sysinfo 层已经滤掉，这里再兜一层）
    #[test]
    fn loopback_is_invisible() {
        let first = raw(&[("lo0", 0, 0)]);
        let prev = tick_diff(None, &first, 1.0).next;
        let t = tick_diff(Some(&prev), &raw(&[("lo0", 999_999, 999_999)]), 1.0);
        assert_eq!(t.down_bps, 0.0);
        assert!(t.ifaces.is_empty());
    }

    #[test]
    fn downsample_uses_bucket_mean_and_keeps_last_at() {
        let pts: Vec<NetTotalPoint> = (0..240)
            .map(|i| NetTotalPoint {
                at: i as i64,
                down_bps: i as f64,
                up_bps: 1.0,
            })
            .collect();
        let out = downsample(&pts, 120);
        assert_eq!(out.len(), 120);
        assert_eq!(out[0].down_bps, 0.5, "桶内均值（0 与 1）");
        assert_eq!(out[119].at, 239, "右端对得上现在");
    }

    #[test]
    fn downsample_is_identity_when_short() {
        let pts = vec![NetTotalPoint {
            at: 1,
            down_bps: 2.0,
            up_bps: 3.0,
        }];
        assert_eq!(downsample(&pts, 120), pts);
        assert!(downsample(&pts, 0).is_empty());
    }

    /// 首拍 warmup（不给 0 B/s），第二拍 ok
    #[test]
    fn first_tick_is_warmup_then_ok() {
        let state = TotalNetState::new(None);
        let s1 = state.record(raw(&[("en0", 1_000, 100)]), 1.0, 1_000);
        assert_eq!(s1.status, NetTotalStatus::Warmup);
        assert_eq!(s1.down_bps, 0.0);

        let s2 = state.record(raw(&[("en0", 3_000, 300)]), 1.0, 1_001);
        assert_eq!(s2.status, NetTotalStatus::Ok);
        assert_eq!(s2.down_bps, 2_000.0);
        assert_eq!(s2.session_down_bytes, 2_000);
        assert_eq!(s2.peak_down_bps, 2_000.0);
        assert_eq!(s2.peak_at, 1_001);
    }

    /// 无库时 today = None（「不可用」），而不是 0
    #[test]
    fn today_is_none_without_store() {
        let state = TotalNetState::new(None);
        state.record(raw(&[("en0", 1_000, 100)]), 1.0, 1_000);
        let s = state.record(raw(&[("en0", 3_000, 300)]), 1.0, 1_001);
        assert!(s.today.is_none());
        assert!(state.recent(7).is_empty());
    }

    /// 有库时逐拍累加到今日，并在首次落库窗口立即写一次
    #[test]
    fn today_accumulates_and_flushes() {
        let store = Arc::new(Store::open_in_memory().unwrap());
        let state = TotalNetState::new(Some(store.clone()));
        state.record(raw(&[("en0", 1_000, 100)]), 1.0, 10_000);
        let s = state.record(raw(&[("en0", 3_000, 300)]), 1.0, 10_001);

        let today = s.today.clone().expect("有库就必须有今日");
        assert_eq!(today.down_bytes, 2_000);
        assert_eq!(today.up_bytes, 200);
        assert_eq!(s.today_persisted_at, 10_001, "首次 flush 立即发生");

        let persisted = store.net_day(&today.day).unwrap().expect("库里要有今天");
        assert_eq!(persisted.down_bytes, 2_000);

        // 7 日柱能读到今天，且与内存一致
        let recent = state.recent(7);
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].down_bytes, 2_000);
    }

    /// 跨天：旧日的量留在旧日，新日从 0 起
    #[test]
    fn day_roll_flushes_old_day_and_starts_new() {
        let store = Arc::new(Store::open_in_memory().unwrap());
        let state = TotalNetState::new(Some(store.clone()));
        let now = 1_762_343_000;
        let yesterday = traffic::local_day(now - 86_400);
        // 手工把状态摆成「昨天还有 5 KB 没落库」
        *state.today.lock().unwrap() = Some(DayTraffic::zero(yesterday.clone()));
        *state.pending.lock().unwrap() = (5_000, 500);

        state.record(raw(&[("en0", 1_000, 100)]), 1.0, now);

        let old = store
            .net_day(&yesterday)
            .unwrap()
            .expect("旧日必须落库（保留期内不会被清理）");
        assert_eq!((old.down_bytes, old.up_bytes), (5_000, 500));

        let today = state.today.lock().unwrap().clone().unwrap();
        assert_eq!(today.day, traffic::local_day(now), "已切到新日");
        assert_eq!(today.down_bytes, 0, "切天那一拍之前没有增量，新日从 0 起");
    }

    /// 落库失败要记账；连续 2 次后 note 必须出现（不假装成功）
    #[test]
    fn flush_failure_surfaces_after_two_failures() {
        let state = TotalNetState::new(None);
        *state.pending.lock().unwrap() = (1_000, 100);

        assert!(!state.apply_flush("2026-11-05", 1, 1_000, 100, Err("disk full".into())));
        assert_eq!(state.flush_failures.load(Ordering::Relaxed), 1);
        assert!(state.flush_err.lock().unwrap().is_some());

        assert!(!state.apply_flush("2026-11-05", 2, 1_000, 100, Err("disk full".into())));
        assert_eq!(state.flush_failures.load(Ordering::Relaxed), 2);
        // 失败不扣 pending（数据不能丢）
        assert_eq!(*state.pending.lock().unwrap(), (1_000, 100));

        let s = state.record(raw(&[("en0", 1_000, 100)]), 1.0, 10);
        assert!(
            s.note.unwrap_or_default().contains("未落库"),
            "连续失败必须让用户看到"
        );
    }

    /// 落库成功后 pending 只扣掉已提交的量，失败后重试不会双计
    #[test]
    fn successful_flush_clears_pending() {
        let store = Arc::new(Store::open_in_memory().unwrap());
        let state = TotalNetState::new(Some(store.clone()));
        *state.today.lock().unwrap() = Some(DayTraffic::zero("2026-11-05"));
        *state.pending.lock().unwrap() = (1_234, 56);

        assert!(state.flush("2026-11-05", 100));
        assert_eq!(*state.pending.lock().unwrap(), (0, 0));
        assert_eq!(*state.persisted_at.lock().unwrap(), 100);
        assert_eq!(state.flush_failures.load(Ordering::Relaxed), 0);

        let row = store.net_day("2026-11-05").unwrap().unwrap();
        assert_eq!((row.down_bytes, row.up_bytes), (1_234, 56));
    }
}
