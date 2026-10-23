//! 内存采样调度（T-206）：双频定时器 + 事件推送。
//!
//! | 频率 | 内容 | 数据源 |
//! |---|---|---|
//! | 10 s | 全量进程 RSS + vm_stat + swap + 压力 | sysinfo / vm_stat / sysctl |
//! | 5 min | Top N footprint 深探（并发 8 路） | `/usr/bin/footprint -p --swapped -j` |
//!
//! 关键约束：
//! - 深探是**付费**动作，只对 RSS ≥ 80 MB 的进程做，最多 40 个；
//! - 深探结果按 `pid + start_time` 缓存，快照轮次合并时校验 start_time，
//!   避免 pid 复用导致序列串台；
//! - 任何 footprint 失败都只降级该进程到 RSS 口径，绝不伪造数值。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use prune_core::db::Store;
use prune_core::mem::process::Scanner;
use prune_core::mem::{self, MemSnapshot, MemSource, ProcMem};
use prune_core::mem::system::{self, SystemMem};
use prune_core::swap::{self, SwapInfo};
use tauri::{AppHandle, Emitter, Manager};

/// 10 s：全量 RSS 快照
pub const FAST_INTERVAL_SECS: u64 = 10;
/// 5 min：footprint 深探
pub const DEEP_INTERVAL_SECS: u64 = 300;
/// 深探候选门槛：RSS ≥ 80 MB
pub const DEEP_MIN_RSS: u64 = 80 * 1024 * 1024;
/// 单轮深探进程上限
pub const DEEP_MAX_PROCS: usize = 40;
/// 深探并发上限
pub const DEEP_CONCURRENCY: usize = 8;
/// 序列保留 7 天（D-M4）
pub const RETENTION_SECS: i64 = 7 * 24 * 3600;

pub const EVENT_SNAPSHOT: &str = "mem://snapshot";
pub const EVENT_DEEP: &str = "mem://deep";

/// 深探缓存行（按 pid 存，合并时校验 start_time）
#[derive(Debug, Clone, Copy)]
struct DeepRow {
    start_time: u64,
    footprint: u64,
    swapped: u64,
}

/// 跨轮次保存的状态
#[derive(Default)]
struct Prev {
    /// (swapins, swapouts, ts)
    swap_counters: Option<(u64, u64, i64)>,
    swap_file_count: Option<usize>,
}

/// 采样器与命令共享的状态
pub struct MemState {
    pub snapshot: Mutex<Option<MemSnapshot>>,
    pub processes: Mutex<Vec<ProcMem>>,
    scanner: Mutex<Scanner>,
    store: Option<Arc<Store>>,
    deep: Mutex<HashMap<i32, DeepRow>>,
    prev: Mutex<Prev>,
    deep_available: Mutex<bool>,
    deep_updated_at: Mutex<Option<i64>>,
    /// 最近两轮耗时（ms），供性能预算校验
    pub last_fast_ms: Mutex<u64>,
    pub last_deep_ms: Mutex<u64>,
}

impl MemState {
    pub fn new(store: Option<Arc<Store>>) -> Self {
        Self {
            snapshot: Mutex::new(None),
            processes: Mutex::new(Vec::new()),
            scanner: Mutex::new(Scanner::new()),
            store,
            deep: Mutex::new(HashMap::new()),
            prev: Mutex::new(Prev::default()),
            deep_available: Mutex::new(true),
            deep_updated_at: Mutex::new(None),
            last_fast_ms: Mutex::new(0),
            last_deep_ms: Mutex::new(0),
        }
    }

    fn lock_deep(&self) -> std::sync::MutexGuard<'_, HashMap<i32, DeepRow>> {
        self.deep.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 查询序列（无库时返回空表，UI 显示「暂无历史」而不是报错）
    pub fn history(&self, pid: i32, since: i64, limit: usize) -> Vec<prune_core::db::Point> {
        match &self.store {
            Some(store) => store.history(pid, since, limit).unwrap_or_default(),
            None => Vec::new(),
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

        let procs = {
            let mut scanner = state.scanner.lock().unwrap_or_else(|e| e.into_inner());
            scanner.scan()
        };
        let merged = merge_deep(procs, &state.lock_deep());
        let snapshot = assemble(&state, &merged);

        let elapsed = started.elapsed().as_millis() as u64;
        *state.last_fast_ms.lock().unwrap_or_else(|e| e.into_inner()) = elapsed;
        *state.processes.lock().unwrap_or_else(|e| e.into_inner()) = merged;
        *state.snapshot.lock().unwrap_or_else(|e| e.into_inner()) = Some(snapshot.clone());

        eprintln!(
            "[m2-mem] 快照轮 {} ms · {} 进程 · swap {}/{} · 压力 {:?}",
            elapsed,
            snapshot.families.iter().map(|f| f.process_count).sum::<usize>(),
            snapshot.swap.used / 1024 / 1024,
            snapshot.swap.total / 1024 / 1024,
            snapshot.pressure.level
        );
        snapshot
    })
    .await
    .map_err(|e| format!("spawn_blocking 失败: {e}"))?;

    let _ = app.emit(EVENT_SNAPSHOT, &outcome);
    Ok(())
}

/// 5 min 轮：Top N footprint 深探 → 更新缓存 → 重建快照 → 落库
pub async fn deep_round(app: &AppHandle) -> Result<(), String> {
    let state = state(app);
    let app = app.clone();

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
    let updated = {
        let mut deep = probe_state.lock_deep();
        // 重建缓存：只保留本轮成功的进程，避免进程退出后残影
        deep.clear();
        let procs = probe_state.processes.lock().unwrap_or_else(|e| e.into_inner());
        for (pid, sample) in &samples {
            if let Some(p) = procs.iter().find(|p| p.pid == *pid) {
                deep.insert(
                    *pid,
                    DeepRow {
                        start_time: p.start_time,
                        footprint: sample.footprint,
                        swapped: sample.swapped,
                    },
                );
            }
        }
        deep.len()
    };

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
        let merged = merge_deep(procs, &probe_state.lock_deep());
        let snapshot = assemble(&probe_state, &merged);
        *probe_state
            .processes
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = merged.clone();
        *probe_state
            .snapshot
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(snapshot.clone());

        // 落库：只写深探成功的行（方案 §5.4）
        if let Some(store) = &probe_state.store {
            match store.record(ts, &merged) {
                Ok(n) => {
                    let cutoff = ts - RETENTION_SECS;
                    let pruned = store.prune_before(cutoff).unwrap_or(0);
                    eprintln!("[m2-mem] 序列写入 {n} 行，清理过期 {pruned} 行");
                }
                Err(e) => eprintln!("[m2-mem] ⚠️ 序列写入失败: {e}"),
            }
        }
        snapshot
    };

    eprintln!(
        "[m2-mem] 深探轮 {elapsed} ms · 成功 {success}/{} · 覆盖 {updated} 进程 · 口径 {}",
        target_count,
        if available { "footprint" } else { "RSS（降级）" }
    );

    let _ = app.emit(EVENT_DEEP, &snapshot);
    Ok(())
}

/// 深探候选：RSS ≥ 80 MB，按 RSS 降序取前 40 个；候选不足 10 个时放宽到前 10
fn select_deep_targets(procs: &[ProcMem]) -> Vec<i32> {
    let mut candidates: Vec<&ProcMem> = procs.iter().filter(|p| p.rss >= DEEP_MIN_RSS).collect();
    candidates.sort_by_key(|p| std::cmp::Reverse(p.rss));
    candidates.truncate(DEEP_MAX_PROCS);

    if candidates.is_empty() {
        let mut top: Vec<&ProcMem> = procs.iter().collect();
        top.sort_by_key(|p| std::cmp::Reverse(p.rss));
        top.truncate(10);
        candidates = top;
    }

    candidates
        .into_iter()
        .map(|p| p.pid)
        .filter(|pid| *pid > 0)
        .collect()
}

/// 合并深探缓存：start_time 不一致视为 pid 复用，丢弃该行
fn merge_deep(mut procs: Vec<ProcMem>, deep: &HashMap<i32, DeepRow>) -> Vec<ProcMem> {
    for p in &mut procs {
        if let Some(row) = deep.get(&p.pid) {
            if row.start_time == p.start_time {
                p.footprint = Some(row.footprint);
                p.swapped = Some(row.swapped);
                p.source = MemSource::Footprint;
            }
        }
    }
    procs
}

/// 读系统侧数据并装配快照（两条轮次共用）
fn assemble(state: &MemState, procs: &[ProcMem]) -> MemSnapshot {
    let now = now_secs();
    let system: SystemMem = system::read().unwrap_or(SystemMem {
        page_size: 4096,
        pages_free: 0,
        pages_compressor: 0,
        swapins_total: 0,
        swapouts_total: 0,
        pageins_total: 0,
        pageouts_total: 0,
    });
    let pressure = system::read_pressure();

    let swap_info: SwapInfo;
    let thrash;
    {
        let mut prev = state.prev.lock().unwrap_or_else(|e| e.into_inner());
        swap_info = swap::collect(prev.swap_file_count);
        prev.swap_file_count = Some(swap_info.file_count);
        thrash = mem::thrash_rate(
            prev.swap_counters,
            system.swapins_total,
            system.swapouts_total,
            now,
        );
        prev.swap_counters = Some((system.swapins_total, system.swapouts_total, now));
    }

    let physical_total = system::physical_total();
    let deep_available = *state
        .deep_available
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let deep_updated_at = *state
        .deep_updated_at
        .lock()
        .unwrap_or_else(|e| e.into_inner());

    mem::build_snapshot(
        physical_total,
        &system,
        swap_info,
        pressure,
        thrash,
        procs,
        now,
        deep_updated_at,
        deep_available,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proc(pid: i32, rss: u64, start_time: u64) -> ProcMem {
        ProcMem {
            pid,
            ppid: 1,
            name: format!("p{pid}"),
            exe: None,
            family: "Test".into(),
            rss,
            footprint: None,
            swapped: None,
            source: MemSource::Rss,
            start_time,
        }
    }

    #[test]
    fn deep_targets_respect_threshold_and_cap() {
        let mut procs: Vec<ProcMem> = (1..=60)
            .map(|i| proc(i, (100 - i) as u64 * 1024 * 1024, 0))
            .collect();
        // 加入几个小进程，必须被排除
        procs.push(proc(999, 10 * 1024 * 1024, 0));
        let targets = select_deep_targets(&procs);
        assert!(targets.len() <= DEEP_MAX_PROCS);
        assert!(!targets.contains(&999));
        // 取的是 RSS 最大的那批
        assert!(targets.contains(&1));
    }

    #[test]
    fn deep_targets_fall_back_to_top10_when_all_small() {
        let procs: Vec<ProcMem> = (1..=20).map(|i| proc(i, 5 * 1024 * 1024, 0)).collect();
        let targets = select_deep_targets(&procs);
        assert_eq!(targets.len(), 10);
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
                swapped: 900,
            },
        );
        // pid 2 的 start_time 不匹配 → 视为复用，不应合并
        deep.insert(
            2,
            DeepRow {
                start_time: 999,
                footprint: 2000,
                swapped: 1900,
            },
        );
        let merged = merge_deep(procs, &deep);
        assert_eq!(merged[0].footprint, Some(1000));
        assert_eq!(merged[0].source, MemSource::Footprint);
        assert_eq!(merged[1].footprint, None);
        assert_eq!(merged[1].source, MemSource::Rss);
    }
}
