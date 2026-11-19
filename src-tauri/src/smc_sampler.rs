//! CPU 温度采样（f26 G-07）：30 s 门控轮 + 快照命令。
//!
//! 门控纪律与 ps_sampler 相同：概览 tab 不可见时完全不碰 SMC；
//! `hw.cpuTemp.enabled` 关闭时快照置 None（前端整卡隐藏）。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use tauri::{Manager, State};

use crate::settings;

pub const TEMP_INTERVAL_SECS: u64 = 30;

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CpuTempSnapshot {
    /// 摄氏度（候选键读数的中位数）
    pub celsius: f64,
    /// 采集时间（Unix 秒）
    pub collected_at: u64,
}

pub struct CpuTempState {
    snapshot: Mutex<Option<CpuTempSnapshot>>,
    active: AtomicBool,
    last_at: Mutex<Option<std::time::Instant>>,
}

impl Default for CpuTempState {
    fn default() -> Self {
        Self::new()
    }
}

impl CpuTempState {
    pub fn new() -> Self {
        Self {
            snapshot: Mutex::new(None),
            active: AtomicBool::new(false),
            last_at: Mutex::new(None),
        }
    }

    pub fn snapshot(&self) -> Option<CpuTempSnapshot> {
        self.snapshot
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn set_active(&self, active: bool) {
        self.active.store(active, Ordering::Relaxed);
        if active {
            *self.last_at.lock().unwrap_or_else(|e| e.into_inner()) = None;
        }
    }

    fn is_active(&self) -> bool {
        self.active.load(Ordering::Relaxed)
    }
}

#[tauri::command]
pub fn cpu_temp_snapshot(state: State<'_, Arc<CpuTempState>>) -> Option<CpuTempSnapshot> {
    state.snapshot()
}

#[tauri::command]
pub fn cpu_temp_set_active(state: State<'_, Arc<CpuTempState>>, active: bool) {
    state.set_active(active);
}

/// 30 s 低频轮。SMC 打开失败 → 快照保持 None（失败即隐藏，不报错刷屏）。
pub async fn run_cpu_temp(handle: tauri::AppHandle) {
    let mut tick = tokio::time::interval(Duration::from_secs(5));
    loop {
        tick.tick().await;
        let state = handle.state::<Arc<CpuTempState>>();
        if !state.is_active() {
            continue;
        }
        let due = {
            let last = state.last_at.lock().unwrap_or_else(|e| e.into_inner());
            last.is_none()
                || last
                    .map(|t| t.elapsed() >= Duration::from_secs(TEMP_INTERVAL_SECS))
                    .unwrap_or(false)
        };
        if !due {
            continue;
        }
        *state.last_at.lock().unwrap_or_else(|e| e.into_inner()) = Some(std::time::Instant::now());
        if !settings::get_bool(&handle, "hw.cpuTemp.enabled").unwrap_or(true) {
            *state.snapshot.lock().unwrap_or_else(|e| e.into_inner()) = None;
            continue;
        }
        if let Some(celsius) = crate::smc::cpu_temp_median() {
            let snap = CpuTempSnapshot {
                celsius,
                collected_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0),
            };
            *state.snapshot.lock().unwrap_or_else(|e| e.into_inner()) = Some(snap);
        }
    }
}
