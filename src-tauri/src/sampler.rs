//! 内存采样调度（T-206）：双频定时器 + 事件推送。
//!
//! | 频率 | 内容 | 数据源 |
//! |---|---|---|
//! | 10 s | 全量进程 RSS + vm_stat + swap + 压力 | sysinfo / vm_stat / sysctl |
//! | 5 min | Top N footprint 深探（并发 8 路） | `/usr/bin/footprint -p --swapped -j` |
//!
//! 关键约束：
//! - 深探是**付费**动作：按家族整组入选（Top 12 家族 / 64 进程 / 单进程 ≥ 8 MB），
//!   保证「应用总量」不是只算了主进程；
//! - 深探**单飞**：定时器轮与手动刷新共用一把 in-flight 闸门，绝不并行 spawn 两轮；
//! - 深探结果按 `pid + start_time` 缓存，快照轮次合并时校验 start_time，
//!   避免 pid 复用导致序列串台；
//! - 缓存更新是**整体替换**，清扫/重建不在锁内分步进行，读侧永远看到完整表；
//! - 任何 footprint 失败都只降级该进程到 RSS 口径，绝不伪造数值。

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use prune_core::db::{HourlyDelta, HourlyRow, Store};
use prune_core::leak::{self, CaseBook as LeakCaseBook, LeakReport};
use prune_core::mem::process::Scanner;
use prune_core::mem::system::{self, SystemMem};
use prune_core::mem::workspace;
use prune_core::mem::{
    self, CpuInfo, CpuLoad, FamilyMem, MemSnapshot, MemSource, ProcMem, WorkspaceRef,
};
use prune_core::swap::{self, SwapInfo};
use tauri::{AppHandle, Emitter, Manager};

/// 10 s：全量 RSS 快照
pub const FAST_INTERVAL_SECS: u64 = 10;
/// 整机 CPU 环形缓冲上限：60 点 × 10 s = 10 min（M5 §5.2，不入库）
pub const CPU_HISTORY_CAP: usize = 60;
/// 5 min：footprint 深探
pub const DEEP_INTERVAL_SECS: u64 = 300;
/// 深探候选：单个子进程 RSS 门槛（低于此值的 helper 不单独探测，按 RSS 计入）
pub const DEEP_MIN_PROC_RSS: u64 = 8 * 1024 * 1024;
/// 单轮深探进程上限（按家族整组入选，保证应用总量口径完整）
pub const DEEP_MAX_PROCS: usize = 64;
/// 单轮深探家族上限
pub const DEEP_MAX_FAMILIES: usize = 12;
/// 无候选时的兜底条数
pub const DEEP_FALLBACK_PROCS: usize = 8;
/// 深探并发上限（实测 12 路 0.27 s；8 路在 22 进程时偶超 1.5 s 预算，故提到 12）
pub const DEEP_CONCURRENCY: usize = 12;
/// 序列保留 7 天（D-M4）
pub const RETENTION_SECS: i64 = 7 * 24 * 3600;

pub const EVENT_SNAPSHOT: &str = "mem://snapshot";
pub const EVENT_DEEP: &str = "mem://deep";

/// 深探缓存行（按 pid 存，合并时校验 start_time）
#[derive(Debug, Clone, Copy)]
struct DeepRow {
    start_time: u64,
    footprint: u64,
    /// None = 该平台没有换出口径（Windows），UI 隐藏换出列
    swapped: Option<u64>,
}

/// 工作区归属缓存行（同样按 start_time 校验，防 pid 复用后把新进程算到旧项目上）
#[derive(Debug, Clone)]
struct WsRow {
    start_time: u64,
    ws: WorkspaceRef,
}

/// 深探单飞闸门：同一时刻只允许一轮 footprint 深探。
///
/// 定时器轮与手动刷新共用。否则用户点「刷新」时若正撞上 5 min 轮，
/// 会有两轮 footprint 并行（并发翻倍、缓存互相覆盖），既浪费也是数据抖动源。
/// Drop 时自动释放，异常路径（超时/panic）也不会把闸门永久卡住。
pub(crate) struct DeepInFlight<'a>(&'a AtomicBool);

impl<'a> DeepInFlight<'a> {
    /// 抢到闸门返回守卫；已有一轮在跑则返回 None（调用方直接跳过本轮）
    pub(crate) fn acquire(flag: &'a AtomicBool) -> Option<Self> {
        flag.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| Self(flag))
    }
}

impl Drop for DeepInFlight<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// 跨轮次保存的状态
#[derive(Default)]
struct Prev {
    /// (swapins, swapouts, ts)
    swap_counters: Option<(u64, u64, i64)>,
    swap_file_count: Option<usize>,
}

// ---------------------------------------------------------------- D3 应用小时历史

/// 小时历史落库节流：攒 60 s 写一次，避免每 10 s 一次事务
const APP_HISTORY_FLUSH: Duration = Duration::from_secs(60);
/// 历史保留 8 天（多留 1 天保证「7 整天」完整）
const APP_HISTORY_RETAIN_SECS: i64 = 8 * 24 * 3600;

/// 单家族的当前小时累计（增量，写完即清）
#[derive(Default)]
struct AppAcc {
    rss_sum: u64,
    rss_n: u32,
    rss_peak: u64,
    cpu_sum: f32,
    cpu_n: u32,
    cpu_peak: f32,
    net_down: u64,
    net_up: u64,
}

/// 应用小时历史聚合器（D3）。
///
/// 只有**一个**写入者语义：fast 轮累加 rss/cpu，net 采样累加流量，两者都经这里
/// 合并后再按 60 s / 跨小时 flush 到 `app_hourly`。增量相加，重启不清零。
struct AppHistoryAgg {
    bucket: String,
    acc: HashMap<String, AppAcc>,
    last_flush: Instant,
    last_prune_day: String,
}

impl AppHistoryAgg {
    fn new(now: i64) -> Self {
        Self {
            bucket: prune_core::localtime::hour_string(now),
            acc: HashMap::new(),
            last_flush: Instant::now(),
            last_prune_day: prune_core::localtime::day_string(now),
        }
    }
}

/// 把累计量写进 `app_hourly`（调用方持锁）。
///
/// `force = true` 用于跨小时：先把上一小时结清，再切桶。
fn flush_app_history(store: &Option<Arc<Store>>, agg: &mut AppHistoryAgg, now: i64, force: bool) {
    if !force && agg.last_flush.elapsed() < APP_HISTORY_FLUSH {
        return;
    }
    agg.last_flush = Instant::now();

    let Some(store) = store else { return };

    let bucket = agg.bucket.clone();
    let mut rows: Vec<HourlyDelta> = Vec::with_capacity(agg.acc.len());
    for (family, a) in agg.acc.drain() {
        if a.rss_n == 0 && a.cpu_n == 0 && a.net_down == 0 && a.net_up == 0 {
            continue;
        }
        rows.push(HourlyDelta {
            family,
            bucket: bucket.clone(),
            rss_sum: a.rss_sum,
            rss_n: a.rss_n,
            rss_peak: a.rss_peak,
            cpu_sum: a.cpu_sum,
            cpu_n: a.cpu_n,
            cpu_peak: a.cpu_peak,
            net_down: a.net_down,
            net_up: a.net_up,
        });
    }

    if let Err(e) = store.app_hourly_add(&rows, now) {
        eprintln!("[d3-history] 小时历史写入失败（{} 行）: {e}", rows.len());
    }

    // 每天清一次 8 天前的桶（跨天判断，避免每小时都 DELETE）
    let today = prune_core::localtime::day_string(now);
    if agg.last_prune_day != today {
        agg.last_prune_day = today;
        let cutoff = prune_core::localtime::hour_string(now - APP_HISTORY_RETAIN_SECS);
        match store.app_hourly_prune_before(&cutoff) {
            Ok(n) if n > 0 => eprintln!("[d3-history] 清理过期小时桶 {n} 行"),
            Ok(_) => {}
            Err(e) => eprintln!("[d3-history] 清理失败: {e}"),
        }
    }
}

/// 采样器与命令共享的状态
pub struct MemState {
    pub snapshot: Mutex<Option<MemSnapshot>>,
    pub processes: Mutex<Vec<ProcMem>>,
    scanner: Mutex<Scanner>,
    store: Option<Arc<Store>>,
    deep: Mutex<HashMap<i32, DeepRow>>,
    /// 工作区归属缓存（M2.5）：只在深探轮刷新，10 s 快照轮复用它，
    /// 避免每 10 s 跑一遍 lsof
    workspaces: Mutex<HashMap<i32, WsRow>>,
    /// 深探单飞闸门（定时器轮与手动刷新共用）
    deep_in_flight: AtomicBool,
    prev: Mutex<Prev>,
    deep_available: Mutex<bool>,
    deep_updated_at: Mutex<Option<i64>>,
    /// B5 泄漏嫌疑判定结果（M3）。只在深探落库后重算
    leaks: Mutex<LeakReport>,
    /// M22 悬案卷宗：把逐轮嫌疑串成有状态的案件（离场结案 / 复发 / 归档）
    cases: Mutex<LeakCaseBook>,
    /// 最近两轮耗时（ms），供性能预算校验
    pub last_fast_ms: Mutex<u64>,
    pub last_deep_ms: Mutex<u64>,
    /// 整机 CPU 环形缓冲（60 点 = 10 min）；只在 fast 轮推进，deep 轮复用最近读数
    cpu_hist: Mutex<VecDeque<f32>>,
    /// 最近一次整机 CPU 读数。deep 轮重建快照时复用（进程 CPU 本就来自上一 fast 轮，口径一致）
    cpu_last: Mutex<Option<CpuInfo>>,
    /// D3：应用小时历史聚合（fast 轮累加 rss/cpu，net 采样累加流量）
    app_history: Mutex<AppHistoryAgg>,
}

impl MemState {
    pub fn new(store: Option<Arc<Store>>) -> Self {
        // 无库时判定永远不可能有结果——必须是「无法判定」的显式状态，
        // 不能用空 items 冒充「没有泄漏」（方案 §5 / §7.3）。
        let leaks = if store.is_some() {
            LeakReport::default()
        } else {
            LeakReport::unavailable("序列库不可用，无法做泄漏判定")
        };
        Self {
            snapshot: Mutex::new(None),
            processes: Mutex::new(Vec::new()),
            scanner: Mutex::new(Scanner::new()),
            store,
            deep: Mutex::new(HashMap::new()),
            workspaces: Mutex::new(HashMap::new()),
            deep_in_flight: AtomicBool::new(false),
            prev: Mutex::new(Prev::default()),
            deep_available: Mutex::new(true),
            deep_updated_at: Mutex::new(None),
            leaks: Mutex::new(leaks),
            cases: Mutex::new(LeakCaseBook::new()),
            last_fast_ms: Mutex::new(0),
            last_deep_ms: Mutex::new(0),
            cpu_hist: Mutex::new(VecDeque::with_capacity(CPU_HISTORY_CAP)),
            cpu_last: Mutex::new(None),
            app_history: Mutex::new(AppHistoryAgg::new(now_secs())),
        }
    }

    fn lock_deep(&self) -> std::sync::MutexGuard<'_, HashMap<i32, DeepRow>> {
        self.deep.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn lock_ws(&self) -> std::sync::MutexGuard<'_, HashMap<i32, WsRow>> {
        self.workspaces.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 悬案卷宗快照（M22）：侦查中的在前，已归档在后
    pub fn cases(&self) -> Vec<prune_core::leak::LeakCase> {
        self.cases
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .cases()
            .to_vec()
    }

    /// 查询序列（无库时返回空表，UI 显示「暂无历史」而不是报错）
    pub fn history(&self, pid: i32, since: i64, limit: usize) -> Vec<prune_core::db::Point> {
        match &self.store {
            Some(store) => store.history(pid, since, limit).unwrap_or_default(),
            None => Vec::new(),
        }
    }

    /// D3：把本轮家族读数累加进当前小时桶（fast 轮 10 s 一次）。
    ///
    /// 内存用 **RSS** 而不是 footprint：历史要的是连续序列，而深探只覆盖 top-12 家族；
    /// 当前读数仍用 footprint，两套口径在 UI 脚注里分开写。
    pub fn accumulate_app(&self, families: &[FamilyMem], now: i64) {
        let bucket = prune_core::localtime::hour_string(now);
        let mut agg = self.app_history.lock().unwrap_or_else(|e| e.into_inner());

        if agg.bucket != bucket {
            // 跨小时：先把上一小时结清再切桶
            flush_app_history(&self.store, &mut agg, now, true);
            agg.acc.clear();
            agg.bucket = bucket;
        }

        for f in families {
            let acc = agg.acc.entry(f.name.clone()).or_default();
            acc.rss_sum = acc.rss_sum.saturating_add(f.rss);
            acc.rss_n = acc.rss_n.saturating_add(1);
            acc.rss_peak = acc.rss_peak.max(f.rss);
            if let Some(cpu) = f.cpu {
                if f.cpu_covered > 0 && cpu.is_finite() && cpu >= 0.0 {
                    acc.cpu_sum += cpu;
                    acc.cpu_n = acc.cpu_n.saturating_add(1);
                    acc.cpu_peak = acc.cpu_peak.max(cpu);
                }
            }
        }

        flush_app_history(&self.store, &mut agg, now, false);
    }

    /// D3：累加某家族的流量增量（后台低频网速轮调用，约 60 s 一次）。
    pub fn add_app_net(&self, family: &str, down: u64, up: u64, now: i64) {
        if down == 0 && up == 0 {
            return;
        }
        let bucket = prune_core::localtime::hour_string(now);
        let mut agg = self.app_history.lock().unwrap_or_else(|e| e.into_inner());

        if agg.bucket != bucket {
            flush_app_history(&self.store, &mut agg, now, true);
            agg.acc.clear();
            agg.bucket = bucket;
        }

        let acc = agg.acc.entry(family.to_string()).or_default();
        acc.net_down = acc.net_down.saturating_add(down);
        acc.net_up = acc.net_up.saturating_add(up);

        flush_app_history(&self.store, &mut agg, now, false);
    }

    /// D3：读某家族自 `from_bucket` 起的小时历史（只含有记录的小时）
    pub fn app_history(&self, family: &str, from_bucket: &str) -> Result<Vec<HourlyRow>, String> {
        match &self.store {
            Some(store) => store
                .app_hourly_get(family, from_bucket)
                .map_err(|e| e.to_string()),
            None => Ok(Vec::new()),
        }
    }
}

/// 启动双频采样（在 setup 里 spawn 一次）
pub async fn run(app: AppHandle) {
    // 首轮：先出 10 s 快照让 UI 有数，再后台补深探（方案 §5.5）
    let _ = fast_round(&app).await;
    let _ = deep_round(&app).await;

    let mut fast = tokio::time::interval(Duration::from_secs(FAST_INTERVAL_SECS));
    let mut deep = tokio::time::interval(Duration::from_secs(DEEP_INTERVAL_SECS));
    fast.tick().await; // 消费掉 interval 的立即触发
    deep.tick().await;

    loop {
        tokio::select! {
            _ = fast.tick() => {
                if let Err(e) = fast_round(&app).await {
                    eprintln!("[m2-mem] 快照轮失败: {e}");
                }
            }
            _ = deep.tick() => {
                if let Err(e) = deep_round(&app).await {
                    eprintln!("[m2-mem] 深探轮失败: {e}");
                }
            }
        }
    }
}

fn state(app: &AppHandle) -> Arc<MemState> {
    app.state::<Arc<MemState>>().inner().clone()
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 10 s 轮：枚举进程 + 系统读数 → 合并上一轮深探结果 → 广播快照
pub async fn fast_round(app: &AppHandle) -> Result<(), String> {
    let state = state(app);
    let app = app.clone();

    let outcome = tauri::async_runtime::spawn_blocking(move || {
        let started = Instant::now();

        let scan = {
            let mut scanner = state.scanner.lock().unwrap_or_else(|e| e.into_inner());
            scanner.scan()
        };
        let merged = merge_workspace(merge_deep(scan.procs, &state.lock_deep()), &state.lock_ws());
        let cpu = build_cpu_info(&state, scan.cpu_usage, scan.cores, scan.load);
        let snapshot = assemble(&state, &merged, cpu.clone());

        let elapsed = started.elapsed().as_millis() as u64;
        *state.last_fast_ms.lock().unwrap_or_else(|e| e.into_inner()) = elapsed;

        // D3：累加应用小时历史（60 s 节流写库，不计入本轮耗时）
        state.accumulate_app(&snapshot.families, now_secs());

        *state.processes.lock().unwrap_or_else(|e| e.into_inner()) = merged;
        *state.snapshot.lock().unwrap_or_else(|e| e.into_inner()) = Some(snapshot.clone());

        eprintln!(
            "[m2-mem] 快照轮 {} ms · {} 进程 · swap {}/{} · 压力 {} · CPU {}",
            elapsed,
            snapshot
                .families
                .iter()
                .map(|f| f.process_count)
                .sum::<usize>(),
            snapshot.swap.used / 1024 / 1024,
            snapshot.swap.total / 1024 / 1024,
            snapshot
                .pressure
                .map(|p| format!("{:?}", p.level))
                .unwrap_or_else(|| "不可用".to_string()),
            snapshot
                .cpu
                .as_ref()
                .map(|c| format!("{:.1}%", c.usage))
                .unwrap_or_else(|| "预热中".to_string())
        );
        snapshot
    })
    .await
    .map_err(|e| format!("spawn_blocking 失败: {e}"))?;

    let _ = app.emit(EVENT_SNAPSHOT, &outcome);
    // 托盘 tooltip 携带精确 CPU 读数（M6-T2 顺手项；动画关闭时数字仍在）
    crate::tray::update_tooltip(&app, outcome.cpu.as_ref().map(|c| c.usage));
    // 菜单栏数字的内存段（M13）：0 = 口径未知（serde default），显示为空而不是 0G
    crate::tray::update_title_mem(&app, Some(outcome.physical_used_bytes).filter(|b| *b > 0));
    Ok(())
}

/// 5 min 轮：Top N footprint 深探 → 更新缓存 → 重建快照 → 落库
pub async fn deep_round(app: &AppHandle) -> Result<(), String> {
    let state = state(app);
    let app = app.clone();

    // 单飞闸门：手动刷新撞上定时器轮时，后到的一轮直接跳过而不是并行 spawn
    let Some(_gate) = DeepInFlight::acquire(&state.deep_in_flight) else {
        eprintln!("[m2-mem] 深探轮跳过：上一轮仍在进行");
        return Ok(());
    };

    let targets = {
        let procs = state.processes.lock().unwrap_or_else(|e| e.into_inner());
        select_deep_targets(&procs)
    };
    if targets.is_empty() {
        eprintln!("[m2-mem] 深探轮：无候选进程（RSS 均低于门槛）");
        return Ok(());
    }

    let started = Instant::now();
    let probe_state = state.clone();
    let target_count = targets.len();
    let (samples, elapsed) = tauri::async_runtime::spawn_blocking(move || {
        let samples = mem::footprint::probe_many(&targets, DEEP_CONCURRENCY);
        let elapsed = started.elapsed().as_millis() as u64;
        (samples, elapsed)
    })
    .await
    .map_err(|e| format!("深探线程失败: {e}"))?;

    *state.last_deep_ms.lock().unwrap_or_else(|e| e.into_inner()) = elapsed;

    let success = samples.len();
    // 先在锁外构建完整的新表，再整体替换：清扫与重建不在锁内分步进行，
    // 读侧（10 s 快照轮）不会撞上「表已清空、尚未重建」的中间态而瞬间全退 RSS 口径。
    let next = {
        let procs = probe_state
            .processes
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        build_deep_table(&samples, &procs)
    };
    let updated = next.len();
    *probe_state.lock_deep() = next;

    // M2.5：工作区归属。lsof 有真实 I/O 成本，只跟 5 min 深探轮走，
    // 归属结果落缓存供后续 10 s 快照轮复用（进程退出/复用由 start_time 校验处理）。
    let ws_started = Instant::now();
    let ws_state = state.clone();
    let ws_table = tauri::async_runtime::spawn_blocking(move || {
        let procs = ws_state
            .processes
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        build_ws_table(&procs, &home_dir())
    })
    .await
    .map_err(|e| format!("工作区扫描线程失败: {e}"))?;
    let ws_count = ws_table.len();
    let ws_elapsed = ws_started.elapsed().as_millis() as u64;
    *probe_state.lock_ws() = ws_table;

    // 全部失败 → footprint 不可用（命令缺失 / 权限被拦），整表退回 RSS 口径
    let available = success > 0;
    *probe_state
        .deep_available
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = available;
    let ts = now_secs();
    if available {
        *probe_state
            .deep_updated_at
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(ts);
    }

    // 重新装配快照（深探字段已更新）
    let snapshot = {
        let procs = probe_state
            .processes
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let merged = merge_workspace(
            merge_deep(procs, &probe_state.lock_deep()),
            &probe_state.lock_ws(),
        );

        // 落库 + 泄漏判定都要在装配快照之前：
        // B5 判定的输入是「刚刚落库的这一批点」，放到后面会让结果永远晚一轮（5 min）。
        if let Some(store) = &probe_state.store {
            match store.record(ts, &merged) {
                Ok(n) => {
                    let cutoff = ts - RETENTION_SECS;
                    let pruned = store.prune_before(cutoff).unwrap_or(0);
                    eprintln!("[m2-mem] 序列写入 {n} 行，清理过期 {pruned} 行");
                }
                Err(e) => eprintln!("[m2-mem] ⚠️ 序列写入失败: {e}"),
            }

            let report = recompute_leaks(store, &merged, ts);
            eprintln!(
                "[m3-leak] 判定 {} 个进程 · 命中 {} · 良性 {} · 数据不足 {} · 未达标 {}{}",
                report.observed,
                report.items.len(),
                report.benign,
                report.insufficient,
                report.below,
                report
                    .note
                    .as_ref()
                    .map(|n| format!(" · ⚠️ {n}"))
                    .unwrap_or_default()
            );
            *probe_state.leaks.lock().unwrap_or_else(|e| e.into_inner()) = report.clone();

            // M22 悬案卷宗：用本轮命中 + 全量进程表推进案件簿（离场 / 复发判定）
            {
                let alive: std::collections::HashSet<(i32, u64)> =
                    merged.iter().map(|p| (p.pid, p.start_time)).collect();
                let mut book = probe_state.cases.lock().unwrap_or_else(|e| e.into_inner());
                book.update(&report.items, &alive, ts);
            }
        }

        let snapshot = assemble(&probe_state, &merged, probe_state.last_cpu());
        *probe_state
            .processes
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = merged.clone();
        *probe_state
            .snapshot
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(snapshot.clone());
        snapshot
    };

    eprintln!(
        "[m2-mem] 深探轮 {elapsed} ms · 成功 {success}/{} · 覆盖 {updated} 进程 · 口径 {}",
        target_count,
        if available {
            "footprint"
        } else {
            "RSS（降级）"
        }
    );
    eprintln!("[m2-ws] 工作区归属 {ws_count} 进程 · {ws_elapsed} ms");

    let _ = app.emit(EVENT_DEEP, &snapshot);
    Ok(())
}

/// 深探候选：按**家族整组**入选，而不是挑单个最大的进程。
///
/// 原因：应用占用 = 主进程 + 一堆 helper，只探主进程会算出一个严重偏低的
/// 「应用内存」。这里先把家族按 RSS 总和排序，再整组纳入（组内过滤掉
/// RSS < 8 MB 的碎片进程），受进程数 / 家族数双上限约束。
fn select_deep_targets(procs: &[ProcMem]) -> Vec<i32> {
    use std::collections::HashMap;

    let mut by_family: HashMap<&str, (u64, Vec<&ProcMem>)> = HashMap::new();
    for p in procs {
        let entry = by_family
            .entry(p.family.as_str())
            .or_insert((0, Vec::new()));
        entry.0 = entry.0.saturating_add(p.rss);
        entry.1.push(p);
    }

    let mut families: Vec<(u64, Vec<&ProcMem>)> = by_family.into_values().collect();
    families.sort_by_key(|(rss, _)| std::cmp::Reverse(*rss));

    let mut out: Vec<i32> = Vec::new();
    let mut picked_families = 0usize;
    for (_, members) in families {
        if out.len() >= DEEP_MAX_PROCS || picked_families >= DEEP_MAX_FAMILIES {
            break;
        }
        let mut heavy: Vec<&ProcMem> = members
            .into_iter()
            .filter(|p| p.rss >= DEEP_MIN_PROC_RSS)
            .collect();
        if heavy.is_empty() {
            continue;
        }
        heavy.sort_by_key(|p| std::cmp::Reverse(p.rss));
        picked_families += 1;
        for p in heavy {
            if out.len() >= DEEP_MAX_PROCS {
                break;
            }
            out.push(p.pid);
        }
    }

    // 整机都很闲时（没有任何家族过门槛）也要有数据，取最大的几个进程
    if out.is_empty() {
        let mut top: Vec<&ProcMem> = procs.iter().collect();
        top.sort_by_key(|p| std::cmp::Reverse(p.rss));
        out = top
            .into_iter()
            .take(DEEP_FALLBACK_PROCS)
            .map(|p| p.pid)
            .collect();
    }

    out.into_iter().filter(|pid| *pid > 0).collect()
}

/// 由本轮深探结果构建完整缓存表：只保留此刻仍存在的进程（丢弃已退出的残影），
/// 并记下 start_time 供后续 pid 复用校验。
///
/// 纯函数，锁外调用；构建完成后整体替换缓存，读侧永远看到一张完整的表。
fn build_deep_table(
    samples: &HashMap<i32, mem::footprint::FootprintSample>,
    procs: &[ProcMem],
) -> HashMap<i32, DeepRow> {
    let mut next: HashMap<i32, DeepRow> = HashMap::with_capacity(samples.len());
    for (pid, sample) in samples {
        if let Some(p) = procs.iter().find(|p| p.pid == *pid) {
            next.insert(
                *pid,
                DeepRow {
                    start_time: p.start_time,
                    footprint: sample.footprint,
                    swapped: sample.swapped,
                },
            );
        }
    }
    next
}

/// 合并深探缓存：start_time 不一致视为 pid 复用，丢弃该行
fn merge_deep(mut procs: Vec<ProcMem>, deep: &HashMap<i32, DeepRow>) -> Vec<ProcMem> {
    for p in &mut procs {
        if let Some(row) = deep.get(&p.pid) {
            if row.start_time == p.start_time {
                p.footprint = Some(row.footprint);
                p.swapped = row.swapped;
                p.source = MemSource::deep();
            }
        }
    }
    procs
}

/// 由本轮归属结果构建缓存表（同样校验 start_time）
fn build_ws_table(procs: &[ProcMem], home: &Path) -> HashMap<i32, WsRow> {
    let resolved = workspace::resolve_many(procs, home);
    let mut next: HashMap<i32, WsRow> = HashMap::with_capacity(resolved.len());
    for (pid, ws) in resolved {
        if let Some(p) = procs.iter().find(|p| p.pid == pid) {
            next.insert(
                pid,
                WsRow {
                    start_time: p.start_time,
                    ws,
                },
            );
        }
    }
    next
}

/// 合并工作区归属：start_time 不一致（pid 被复用）时不继承旧归属
fn merge_workspace(mut procs: Vec<ProcMem>, ws: &HashMap<i32, WsRow>) -> Vec<ProcMem> {
    for p in &mut procs {
        p.workspace = match ws.get(&p.pid) {
            Some(row) if row.start_time == p.start_time => Some(row.ws.clone()),
            _ => None,
        };
    }
    procs
}

/// 用户主目录（归属解析的基准路径；取不到时退化为平台根，后续解析会全部判为噪声）
fn home_dir() -> PathBuf {
    prune_core::paths::home_dir_or_root()
}

/// 读系统侧数据并装配快照（两条轮次共用）。
///
/// `cpu`：fast 轮传入本轮新读数；deep 轮复用最近一次读数
/// （进程 CPU 本就来自上一 fast 轮，快照内部口径保持一致）。
fn assemble(state: &MemState, procs: &[ProcMem], cpu: Option<CpuInfo>) -> MemSnapshot {
    let now = now_secs();
    // 读不到就整体降级为「无口径」：宁可少显示，也不能拿 0 冒充真实读数
    let system = system::read().unwrap_or_else(|e| {
        eprintln!("[m2-mem] 系统内存读数失败（本轮回退为无口径）: {e}");
        SystemMem::unavailable()
    });
    let pressure = system::read_pressure();

    let swap_info: SwapInfo;
    let thrash;
    {
        let mut prev = state.prev.lock().unwrap_or_else(|e| e.into_inner());
        swap_info = swap::collect(prev.swap_file_count);
        prev.swap_file_count = swap_info.file_count;
        // 只有两个平台都给了计数器才有速率；Windows 恒为 None → UI 隐藏
        thrash = match (system.swapins_total, system.swapouts_total) {
            (Some(ins), Some(outs)) => {
                let t = mem::thrash_rate(prev.swap_counters, ins, outs, now);
                prev.swap_counters = Some((ins, outs, now));
                Some(t)
            }
            _ => None,
        };
    }

    let deep_available = *state
        .deep_available
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let deep_updated_at = *state
        .deep_updated_at
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let leaks = state
        .leaks
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();

    mem::build_snapshot(
        &system,
        swap_info,
        pressure,
        thrash,
        procs,
        now,
        deep_updated_at,
        deep_available,
        leaks,
        cpu,
    )
}

/// 组装整机 CPU 读数并推进环形缓冲（只在 fast 轮调用，deep 轮走 `last_cpu`）。
///
/// 首轮 `cpu_usage = None` → 返回 None，绝不拿 `new_all` 的初始化采样冒充窗口均值。
fn build_cpu_info(
    state: &MemState,
    usage: Option<f32>,
    cores: u32,
    load: Option<CpuLoad>,
) -> Option<CpuInfo> {
    let usage = usage?;
    let mut hist = state.cpu_hist.lock().unwrap_or_else(|e| e.into_inner());
    hist.push_back(usage);
    while hist.len() > CPU_HISTORY_CAP {
        hist.pop_front();
    }
    let info = CpuInfo {
        usage,
        cores,
        load,
        history: hist.iter().copied().collect(),
        at: now_secs(),
    };
    *state.cpu_last.lock().unwrap_or_else(|e| e.into_inner()) = Some(info.clone());
    Some(info)
}

impl MemState {
    /// 最近一次整机 CPU 读数（deep 轮重建快照时复用；不推进环形缓冲）
    fn last_cpu(&self) -> Option<CpuInfo> {
        self.cpu_last
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

/// B5：深探落库后重算泄漏嫌疑。
///
/// 查询范围 = 最大窗口（GC 60 min）+ 5 min 余量。回归在内存里做，
/// 相对 footprint 深探（百 ms 级）成本可忽略。
fn recompute_leaks(store: &Store, procs: &[ProcMem], now: i64) -> LeakReport {
    let since = now - leak::GC_WINDOW_SECS - 300;
    match store.series(since) {
        Ok(series) => leak::detect(&series, procs, now),
        Err(e) => LeakReport::unavailable(format!("序列查询失败：{e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proc(pid: i32, rss: u64, start_time: u64) -> ProcMem {
        fam_proc(pid, "Test", rss, start_time)
    }

    fn fam_proc(pid: i32, family: &str, rss: u64, start_time: u64) -> ProcMem {
        ProcMem {
            pid,
            ppid: 1,
            name: format!("p{pid}"),
            exe: None,
            family: family.into(),
            rss,
            footprint: None,
            swapped: None,
            source: MemSource::Rss,
            start_time,
            cmd: Vec::new(),
            workspace: None,
            cpu: None,
            cpu_time_ms: 0,
        }
    }

    fn mb(v: u64) -> u64 {
        v * 1024 * 1024
    }

    #[test]
    fn deep_targets_probe_whole_family() {
        // 大应用的 helper 也必须一起探，否则应用总量只有主进程那部分
        let procs = vec![
            fam_proc(1, "Cursor", mb(400), 0),
            fam_proc(2, "Cursor", mb(120), 0),
            fam_proc(3, "Cursor", mb(30), 0),
            fam_proc(4, "Slack", mb(90), 0),
        ];
        let targets = select_deep_targets(&procs);
        assert!(targets.contains(&1) && targets.contains(&2));
        // 碎片进程（< 8 MB）不单独探测
        let with_frag = {
            let mut v = procs.clone();
            v.push(fam_proc(5, "Cursor", mb(3), 0));
            v
        };
        let t2 = select_deep_targets(&with_frag);
        assert!(!t2.contains(&5));
    }

    #[test]
    fn deep_targets_respect_caps() {
        // 家族数上限
        let mut procs = Vec::new();
        for f in 0..30 {
            for i in 0..3 {
                procs.push(fam_proc(
                    f * 10 + i + 1,
                    &format!("F{f}"),
                    mb(100 - f as u64),
                    0,
                ));
            }
        }
        let targets = select_deep_targets(&procs);
        assert!(targets.len() <= DEEP_MAX_PROCS);
        assert!(targets.len() >= DEEP_MAX_FAMILIES);
        // 大 RSS 家族优先入选
        assert!(targets.contains(&1));
        assert!(!targets.contains(&(29 * 10 + 1)));
    }

    #[test]
    fn deep_targets_fall_back_when_all_small() {
        let procs: Vec<ProcMem> = (1..=20)
            .map(|i| fam_proc(i, &format!("F{i}"), mb(3), 0))
            .collect();
        let targets = select_deep_targets(&procs);
        assert_eq!(targets.len(), DEEP_FALLBACK_PROCS);
    }

    #[test]
    fn deep_guard_is_single_flight() {
        let flag = AtomicBool::new(false);
        let gate = DeepInFlight::acquire(&flag);
        assert!(gate.is_some(), "空闲时应能进入深探");
        assert!(
            DeepInFlight::acquire(&flag).is_none(),
            "上一轮未结束时不得并行开第二轮（刷新撞定时器轮的场景）"
        );
        drop(gate);
        assert!(
            DeepInFlight::acquire(&flag).is_some(),
            "上一轮结束后必须放行，否则深探会被永久卡死"
        );
    }

    #[test]
    fn deep_table_rebuilds_wholesale_and_drops_exited() {
        use prune_core::mem::footprint::FootprintSample;

        let mut samples = HashMap::new();
        samples.insert(
            1,
            FootprintSample {
                pid: 1,
                footprint: 1000,
                swapped: Some(900),
            },
        );
        samples.insert(
            2,
            FootprintSample {
                pid: 2,
                footprint: 2000,
                swapped: Some(1800),
            },
        );
        // pid 2 已退出（本轮快照里没有它）
        let procs = vec![proc(1, 100, 111)];
        let table = build_deep_table(&samples, &procs);

        assert_eq!(table.len(), 1);
        assert_eq!(table.get(&1).unwrap().start_time, 111);
        assert!(table.get(&2).is_none(), "已退出进程不能留下残影");
    }

    #[test]
    fn merge_workspace_respects_pid_reuse() {
        use prune_core::mem::{WorkspaceRef, WorkspaceSource};

        let procs = vec![proc(1, 100, 111), proc(2, 100, 222)];
        let mut table = HashMap::new();
        table.insert(
            1,
            WsRow {
                start_time: 111,
                ws: WorkspaceRef::new("/Users/xuyi/Desktop/git/AI", WorkspaceSource::Label),
            },
        );
        // pid 2 的 start_time 不匹配 → 新进程不能继承旧项目的归属
        table.insert(
            2,
            WsRow {
                start_time: 999,
                ws: WorkspaceRef::new("/Users/xuyi/Desktop/git/old", WorkspaceSource::Handle),
            },
        );

        let merged = merge_workspace(procs, &table);
        assert_eq!(
            merged[0].workspace.as_ref().map(|w| w.label.as_str()),
            Some("AI")
        );
        assert!(merged[1].workspace.is_none(), "pid 复用不得继承归属");
    }

    #[test]
    fn merge_skips_reused_pid() {
        let procs = vec![proc(1, 100, 111), proc(2, 100, 222)];
        let mut deep = HashMap::new();
        deep.insert(
            1,
            DeepRow {
                start_time: 111,
                footprint: 1000,
                swapped: Some(900),
            },
        );
        // pid 2 的 start_time 不匹配 → 视为复用，不应合并
        deep.insert(
            2,
            DeepRow {
                start_time: 999,
                footprint: 2000,
                swapped: Some(1900),
            },
        );
        let merged = merge_deep(procs, &deep);
        assert_eq!(merged[0].footprint, Some(1000));
        assert_eq!(merged[0].source, MemSource::deep());
        assert_eq!(merged[1].footprint, None);
        assert_eq!(merged[1].source, MemSource::Rss);
    }
}
