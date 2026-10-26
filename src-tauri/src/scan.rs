//! 磁盘扫描调度（M1/T-306）：按需/定时扫描 + 进度事件 + 报告缓存。
//!
//! | 时机 | 行为 |
//! |---|---|
//! | 启动后 30 s | 后台全量扫一轮（避开启动竞争） |
//! | 每周一次 | 静默刷新菜单栏主数字 |
//! | 手动 | `disk_scan` 命令 |
//! | 清理完成后 | 重新扫一轮（v1 不做增量，单轮 17.6 s 可接受） |
//!
//! 单飞：与 M2 深探同一套闸门思路（`AtomicBool` + Drop 自动释放），
//! 手动刷新撞上定时器轮时后来者直接跳过，绝不并行扫两遍。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use prune_core::disk::{self, DiskReport, ScanProgress};
use tauri::{AppHandle, Emitter, Manager};

/// 启动后首扫延迟（留出 WebView / tray 初始化时间）
pub const FIRST_SCAN_DELAY_SECS: u64 = 30;
/// 静默刷新周期：7 天
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
#[derive(Default)]
pub struct DiskState {
    pub report: Mutex<Option<DiskReport>>,
    in_flight: AtomicBool,
    /// 最近一轮耗时（ms）
    pub last_scan_ms: Mutex<u64>,
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

pub fn home_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/".into()))
}

/// 后台调度：启动 30 s 后首扫，此后每周一次
pub async fn run_background(app: AppHandle) {
    tokio::time::sleep(Duration::from_secs(FIRST_SCAN_DELAY_SECS)).await;
    if let Err(e) = scan_now(&app).await {
        eprintln!("[m1-disk] 首轮扫描失败: {e}");
    }

    let mut ticker = tokio::time::interval(Duration::from_secs(WEEKLY_SECS));
    ticker.tick().await; // interval 首次立即返回，跳过
    loop {
        ticker.tick().await;
        if let Err(e) = scan_now(&app).await {
            eprintln!("[m1-disk] 周期扫描失败: {e}");
        }
    }
}

/// 跑一轮扫描；已有扫描在进行则直接忽略（幂等）。
pub async fn scan_now(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<Arc<DiskState>>().inner().clone();
    let Some(_guard) = ScanInFlight::acquire(&state.in_flight) else {
        return Ok(()); // 已有扫描在跑
    };

    let started = Instant::now();
    let handle = app.clone();
    let home = home_dir();

    let progress_handle = handle.clone();
    let report = tauri::async_runtime::spawn_blocking(move || {
        disk::scan(
            &home,
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

    eprintln!(
        "[m1-disk] 扫描完成 {} ms · 主数字 {:.2} GB · 死重 {:.2} GB",
        elapsed,
        report.totals.reclaimable as f64 / 1e9,
        report.totals.deadweight as f64 / 1e9
    );

    let _ = handle.emit(EVENT_DONE, &report);
    Ok(())
}
