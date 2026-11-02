//! 磁盘扫描调度（M1/T-306 + A7 增量，见 docs/a7-incremental-plan.md §1）。
//!
//! | 时机 | 模式 | 说明 |
//! |---|---|---|
//! | 启动后 30 s | 全量 | 应用生命周期内的「首次全量」 |
//! | 每小时 | 增量 | 静默刷新菜单栏数字（mtime 指纹 diff，预期 < 1 s） |
//! | 每 168 个小时刻度 | 全量 | 每周校准增量盲区（深层新增文件，D-A7-1） |
//! | 手动 `disk_scan` | 全量 | 用户正在盯着数字求证，准确性优先（D-A7-2） |
//! | 清理完成后 | 增量 | 删除 = 首层条目 / 整根消失，增量 100% 捕获 |
//!
//! 保险：增量请求发现快照缺失或距上次全量超过 7 天时，自动升级为全量。
//!
//! 单飞：与 M2 深探同一套闸门思路（`AtomicBool` + Drop 自动释放），
//! 手动刷新撞上定时器轮时后来者直接跳过，绝不并行扫两遍。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use prune_core::disk::{self, DiskReport, ScanProgress, ScanSnapshot};
use tauri::{AppHandle, Emitter, Manager};

/// 启动后首扫延迟（留出 WebView / tray 初始化时间）
pub const FIRST_SCAN_DELAY_SECS: u64 = 30;
/// 增量刷新周期：1 小时
pub const INCREMENTAL_INTERVAL_SECS: u64 = 3600;
/// 每 N 个增量刻度做一次全量校准（168 × 1 h = 7 天）
pub const FULL_EVERY_TICKS: u64 = 7 * 24;
/// 距上次全量超过这个秒数，任何增量请求升级为全量
pub const WEEKLY_SECS: u64 = 7 * 24 * 3600;

pub const EVENT_PROGRESS: &str = "disk://progress";
pub const EVENT_DONE: &str = "disk://done";

/// 扫描单飞闸门
struct ScanInFlight<'a>(&'a AtomicBool);

impl<'a> ScanInFlight<'a> {
    fn acquire(flag: &'a AtomicBool) -> Option<Self> {
        flag.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| Self(flag))
    }
}

impl Drop for ScanInFlight<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// 磁盘扫描状态（命令与定时器共享）
pub struct DiskState {
    pub report: Mutex<Option<DiskReport>>,
    /// A7 增量快照（内存态：重启即失，下轮自动退化为全量）
    snapshot: Mutex<Option<ScanSnapshot>>,
    /// 最近一次全量扫描的 epoch 秒（升级保险的判定依据）
    last_full_at: Mutex<i64>,
    in_flight: AtomicBool,
    /// 最近一轮耗时（ms）
    pub last_scan_ms: Mutex<u64>,
}

impl Default for DiskState {
    fn default() -> Self {
        Self {
            report: Mutex::new(None),
            snapshot: Mutex::new(None),
            last_full_at: Mutex::new(0),
            in_flight: AtomicBool::new(false),
            last_scan_ms: Mutex::new(0),
        }
    }
}

impl DiskState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn report(&self) -> Option<DiskReport> {
        self.report
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

/// 用户主目录：统一走 `prune_core::paths`（Windows 上取 USERPROFILE）
pub fn home_dir() -> std::path::PathBuf {
    prune_core::paths::home_dir_or_root()
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 后台调度：启动 30 s 全量首扫，此后每小时增量、每 168 刻度（7 天）全量校准
pub async fn run_background(app: AppHandle) {
    tokio::time::sleep(Duration::from_secs(FIRST_SCAN_DELAY_SECS)).await;
    if let Err(e) = scan_now(&app, true).await {
        eprintln!("[m1-disk] 首轮扫描失败: {e}");
    }

    let mut ticker = tokio::time::interval(Duration::from_secs(INCREMENTAL_INTERVAL_SECS));
    ticker.tick().await; // interval 首次立即返回，跳过
    let mut ticks: u64 = 0;
    loop {
        ticker.tick().await;
        ticks += 1;
        let full = ticks % FULL_EVERY_TICKS == 0;
        if let Err(e) = scan_now(&app, full).await {
            eprintln!("[m1-disk] 周期扫描失败: {e}");
        }
    }
}

/// 跑一轮扫描；已有扫描在进行则直接忽略（幂等）。
///
/// `full = true` 强制全量；`full = false` 走增量，但快照缺失或距上次全量
/// 超过 [`WEEKLY_SECS`] 时自动升级为全量。
pub async fn scan_now(app: &AppHandle, full: bool) -> Result<(), String> {
    let state = app.state::<Arc<DiskState>>().inner().clone();
    let Some(_guard) = ScanInFlight::acquire(&state.in_flight) else {
        return Ok(()); // 已有扫描在跑
    };

    // 决定本轮模式：增量 → 全量的升级保险
    let (is_full, prev) = {
        let snap = state
            .snapshot
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let last_full = *state.last_full_at.lock().unwrap_or_else(|e| e.into_inner());
        let stale = snap.is_none() || now_secs() - last_full >= WEEKLY_SECS as i64;
        let force_full = full || stale;
        (force_full, if force_full { None } else { snap })
    };

    let started = Instant::now();
    let handle = app.clone();
    let home = home_dir();

    let progress_handle = handle.clone();
    let (report, snapshot) = tauri::async_runtime::spawn_blocking(move || {
        disk::scan_with(
            &home,
            prev.as_ref(),
            Some(&|p: ScanProgress| {
                let _ = progress_handle.emit(EVENT_PROGRESS, &p);
            }),
        )
    })
    .await
    .map_err(|e| format!("扫描线程失败: {e}"))?;

    let elapsed = started.elapsed().as_millis() as u64;
    *state.last_scan_ms.lock().unwrap_or_else(|e| e.into_inner()) = elapsed;
    *state.report.lock().unwrap_or_else(|e| e.into_inner()) = Some(report.clone());
    *state.snapshot.lock().unwrap_or_else(|e| e.into_inner()) = Some(snapshot);
    if is_full {
        *state.last_full_at.lock().unwrap_or_else(|e| e.into_inner()) = now_secs();
    }

    eprintln!(
        "[m1-disk] {}完成 {} ms · 主数字 {:.2} GB · 死重 {:.2} GB",
        if is_full { "全量扫描" } else { "增量扫描" },
        elapsed,
        report.totals.reclaimable as f64 / 1e9,
        report.totals.deadweight as f64 / 1e9
    );

    let _ = handle.emit(EVENT_DONE, &report);
    Ok(())
}
