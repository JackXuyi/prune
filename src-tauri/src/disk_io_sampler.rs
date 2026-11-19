//! 磁盘 I/O 采样（f26 G-04，docs/f26-hardware-modules-plan.md D2）。
//!
//! **方案升级（2026-11-19 实施）**：sysinfo 0.39 自带 `Disk::usage()`，
//! macOS 侧按 bsd 物理盘计数（IOKit 设备级 IO 统计），给出**真正的读/写拆分**
//! ——比原方案的 iostat（读+写合计）口径更好，且零子进程、跨平台。
//!
//! 纪律不变：
//! - **可见性门控**。`active == false` 时完全不 refresh，常驻开销为 0；
//! - 2 s 一轮差分：`usage()` 返回距上次 refresh 的读/写字节增量，除以
//!   采样间隔即速率；
//! - APFS 多卷共享同一物理盘（bsd 相同），按盘节点去重取首见卷的计数器，
//!   防止多卷双计；
//! - 首轮为 warmup（计数器基线未建），快照保持 None，前端显示「采样中」。

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use sysinfo::{DiskRefreshKind, Disks};
use tauri::{Manager, State};

use crate::settings;

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DiskIoEntry {
    /// 物理盘节点名（disk0）
    pub name: String,
    /// 读速率（KB/s）
    pub read_kb_per_sec: f64,
    /// 写速率（KB/s）
    pub write_kb_per_sec: f64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DiskIoSnapshot {
    pub disks: Vec<DiskIoEntry>,
    /// 采集时间（Unix 秒）
    pub collected_at: u64,
}

pub struct DiskIoState {
    snapshot: Mutex<Option<DiskIoSnapshot>>,
    active: AtomicBool,
}

impl Default for DiskIoState {
    fn default() -> Self {
        Self::new()
    }
}

impl DiskIoState {
    pub fn new() -> Self {
        Self {
            snapshot: Mutex::new(None),
            active: AtomicBool::new(false),
        }
    }

    pub fn snapshot(&self) -> Option<DiskIoSnapshot> {
        self.snapshot
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn set_active(&self, active: bool) {
        self.active.store(active, Ordering::Relaxed);
    }

    fn is_active(&self) -> bool {
        self.active.load(Ordering::Relaxed)
    }
}

/// 一轮差分。Disks 实例跨轮持有（计数器基线靠它延续），差分逻辑在 loop 里。
struct DiskIoRound {
    disks: Disks,
    /// 首轮 warmup：计数器基线为 0，差分无意义
    primed: bool,
    /// 上一轮的挂载点集合（设备插拔时重建基线）
    mounted: std::collections::HashSet<String>,
}

impl DiskIoRound {
    fn new() -> Self {
        let kind = DiskRefreshKind::nothing().with_io_usage();
        Self {
            disks: Disks::new_with_refreshed_list_specifics(kind),
            primed: false,
            mounted: current_mounts(&Disks::new_with_refreshed_list()),
        }
    }

    fn round(&mut self) -> Option<Vec<DiskIoEntry>> {
        let kind = DiskRefreshKind::nothing().with_io_usage();
        self.disks.refresh_specifics(false, kind);

        let mounts = current_mounts(&self.disks);
        if mounts != self.mounted {
            // 挂载集变化（插拔盘/重新挂载）：旧计数器基线失效，本轮只重建基线
            self.mounted = mounts;
            self.primed = false;
            return None;
        }

        // 按盘节点去重：APFS 多卷共享一个 bsd 设备，计数器相同，取首见卷
        let mut by_dev: HashMap<String, (u64, u64)> = HashMap::new();
        for d in self.disks.list() {
            let mp = d.mount_point().to_string_lossy().to_string();
            if let Some(rest) = mp.strip_prefix("/dev/disk") {
                if let Some(n) = rest.split(['s', 'p']).next() {
                    if !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()) {
                        let key = format!("disk{n}");
                        by_dev.entry(key).or_insert_with(|| {
                            let u = d.usage();
                            (u.read_bytes, u.written_bytes)
                        });
                    }
                }
            }
        }

        if !self.primed {
            self.primed = true;
            return None; // 本轮只建基线
        }

        const INTERVAL_SECS: f64 = 2.0;
        let mut entries: Vec<DiskIoEntry> = by_dev
            .into_iter()
            .map(|(name, (read, written))| DiskIoEntry {
                name,
                read_kb_per_sec: read as f64 / 1024.0 / INTERVAL_SECS,
                write_kb_per_sec: written as f64 / 1024.0 / INTERVAL_SECS,
            })
            .collect();
        entries.sort_by_key(|e| natural_key(&e.name));
        Some(entries)
    }
}

/// 提取 diskN 中的数字做自然排序（disk2 < disk10）
fn natural_key(name: &str) -> u32 {
    name.trim_start_matches("disk").parse().unwrap_or(u32::MAX)
}

fn current_mounts(disks: &Disks) -> std::collections::HashSet<String> {
    disks
        .list()
        .iter()
        .map(|d| d.mount_point().to_string_lossy().to_string())
        .collect()
}

/// 门控轮：只有存储 tab 可见时才 refresh（无子进程，开销仅系统调用）
pub async fn run_disk_io(handle: tauri::AppHandle) {
    let mut round_state = Some(DiskIoRound::new());
    let mut tick = tokio::time::interval(Duration::from_secs(2));
    loop {
        tick.tick().await;
        let state = handle.state::<Arc<DiskIoState>>();
        if !state.is_active() {
            continue;
        }
        if !settings::get_bool(&handle, "hw.diskIo.enabled").unwrap_or(true) {
            *state.snapshot.lock().unwrap_or_else(|e| e.into_inner()) = None;
            continue;
        }
        // refresh_specifics 是同步系统调用（毫秒级），放阻塞线程避免饿死 runtime；
        // DiskIoRound 跨轮持有（take 传出、结果里带回归还）
        let Some(mut rs) = round_state.take() else {
            continue;
        };
        let result = tokio::task::spawn_blocking(move || {
            let out = rs.round();
            (rs, out)
        })
        .await;
        let (rs_back, out) = result.unwrap_or_else(|_| (DiskIoRound::new(), None));
        round_state = Some(rs_back);
        if let Some(disks) = out {
            let snap = DiskIoSnapshot {
                disks,
                collected_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0),
            };
            *state.snapshot.lock().unwrap_or_else(|e| e.into_inner()) = Some(snap);
        }
    }
}

#[tauri::command]
pub fn disk_io_snapshot(state: State<'_, Arc<DiskIoState>>) -> Option<DiskIoSnapshot> {
    state.snapshot()
}

#[tauri::command]
pub fn disk_io_set_active(state: State<'_, Arc<DiskIoState>>, active: bool) {
    state.set_active(active);
}
