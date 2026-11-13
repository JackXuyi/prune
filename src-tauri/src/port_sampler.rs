//! 端口采样（M7）：与内存采样**完全解耦**的独立通道。
//!
//! 为什么不进 `fast_round`（docs/m7-port-plan.md §5.1）：
//! - 端口扫描 ≈ 40 ms（热）+ 127 ms（冷），对 20–33 ms 的 fast 轮是 +100% 以上；
//! - 端口变化频率比内存低两个数量级（一次 dev server 启动/停止），固定高频采样是纯浪费。
//!
//! 触发策略（§5.2）：
//! | 触发 | 条件 | 去抖 |
//! |---|---|---|
//! | 按需（主） | 切到端口 tab / 用户点刷新 | 前端 3 s |
//! | 定时（辅） | 主窗口停在端口 tab（`active`）时 30 s 一轮 | 3 s |
//!
//! `active == false` 时定时器**完全不扫**——常驻开销为 0，这是验收项 §12-9。

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use prune_core::net::{host as net_host, ports as net_ports, PortSnapshot};
use tauri::{AppHandle, Emitter, Manager};

use crate::sampler::{DeepInFlight, MemState};

/// 端口 tab 可见时的刷新间隔
pub const PORT_INTERVAL_SECS: u64 = 30;
/// 两轮之间的最小间隔（防抖：切 tab + 点刷新连击只扫一次）
pub const PORT_MIN_GAP_SECS: u64 = 3;
pub const EVENT_PORT: &str = "port://snapshot";

/// 端口采样状态。独立于 `MemState`：两者的采样频率与消费者都不同。
pub struct PortState {
    snapshot: Mutex<Option<PortSnapshot>>,
    /// 跨轮的项目归属缓存（cwd 查询 20 ms/进程，dev server 的 cwd 不会变）
    cache: Mutex<net_host::ProjectCache>,
    /// L1 探针结果缓存。键 `(port, pid, start_time)`——进程活着、端口没变
    /// 则协议不变，重探只会堆积 MCP session。容量熔断见 [`Self::probe_put`]。
    probe_cache: Mutex<HashMap<(u16, i32, u64), prune_core::net::probe::ProbeOutcome>>,
    /// 单飞闸门：定时器轮与手动刷新共用（绝不允许两轮 lsof 并行）
    in_flight: AtomicBool,
    /// 是否有可见消费者（主窗口停在端口 tab）。false 时定时器不跑
    active: AtomicBool,
    last_at: Mutex<Option<Instant>>,
    pub last_ms: Mutex<u64>,
}

impl Default for PortState {
    fn default() -> Self {
        Self::new()
    }
}

impl PortState {
    pub fn new() -> Self {
        Self {
            snapshot: Mutex::new(None),
            cache: Mutex::new(net_host::ProjectCache::new()),
            probe_cache: Mutex::new(HashMap::new()),
            in_flight: AtomicBool::new(false),
            active: AtomicBool::new(false),
            last_at: Mutex::new(None),
            last_ms: Mutex::new(0),
        }
    }

    /// 探针缓存读取。未命中 = None（不是「不是服务」）。
    pub fn probe_get(&self, key: &(u16, i32, u64)) -> Option<prune_core::net::probe::ProbeOutcome> {
        self.probe_cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(key)
            .cloned()
    }

    /// 探针缓存写入。容量 256 熔断：探针由用户逐行触发，正常会话到不了这个量；
    /// 真到了就整体清空重来（缓存丢失的代价只是一次重探）。
    pub fn probe_put(&self, key: (u16, i32, u64), out: prune_core::net::probe::ProbeOutcome) {
        let mut cache = self.probe_cache.lock().unwrap_or_else(|e| e.into_inner());
        if cache.len() >= 256 && !cache.contains_key(&key) {
            cache.clear();
        }
        cache.insert(key, out);
    }

    pub fn snapshot(&self) -> Option<PortSnapshot> {
        self.snapshot
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn set_active(&self, active: bool) {
        self.active.store(active, Ordering::Relaxed);
        eprintln!("[m7-port] 可见性 → {active}（false 时不采样）");
    }

    fn is_active(&self) -> bool {
        self.active.load(Ordering::Relaxed)
    }

    fn since_last(&self) -> Option<Duration> {
        self.last_at
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .map(|t| t.elapsed())
    }

    /// 距上一轮扫描多久了。端口冲突保镖（M19）用它判断「快照还新鲜吗」：
    /// 新鲜就直接复用，不再起一轮 lsof（防双扫描）。
    pub fn age(&self) -> Option<Duration> {
        self.since_last()
    }

    /// 跨轮项目归属缓存（cwd 查询 20 ms/进程）。M19 的 30 s 守望轮共享这份
    /// 缓存——不然每轮守望着都从冷缓存起步，新增进程全走一遍 cwd 查询。
    pub fn project_cache(&self) -> &Mutex<net_host::ProjectCache> {
        &self.cache
    }
}

/// 30 s 定时器轮（仅在端口 tab 可见时推进）
pub async fn run_port(app: AppHandle) {
    let mut tick = tokio::time::interval(Duration::from_secs(5));
    tick.tick().await;

    loop {
        tick.tick().await;
        let state = app.state::<Arc<PortState>>().inner().clone();
        if !state.is_active() {
            continue;
        }
        if let Some(gap) = state.since_last() {
            if gap < Duration::from_secs(PORT_INTERVAL_SECS) {
                continue;
            }
        }
        if let Err(e) = port_round(&app, false).await {
            eprintln!("[m7-port] 定时轮失败: {e}");
        }
    }
}

/// 一轮端口采样：扫描 → 缓存快照 → 广播 `port://snapshot`。
///
/// `force = false` 时受 `PORT_MIN_GAP_SECS` 去抖（定时器轮用）；
/// 手动刷新传 `true`（用户点了就该扫）。
pub async fn port_round(app: &AppHandle, force: bool) -> Result<(), String> {
    let state = app.state::<Arc<PortState>>().inner().clone();

    if !force {
        if let Some(gap) = state.since_last() {
            if gap < Duration::from_secs(PORT_MIN_GAP_SECS) {
                return Ok(());
            }
        }
    }

    // 单飞：手动刷新撞上定时器轮时，后到的一轮直接跳过
    let Some(_gate) = DeepInFlight::acquire(&state.in_flight) else {
        return Ok(());
    };

    let scan_state = state.clone();
    let handle = app.clone();
    let snap = tauri::async_runtime::spawn_blocking(move || {
        // Windows 的 iphlpapi 只给 pid，进程身份从内存轮的全量进程表借；
        // macOS 的 lsof 自带 `c` 行，exe/cmd 由进程表补（服务推断用）。
        let identities: Option<HashMap<i32, prune_core::net::service::ProcIdentity>> =
            handle.try_state::<Arc<MemState>>().map(|ms| {
                ms.processes
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .iter()
                    .map(|p| {
                        (
                            p.pid,
                            prune_core::net::service::ProcIdentity {
                                comm: p.name.clone(),
                                exe: p.exe.clone(),
                                cmd: p.cmd.clone(),
                            },
                        )
                    })
                    .collect()
            });
        let mut cache = scan_state.cache.lock().unwrap_or_else(|e| e.into_inner());
        net_ports::scan_cached(identities.as_ref(), &mut cache)
    })
    .await
    .map_err(|e| format!("端口扫描线程失败: {e}"))?;

    *state.last_at.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
    *state.last_ms.lock().unwrap_or_else(|e| e.into_inner()) = snap.scan_ms;
    *state.snapshot.lock().unwrap_or_else(|e| e.into_inner()) = Some(snap.clone());

    eprintln!(
        "[m7-port] {} 个端口 · 关注 {} · 折叠 {} · {} ms{}",
        snap.entries.len(),
        snap.watched_count(),
        snap.folded_count(),
        snap.scan_ms,
        snap.coverage
            .note
            .as_ref()
            .map(|n| format!(" · ⚠️ {n}"))
            .unwrap_or_default()
    );

    // 事件失败不影响已落库的快照（面板打开时会主动拉一次）
    let _ = app.emit(EVENT_PORT, &snap);
    Ok(())
}
