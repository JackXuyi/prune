//! IPC 命令（T-207 / T-306）：只做读取与转发，业务逻辑都在 prune-core / sampler / scan。
//!
//! 契约见 docs/m2-memory-plan.md §6 与 docs/m1-disk-plan.md §8：
//! - `mem_snapshot`   取内存中最新快照（零 IO，前端打开即用）
//! - `mem_processes`  全量进程明细（家族聚合前）
//! - `mem_history`    某进程的序列（读 SQLite）
//! - `mem_refresh`    手动触发一轮快照 + 深探
//! - `disk_report`    取最新可回收报告
//! - `disk_volume`    M9：卷容量直读（statvfs，不依赖扫描）
//! - `disk_scan`      触发一轮扫描（单飞，重复调用忽略）
//! - `reclaim_preview` dry-run：生成待清理清单
//! - `reclaim_execute` 执行清理（走废纸篓，护栏在 prune-core）
//! - `app_disk`       M8：按家族测量磁盘三层占用（按需 + TTL，绝不进定时轮）

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use prune_core::appdisk::{AppDisk, AppDiskCache, BundleQuery, BundleSize};
use prune_core::db::{HourlyRow, Point};
use prune_core::disk::{DiskReport, VolumeInfo};
use prune_core::mem::{MemSnapshot, ProcMem};
use prune_core::net::PortSnapshot;
use prune_core::reclaim::{self, Limits, ReclaimChild, ReclaimItem, ReclaimPreview, ReclaimReport};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::port_sampler::PortState;
use crate::sampler::{self, MemState};
use crate::scan::{self, DiskState};

#[tauri::command]
pub fn mem_snapshot(state: State<'_, Arc<MemState>>) -> Option<MemSnapshot> {
    state
        .snapshot
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

#[tauri::command]
pub fn mem_processes(state: State<'_, Arc<MemState>>) -> Vec<ProcMem> {
    state
        .processes
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

#[tauri::command]
pub fn mem_history(
    state: State<'_, Arc<MemState>>,
    pid: i32,
    minutes: u32,
) -> Vec<Point> {
    let since = now_secs() - (minutes.max(1) as i64) * 60;
    state.history(pid, since, 2000)
}

/// 手动刷新：先出一轮快照，再尝试深探。
/// 深探自带单飞闸门——若定时器轮正在跑，这里的 `deep_round` 会直接跳过并返回，
/// 不会出现两轮 footprint 并行；本轮结果仍会由定时器轮的 `mem://deep` 事件送达。
#[tauri::command]
pub async fn mem_refresh(app: AppHandle) -> Result<(), String> {
    sampler::fast_round(&app).await?;
    sampler::deep_round(&app).await?;
    Ok(())
}

/// 面板点击 → 打开主窗口并选中目标（可选指定 tab）。
/// 窗口显示逻辑统一走 tray::show_main（Accessory 下 show 不切 Space，需定位 + MoveToActiveSpace）。
#[tauri::command]
pub fn open_main(app: AppHandle, pid: Option<i32>, tab: Option<String>) {
    if let Some(panel) = app.get_webview_window("panel") {
        let _ = panel.hide();
    }
    crate::tray::show_main(&app);
    if let Some(pid) = pid {
        let _ = app.emit("mem://select", pid);
    }
    if let Some(tab) = tab {
        let _ = app.emit("ui://tab", tab);
    }
}

/// 结束进程：默认 SIGTERM（给应用保存数据、优雅退出的机会），`force` 时 SIGKILL。
/// Windows 没有 SIGTERM 语义，两者都走 TerminateProcess（见 `prune_core::mem::process::kill`）。
/// 护栏：不杀 1 号进程（launchd）与 Prune 自身——误杀任一都会破坏宿主环境。
#[tauri::command]
pub fn kill_process(pid: i32, force: bool) -> Result<(), String> {
    if pid <= 1 {
        return Err(format!("pid {pid} 是系统关键进程，不允许结束"));
    }
    if pid == std::process::id() as i32 {
        return Err("不能结束 Prune 自身".into());
    }
    prune_core::mem::process::kill(pid, force)?;
    eprintln!("[ipc] kill pid={pid} force={force}");
    Ok(())
}

/// 在文件管理器中定位路径：目录直接打开，文件则选中。
/// - macOS：`open` / `open -R`（Finder）
/// - Windows：`explorer` / `explorer /select,`（资源管理器）
///
/// 只做「展示」不做修改，因此不做白名单校验；但要求路径真实存在，
/// 免得弹一个空窗口。
#[tauri::command]
pub fn reveal_path(path: String) -> Result<(), String> {
    let p = std::path::Path::new(&path);
    if !p.exists() {
        return Err(format!("路径不存在：{path}"));
    }
    reveal_impl(p)
        .map_err(|e| format!("打开文件管理器失败：{e}"))
}

#[cfg(target_os = "macos")]
fn reveal_impl(p: &std::path::Path) -> std::io::Result<()> {
    let mut cmd = std::process::Command::new("open");
    if p.is_dir() {
        cmd.arg(p);
    } else {
        cmd.arg("-R").arg(p);
    }
    cmd.spawn().map(|_| ())
}

#[cfg(target_os = "windows")]
fn reveal_impl(p: &std::path::Path) -> std::io::Result<()> {
    // explorer 的解析规则很脆：`/select,<path>` 必须整体作为一条原始命令行片段传入。
    // `arg()` 会给含空格的路径套一层引号、把 `/select,` 和路径拆成两半，explorer 就认不出来；
    // `raw_arg` 是 `std::os::windows::process::CommandExt` 的 trait 方法，必须显式引入。
    use std::os::windows::process::CommandExt;
    let mut cmd = std::process::Command::new("explorer");
    if p.is_dir() {
        cmd.raw_arg(format!("\"{}\"", p.display()));
    } else {
        cmd.raw_arg(format!("/select,\"{}\"", p.display()));
    }
    cmd.spawn().map(|_| ())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn reveal_impl(p: &std::path::Path) -> std::io::Result<()> {
    let target = if p.is_dir() {
        p.to_path_buf()
    } else {
        p.parent().unwrap_or(p).to_path_buf()
    };
    std::process::Command::new("xdg-open").arg(target).spawn().map(|_| ())
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------- 端口（M7）

#[tauri::command]
pub fn port_snapshot(state: State<'_, Arc<PortState>>) -> Option<PortSnapshot> {
    state.snapshot()
}

/// 手动刷新：用户点了就该扫（给 `force`，绕开定时器的 30 s 间隔）。
/// 单飞闸门仍在——撞上正在跑的一轮时直接返回旧快照。
#[tauri::command]
pub async fn port_refresh(app: AppHandle) -> Result<(), String> {
    crate::port_sampler::port_round(&app, true).await
}

/// 可见性门控：主窗口切进/切出端口 tab 时调用。
/// `active = false` 时定时器不采样，常驻开销为 0（验收 §12-9）。
#[tauri::command]
pub fn port_set_active(state: State<'_, Arc<PortState>>, active: bool) {
    state.set_active(active);
}

// ---------------------------------------------------------------- 应用磁盘（M8）

/// 家族磁盘测量缓存。放在壳层而不是 core：它是有状态的（TTL），
/// 生命周期应当与进程一致，跟 mem/disk 的 State 一个模式。
pub struct AppDiskState {
    cache: Mutex<AppDiskCache>,
    /// M10 批量本体测量缓存：卡片首屏每张卡都要一个数，同一批数据不该反复 du。
    /// 键是家族名；30 min 足以覆盖一次会话里反复切 tab 的场景。
    bundles: Mutex<Option<(std::time::Instant, std::collections::HashMap<String, BundleSize>)>>,
}

impl AppDiskState {
    pub fn new() -> Self {
        Self {
            cache: Mutex::new(AppDiskCache::new()),
            bundles: Mutex::new(None),
        }
    }
}

impl Default for AppDiskState {
    fn default() -> Self {
        Self::new()
    }
}

/// 测量一个应用的磁盘三层（本体 / 缓存 / 数据）。
///
/// - 只读：不产生任何删除动作，也不进任何定时轮（方案 §6）
/// - `force = true` 时绕开 TTL 重测（UI 的「重新测量」）
/// - `exe` 由前端给（它本来就有进程明细），避免后端再枚举一次进程
#[tauri::command]
pub async fn app_disk(
    state: State<'_, Arc<AppDiskState>>,
    family: String,
    exe: Option<String>,
    force: Option<bool>,
) -> Result<AppDisk, String> {
    let cache = state.inner().clone();
    let home = prune_core::paths::home_dir_or_root();
    // du 是阻塞 IO（实测 Chrome 的 profile 目录可跑到 4 s 预算上限），
    // 必须离开 async 执行器，否则会卡住其他 IPC
    tauri::async_runtime::spawn_blocking(move || {
        let mut guard = cache.cache.lock().unwrap_or_else(|e| e.into_inner());
        if force.unwrap_or(false) {
            guard.invalidate(&family);
        }
        guard.get_or_measure(&family, exe.as_deref(), &home)
    })
    .await
    .map_err(|e| format!("磁盘测量失败：{e}"))
}

// ---------------------------------------------------------------- 批量本体测量（M10）

/// 批量测量一批应用的**本体**体积（卡片首屏用）。
///
/// - 只测 `.app` 本体：缓存/数据层 0.25–4 s/应用，进不了首屏
/// - 8 路并行 + 单应用 4 s 预算；超预算的条目 `partial = true`（UI 显示「≥」）
/// - 30 min 缓存；`force` 绕开（刷新按钮）。新出现的家族会补测，不会因为
///   「缓存里没有」就一直空着
#[tauri::command]
pub async fn app_bundles(
    state: State<'_, Arc<AppDiskState>>,
    items: Vec<BundleQuery>,
    force: Option<bool>,
) -> Result<std::collections::HashMap<String, BundleSize>, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut guard = state.bundles.lock().unwrap_or_else(|e| e.into_inner());

        // 命中缓存：同一批家族且未过期就直接用（新家族单独补测）
        let mut cached: std::collections::HashMap<String, BundleSize> = match guard.as_ref() {
            Some((at, map)) if !force.unwrap_or(false) && at.elapsed() < prune_core::appdisk::TTL => {
                map.clone()
            }
            _ => std::collections::HashMap::new(),
        };

        let missing: Vec<BundleQuery> = items
            .iter()
            .filter(|q| !cached.contains_key(&q.family))
            .cloned()
            .collect();

        if !missing.is_empty() {
            let fresh =
                prune_core::appdisk::bundle_bytes_batch(&missing, prune_core::appdisk::BUDGET);
            cached.extend(fresh);
            *guard = Some((std::time::Instant::now(), cached.clone()));
        }

        cached
    })
    .await
    .map_err(|e| format!("批量磁盘测量失败：{e}"))
}

// ---------------------------------------------------------------- 磁盘（M1）

#[tauri::command]
pub fn disk_report(state: State<'_, Arc<DiskState>>) -> Option<DiskReport> {
    state.report()
}

/// M9 统计 tab：home 所在卷的容量，statvfs 微秒级同步直读。
///
/// 不能用 `disk_report` 里的 `volume` 代替：那是**扫描完成时**装配的，
/// 首次启动（首扫 30 s 后才跑）或扫描失败时读数为 null——统计 tab
/// 作为默认页必须在任何扫描之前就能显示容量。
#[tauri::command]
pub fn disk_volume() -> Option<VolumeInfo> {
    prune_core::disk::volume_info(&prune_core::paths::home_dir_or_root())
}

/// 手动触发扫描（A7 D-A7-2：手动 = 全量，用户盯着数字求证，准确性优先）。
/// 已有扫描在跑时直接返回 Ok（幂等），前端靠 `disk://done` 更新。
#[tauri::command]
pub async fn disk_scan(app: AppHandle) -> Result<(), String> {
    scan::scan_now(&app, true).await
}

/// dry-run：只生成清单，不动任何文件。
#[tauri::command]
pub fn reclaim_preview(state: State<'_, Arc<DiskState>>, keys: Vec<String>) -> ReclaimPreview {
    match state.report() {
        Some(report) => reclaim::preview(&keys, &report),
        None => ReclaimPreview {
            items: Vec::new(),
            total_bytes: 0,
            needs_confirm: false,
            notes: vec!["尚未完成扫描，请稍候或手动刷新".into()],
        },
    }
}

/// 一级子项体积明细（按体积降序）：du 较慢，独立命令让预览弹层先出、内容后到。
/// du 放到阻塞线程池，不卡主线程。
#[tauri::command]
pub async fn reclaim_children(path: String) -> Result<Vec<ReclaimChild>, String> {
    let p = PathBuf::from(&path);
    if !p.is_dir() {
        return Err(format!("不是目录：{path}"));
    }
    tauri::async_runtime::spawn_blocking(move || Ok(reclaim::first_level_children(&p)))
        .await
        .map_err(|e| format!("明细线程失败: {e}"))?
}

/// 执行清理：路径校验 / 级别闸门 / 熔断都在 prune-core，这里只转发。
/// `allowed` 从当前报告的明细表组装（A4：node_modules 逐项清理的报告派生白名单）。
/// 成功后自动增量重扫一轮，前端通过 `disk://done` 收到新报告。
#[tauri::command]
pub async fn reclaim_execute(
    app: AppHandle,
    state: State<'_, Arc<DiskState>>,
    items: Vec<ReclaimItem>,
    confirmed: bool,
) -> Result<ReclaimReport, String> {
    let home = scan::home_dir();
    // 报告派生白名单：只有当前报告里出现过的明细路径会被放行
    let allowed: Vec<String> = state
        .report()
        .map(|r| {
            r.groups
                .iter()
                .flat_map(|g| g.details.iter().map(|d| d.path.clone()))
                .collect()
        })
        .unwrap_or_default();

    let result = tauri::async_runtime::spawn_blocking(move || {
        reclaim::execute(&items, &home, confirmed, Limits::default(), &allowed)
    })
    .await
    .map_err(|e| format!("清理线程失败: {e}"))?
    .map_err(|e| e.to_string())?;

    eprintln!(
        "[m1-disk] 已移入废纸篓 {} 项 · {:.2} GB · 失败 {} 项",
        result.moved,
        result.bytes as f64 / 1e9,
        result.failed.len()
    );

    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = scan::scan_now(&handle, false).await {
            eprintln!("[m1-disk] 清理后增量重扫失败: {e}");
        }
    });

    Ok(result)
}

// ---------------------------------------------------------------- 应用图标（D1）

/// 前端一次请求的图标上限：首屏 12 + 一屏滚动余量足够，防止误传 348 个把 IPC 拖长
const ICON_BATCH_MAX: usize = 64;
/// 单应用转码预算：超过就按失败处理（前端落字母徽标），保证一批不被人拖住
const ICON_BUDGET: std::time::Duration = std::time::Duration::from_millis(300);
/// 并行度：sips 是独立进程，8 路实测能吃掉 30 个图标而不拖慢采样
const ICON_CONCURRENCY: usize = 8;

#[derive(serde::Deserialize)]
pub struct IconQuery {
    pub family: String,
    pub exe: Option<String>,
}

/// 图标缓存根目录（只写自有目录，不碰系统位置）。
///
/// - macOS：`~/Library/Caches/com.xuyi.prune/icons`
/// - Windows：`%LOCALAPPDATA%\com.xuyi.prune\icons`
fn icon_cache_root() -> PathBuf {
    let base = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else {
        prune_core::paths::home_dir().map(|h| h.join("Library").join("Caches"))
    };
    base.unwrap_or_else(std::env::temp_dir)
        .join("com.xuyi.prune")
        .join("icons")
}

/// 批量取应用图标：返回 `family -> data:image/png;base64,...`。
///
/// 失败语义：单个家族失败（系统进程、无 icns、超预算）**不放进 map**，也不报错；
/// 只有整批线程起不来才返回 `Err`。前端对缺失项统一落字母徽标。
#[tauri::command]
pub async fn app_icons(items: Vec<IconQuery>) -> Result<HashMap<String, String>, String> {
    let items: Vec<IconQuery> = items.into_iter().take(ICON_BATCH_MAX).collect();
    if items.is_empty() {
        return Ok(HashMap::new());
    }
    let cache = icon_cache_root();

    tauri::async_runtime::spawn_blocking(move || {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Mutex as StdMutex;

        let items = &items;
        let cursor = AtomicUsize::new(0);
        let out: StdMutex<HashMap<String, String>> = StdMutex::new(HashMap::new());
        let started = std::time::Instant::now();

        std::thread::scope(|scope| {
            for _ in 0..ICON_CONCURRENCY.min(items.len()) {
                scope.spawn(|| loop {
                    let i = cursor.fetch_add(1, Ordering::Relaxed);
                    let Some(item) = items.get(i) else { break };
                    let Some(exe) = item.exe.as_deref() else { continue };

                    let t0 = std::time::Instant::now();
                    let Ok(path) =
                        prune_core::appicon::icon_png(exe, prune_core::appicon::ICON_SIZE, &cache)
                    else {
                        continue;
                    };
                    if t0.elapsed() > ICON_BUDGET {
                        eprintln!(
                            "[d1-icon] {} 转码超预算（{} ms），本次按失败处理",
                            item.family,
                            t0.elapsed().as_millis()
                        );
                        continue;
                    }
                    let Ok(bytes) = std::fs::read(&path) else { continue };
                    let url = format!(
                        "data:image/png;base64,{}",
                        prune_core::base64::encode(&bytes)
                    );
                    if let Ok(mut map) = out.lock() {
                        map.insert(item.family.clone(), url);
                    }
                });
            }
        });

        let map = out.into_inner().unwrap_or_default();
        eprintln!(
            "[d1-icon] 请求 {} 个 · 命中 {} 个 · {} ms",
            items.len(),
            map.len(),
            started.elapsed().as_millis()
        );
        Ok(map)
    })
    .await
    .map_err(|e| format!("图标线程失败: {e}"))?
}

// ---------------------------------------------------------------- 应用历史（D3）

/// 应用近 N 小时的历史聚合（小时桶）。
///
/// 只返回**有记录的小时**：缺失由前端画灰柱，不在这里补 0
/// （补 0 等于把「Prune 没运行」说成「那一小时 CPU 为 0」）。
#[tauri::command]
pub fn app_history(
    state: State<'_, Arc<MemState>>,
    family: String,
    hours: u32,
) -> Result<Vec<HourlyRow>, String> {
    let from = prune_core::localtime::hour_string(
        now_secs() - (hours.clamp(1, 24 * 10) as i64) * 3600,
    );
    state
        .app_history(&family, &from)
        .map_err(|e| format!("读取应用历史失败：{e}"))
}
