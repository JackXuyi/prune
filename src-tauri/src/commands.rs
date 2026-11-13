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

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};

use prune_core::appdisk::{AppDisk, AppDiskCache, BundleQuery, BundleSize};
use prune_core::db::Store;
use prune_core::db::{CutSummary, CutTop, HourlyRow, Point};
use prune_core::disk::{DiskReport, VolumeInfo};
use prune_core::i18n::err::err_text;
use prune_core::i18n::fill;
use prune_core::mem::{MemSnapshot, ProcMem};
use prune_core::net::PortSnapshot;
use prune_core::reclaim::{
    self, Limits, ReclaimChild, ReclaimItem, ReclaimNote, ReclaimPreview, ReclaimReport,
};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::port_sampler::PortState;
use crate::sampler::{self, MemState};
use crate::scan::{self, DiskState};

/// 剪除成绩单的记录保留期（天）。与卡片上「本地留存两年」、changelog 的
/// 「留存 800 天」同一口径——只有写入路径会剪旧行，读路径保持纯查询。
const CUT_RETAIN_DAYS: i64 = 800;

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
pub fn mem_history(state: State<'_, Arc<MemState>>, pid: i32, minutes: u32) -> Vec<Point> {
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

/// 面板点击 → 打开主窗口并选中目标（可选指定 tab，或直接弹设置）。
/// 窗口显示逻辑统一走 tray::show_main（Accessory 下 show 不切 Space，需定位 + MoveToActiveSpace）。
#[tauri::command]
pub fn open_main(app: AppHandle, pid: Option<i32>, tab: Option<String>, settings: Option<bool>) {
    if let Some(panel) = app.get_webview_window("panel") {
        let _ = panel.hide();
    }
    crate::tray::show_main(&app);
    // 顺序要紧：emit 必须排在 show_main 之后 —— webview 还没显示时事件会丢，
    // 用户点了齿轮却什么都没发生
    if settings.unwrap_or(false) {
        let _ = app.emit("ui://settings", ());
    }
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
pub fn kill_process(app: AppHandle, pid: i32, force: bool) -> Result<(), String> {
    let t = err_text(crate::i18n::current(&app));
    if pid <= 1 {
        return Err(fill(t.pid_critical, &[("pid", &pid.to_string())]));
    }
    if pid == std::process::id() as i32 {
        return Err(t.self_kill.into());
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
pub fn reveal_path(app: AppHandle, path: String) -> Result<(), String> {
    let t = err_text(crate::i18n::current(&app));
    let p = std::path::Path::new(&path);
    if !p.exists() {
        return Err(fill(t.path_missing, &[("path", &path)]));
    }
    reveal_impl(p).map_err(|e| fill(t.reveal_failed, &[("err", &e.to_string())]))
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
    std::process::Command::new("xdg-open")
        .arg(target)
        .spawn()
        .map(|_| ())
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
    bundles: Mutex<
        Option<(
            std::time::Instant,
            std::collections::HashMap<String, BundleSize>,
        )>,
    >,
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
    app: AppHandle,
    state: State<'_, Arc<AppDiskState>>,
    family: String,
    exe: Option<String>,
    force: Option<bool>,
) -> Result<AppDisk, String> {
    let t = err_text(crate::i18n::current(&app));
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
    .map_err(|e| fill(t.disk_measure_failed, &[("err", &e.to_string())]))
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
    app: AppHandle,
    state: State<'_, Arc<AppDiskState>>,
    items: Vec<BundleQuery>,
    force: Option<bool>,
) -> Result<std::collections::HashMap<String, BundleSize>, String> {
    let t = err_text(crate::i18n::current(&app));
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut guard = state.bundles.lock().unwrap_or_else(|e| e.into_inner());

        // 命中缓存：同一批家族且未过期就直接用（新家族单独补测）
        let mut cached: std::collections::HashMap<String, BundleSize> = match guard.as_ref() {
            Some((at, map))
                if !force.unwrap_or(false) && at.elapsed() < prune_core::appdisk::TTL =>
            {
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
    .map_err(|e| fill(t.disk_measure_batch_failed, &[("err", &e.to_string())]))
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

/// 成绩单落库口径（M21）：只认「执行前还在磁盘上、执行后没进失败名单」的条目。
///
/// 之所以要 `preexisting`：`reclaim::execute` 对报告过期（上轮已清理 / 用户手动删）
/// 的条目记 `skipped` 而不是 `failed`，它们**没被移进废纸篓**。只排除 `failed`
/// 会把它们算成「剪掉了」，成绩单的累计数字就虚高了。
fn cut_entries<'a>(
    items: &'a [ReclaimItem],
    preexisting: &HashSet<String>,
    failed: &HashSet<String>,
) -> Vec<(&'a str, u64)> {
    items
        .iter()
        .filter(|i| preexisting.contains(&i.path) && !failed.contains(i.path.as_str()))
        .map(|i| (i.label.as_str(), i.bytes))
        .collect()
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
            // 文案在前端 catalog：后端只给 code（见 `ReclaimNote` 的说明）
            notes: vec![ReclaimNote::ScanPending],
        },
    }
}

/// 一级子项体积明细（按体积降序）：du 较慢，独立命令让预览弹层先出、内容后到。
/// du 放到阻塞线程池，不卡主线程。
#[tauri::command]
pub async fn reclaim_children(app: AppHandle, path: String) -> Result<Vec<ReclaimChild>, String> {
    let t = err_text(crate::i18n::current(&app));
    let p = PathBuf::from(&path);
    if !p.is_dir() {
        return Err(fill(t.not_a_dir, &[("path", &path)]));
    }
    tauri::async_runtime::spawn_blocking(move || Ok(reclaim::first_level_children(&p)))
        .await
        .map_err(|e| fill(t.detail_thread, &[("err", &e.to_string())]))?
}

/// 执行清理：路径校验 / 级别闸门 / 熔断都在 prune-core，这里只转发。
/// `allowed` 从当前报告的明细表组装（A4：node_modules 逐项清理的报告派生白名单）。
/// 成功后自动增量重扫一轮，前端通过 `disk://done` 收到新报告。
#[tauri::command]
pub async fn reclaim_execute(
    app: AppHandle,
    state: State<'_, Arc<DiskState>>,
    store: State<'_, Option<Arc<Store>>>,
    items: Vec<ReclaimItem>,
    confirmed: bool,
) -> Result<ReclaimReport, String> {
    let locale = crate::i18n::current(&app);
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

    // 闭包要拿走 items（跨线程）；落库在 await 之后用另一份
    let items_for_cut = items.clone();
    // 执行前快照「这一刻还在磁盘上的条目」：成绩单只认真正被移走的，
    // 报告过期导致的 skipped（路径已消失）必须排除，否则数字虚高
    let preexisting: HashSet<String> = items
        .iter()
        .filter(|i| std::path::Path::new(&i.path).exists())
        .map(|i| i.path.clone())
        .collect();
    let result = tauri::async_runtime::spawn_blocking(move || {
        reclaim::execute(&items, &home, confirmed, Limits::default(), &allowed)
    })
    .await
    .map_err(|e| fill(err_text(locale).reclaim_thread, &[("err", &e.to_string())]))?
    .map_err(|e| e.text(locale))?;

    eprintln!(
        "[m1-disk] 已移入废纸篓 {} 项 · {:.2} GB · 失败 {} 项",
        result.moved,
        result.bytes as f64 / 1e9,
        result.failed.len()
    );

    // M21 剪除成绩单：成功项落库（append-only）。失败项与「执行时已不存在」的
    // 条目都不计——「剪掉了」必须以废纸篓里的东西为准，报告数字才经得起对账。
    if let Some(store) = store.as_deref() {
        let failed: HashSet<String> = result.failed.iter().map(|f| f.path.clone()).collect();
        let cuts = cut_entries(&items_for_cut, &preexisting, &failed);
        if let Err(e) = store.cut_add(&cuts, now_secs()) {
            eprintln!("[m21-cut] 剪除记录落库失败（不影响清理结果）: {e}");
        }
        // 保留期：与 changelog / 卡片上写的「两年」对齐（只写入路径剪旧行，
        // 记录量级几百行/年，一次 DELETE 成本可忽略）
        let cutoff = now_secs() - CUT_RETAIN_DAYS * 86_400;
        match store.cut_prune_before(cutoff) {
            Ok(0) => {}
            Ok(n) => eprintln!("[m21-cut] 清理超出保留期的剪除记录 {n} 行"),
            Err(e) => eprintln!("[m21-cut] 剪除记录过期清理失败（不影响清理结果）: {e}"),
        }
    }

    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = scan::scan_now(&handle, false).await {
            eprintln!("[m1-disk] 清理后增量重扫失败: {e}");
        }
    });

    Ok(result)
}

/// 剪除成绩单（M21）：累计字节 / 次数 / 首条时间 / 受害者 Top 5。
/// store 不可用时返回全 0 + 空 Top（前端空态，不是报错——与 net「今日不可用」同纪律）。
#[tauri::command]
pub fn cut_report(
    app: AppHandle,
    store: State<'_, Option<Arc<Store>>>,
) -> Result<CutReportDto, String> {
    let t = err_text(crate::i18n::current(&app));
    let Some(store) = store.as_deref() else {
        return Ok(CutReportDto {
            total_bytes: 0,
            total_items: 0,
            first_ts: None,
            top: Vec::new(),
        });
    };
    let summary = store
        .cut_summary()
        .map_err(|e| fill(t.cuts_read, &[("err", &e.to_string())]))?;
    let top = store
        .cut_top(5)
        .map_err(|e| fill(t.cuts_top_read, &[("err", &e.to_string())]))?;
    let CutSummary {
        total_bytes,
        total_items,
        first_ts,
    } = summary;
    Ok(CutReportDto {
        total_bytes,
        total_items,
        first_ts,
        top,
    })
}

/// `cut_report` 的返回 DTO（camelCase，与 Rust 侧 struct 字段一一对应）
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CutReportDto {
    pub total_bytes: u64,
    pub total_items: u64,
    pub first_ts: Option<i64>,
    pub top: Vec<CutTop>,
}

/// 悬案卷宗（M22）：泄漏嫌疑的案件簿快照。侦查中的在前，已归档在后。
/// 案件簿在内存中，Prune 重启即清零（档案描述「这一段试用期」）。
#[tauri::command]
pub fn leak_cases(state: State<'_, Arc<MemState>>) -> Vec<prune_core::leak::LeakCase> {
    state.cases().to_vec()
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
pub async fn app_icons(
    app: AppHandle,
    items: Vec<IconQuery>,
) -> Result<HashMap<String, String>, String> {
    let t = err_text(crate::i18n::current(&app));
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
                    let Some(exe) = item.exe.as_deref() else {
                        continue;
                    };

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
                    let Ok(bytes) = std::fs::read(&path) else {
                        continue;
                    };
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
    .map_err(|e| fill(t.icon_thread, &[("err", &e.to_string())]))?
}

// ---------------------------------------------------------------- 应用历史（D3）

/// 应用近 N 小时的历史聚合（小时桶）。
///
/// 只返回**有记录的小时**：缺失由前端画灰柱，不在这里补 0
/// （补 0 等于把「Prune 没运行」说成「那一小时 CPU 为 0」）。
#[tauri::command]
pub fn app_history(
    app: AppHandle,
    state: State<'_, Arc<MemState>>,
    family: String,
    hours: u32,
) -> Result<Vec<HourlyRow>, String> {
    let t = err_text(crate::i18n::current(&app));
    // M25：保留期扩到 30 天，查询窗口同步放开到 30 天
    let from =
        prune_core::localtime::hour_string(now_secs() - (hours.clamp(1, 24 * 30) as i64) * 3600);
    state
        .app_history(&family, &from)
        .map_err(|e| fill(t.app_history_read, &[("err", &e.to_string())]))
}

/// 应用近 N 天的历史聚合（天粒度，M25）。
///
/// 聚合在 SQLite 里完成（720 个小时桶 → 30 行），只返回**有记录的天**：
/// 缺失由前端画灰柱，不在这里补 0（与小时桶同纪律）。
#[tauri::command]
pub fn app_history_daily(
    app: AppHandle,
    state: State<'_, Arc<MemState>>,
    family: String,
    days: u32,
) -> Result<Vec<prune_core::db::DailyRow>, String> {
    let t = err_text(crate::i18n::current(&app));
    let from = prune_core::localtime::day_string(now_secs() - (days.clamp(1, 30) as i64) * 86_400);
    state
        .app_history_daily(&family, &from)
        .map_err(|e| fill(t.app_day_history_read, &[("err", &e.to_string())]))
}

// ---------------------------------------------------------------- M30 项目休眠归档（F14）

/// 进度事件名（前端 `listen` 同名）
const ARCHIVE_PROGRESS_EVENT: &str = "archive://progress";
/// 进度节流窗口：node_modules 十几万条目，逐条 emit 会把 IPC 打满
const PROGRESS_THROTTLE_MS: u64 = 200;
/// 归档区残留 `.part` 的清理阈值（秒）。进行中的打包 mtime 是新鲜的，
/// 只有强杀 / 断电留下的陈旧半成品才会被扫掉。
const STALE_PART_SECS: u64 = 600;

/// 归档/恢复的串行闸门：同一时刻只允许一个任务。压缩是 CPU 密集操作，
/// 并行只会互相拖慢并把磁盘打满；第二个请求**立即返回**而不是排队——
/// 排队会让用户以为「点了没反应」，正是 0.9.0 卡死投诉的同一个心理模型。
static ARCHIVE_BUSY: AtomicBool = AtomicBool::new(false);
/// 当前任务的取消令牌：`archive_cancel` 置位，每次开新任务时清零
static ARCHIVE_CANCEL: AtomicBool = AtomicBool::new(false);

/// 闸门守卫：Drop 时自动放行，panic / 提前 return 都不会把应用锁死
struct ArchiveGuard;

impl ArchiveGuard {
    fn acquire() -> Result<Self, String> {
        ARCHIVE_BUSY
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .map(|_| {
                ARCHIVE_CANCEL.store(false, Ordering::SeqCst);
                ArchiveGuard
            })
            .map_err(|_| "上一个归档任务还在进行中，请等它完成或先取消".to_string())
    }
}

impl Drop for ArchiveGuard {
    fn drop(&mut self) {
        ARCHIVE_BUSY.store(false, Ordering::SeqCst);
    }
}

/// 进度节流：阶段切换、每 200 ms、以及「跑满」这三种情况才发事件。
///
/// 为什么按「字节 + 阶段」而不是按条目：进度条只关心字节进度；一次归档
/// 十几万条目，逐条发事件会把 IPC 和渲染线程打满——那是把「卡死」换成
/// 「卡顿」，不算修好。
fn progress_sink(app: AppHandle) -> impl Fn(&prune_core::archive::Progress) + Send + 'static {
    let base = std::time::Instant::now();
    let last_ms = AtomicU64::new(0);
    let last_stage = AtomicU8::new(u8::MAX);
    move |p: &prune_core::archive::Progress| {
        let stage = p.stage as u8;
        let ms = base.elapsed().as_millis() as u64;
        let prev_stage = last_stage.load(Ordering::Relaxed);
        // 收尾必须发：否则前端进度条停在 97% 直到任务返回
        let full = p.total_bytes > 0 && p.done_bytes >= p.total_bytes;
        let due = ms.saturating_sub(last_ms.load(Ordering::Relaxed)) >= PROGRESS_THROTTLE_MS;
        if stage == prev_stage && !due && !full {
            return;
        }
        last_stage.store(stage, Ordering::Relaxed);
        last_ms.store(ms, Ordering::Relaxed);
        let _ = app.emit(ARCHIVE_PROGRESS_EVENT, p);
    }
}

/// 归档区根（`<config>/com.xuyi.prune/archive`）。
fn archive_root_or_err() -> Result<PathBuf, String> {
    prune_core::archive::system_archive_root().ok_or_else(|| "找不到应用配置目录".to_string())
}

/// 项目根 → 依赖目录。**不接受前端传任意路径**：只拼 `node_modules`，
/// 再由 prune-core 的护栏校验（目录名 / 绝对路径 / 非 symlink）。
fn deps_of(project_root: &str) -> PathBuf {
    PathBuf::from(project_root).join("node_modules")
}

/// 归档账本（顺带清理异常中断留下的 `.part`）。
///
/// 只读小 JSON，保留同步写法；清理动作只碰归档区里陈旧的 `.part`。
#[tauri::command]
pub fn archive_list() -> Result<Vec<prune_core::archive::ArchiveEntry>, String> {
    let root = archive_root_or_err()?;
    prune_core::archive::cleanup_stale_parts(&root, STALE_PART_SECS);
    Ok(prune_core::archive::list(&root))
}

/// dry-run：统计依赖目录体积与目标归档路径，不写任何文件。
///
/// **必须 spawn_blocking**：统计要遍历十几万条目，同步调用会占住主线程
/// （Tauri v2 的同步 command 跑在主线程，见 docs/diag-m30-archive-freeze.md）。
#[tauri::command]
pub async fn archive_plan(
    project_root: String,
) -> Result<prune_core::archive::ArchivePlan, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let root = archive_root_or_err()?;
        prune_core::archive::plan(&deps_of(&project_root), &root, now_secs())
    })
    .await
    .map_err(|e| format!("归档线程异常：{e}"))?
}

/// 执行归档：必须 `confirmed`（先预览再执行）。校验通过后才删原件。
///
/// 全程在阻塞线程池里跑，进度以 `archive://progress` 事件推出，可被
/// `archive_cancel` 取消（取消后原件不动、不留 `.part`、不记账）。
#[tauri::command]
pub async fn archive_create(
    app: AppHandle,
    project_root: String,
    confirmed: bool,
) -> Result<prune_core::archive::ArchiveEntry, String> {
    if !confirmed {
        return Err("归档需要确认：请先预览再执行".into());
    }
    let _guard = ArchiveGuard::acquire()?;
    let cancel: &'static AtomicBool = &ARCHIVE_CANCEL;
    let sink = progress_sink(app);
    tauri::async_runtime::spawn_blocking(move || {
        let root = archive_root_or_err()?;
        prune_core::archive::create_guarded(
            &deps_of(&project_root),
            &root,
            now_secs(),
            &sink,
            Some(cancel),
        )
    })
    .await
    .map_err(|e| format!("归档线程异常：{e}"))?
}

/// dry-run：恢复目标 / 体积 / 冲突标记，不写任何文件
#[tauri::command]
pub async fn archive_restore_preview(
    id: String,
) -> Result<prune_core::archive::RestorePreview, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let root = archive_root_or_err()?;
        let entry = prune_core::archive::find(&root, &id)
            .ok_or_else(|| "归档记录不存在（账本可能已被修改）".to_string())?;
        Ok(prune_core::archive::restore_preview(&entry))
    })
    .await
    .map_err(|e| format!("归档线程异常：{e}"))?
}

/// 执行恢复：`confirmed` 必填；`overwrite` 用于目标已存在时的二次确认
/// （覆盖也走废纸篓）。校验失败会把解压产物移入废纸篓。
#[tauri::command]
pub async fn archive_restore(
    app: AppHandle,
    id: String,
    confirmed: bool,
    overwrite: bool,
) -> Result<prune_core::archive::RestoreReport, String> {
    if !confirmed {
        return Err("恢复需要确认：请先预览再执行".into());
    }
    let _guard = ArchiveGuard::acquire()?;
    let cancel: &'static AtomicBool = &ARCHIVE_CANCEL;
    let sink = progress_sink(app);
    tauri::async_runtime::spawn_blocking(move || {
        let root = archive_root_or_err()?;
        let entry = prune_core::archive::find(&root, &id)
            .ok_or_else(|| "归档记录不存在（账本可能已被修改）".to_string())?;
        prune_core::archive::restore_guarded(&entry, overwrite, &sink, Some(cancel))
    })
    .await
    .map_err(|e| format!("归档线程异常：{e}"))?
}

/// 删除归档：归档文件走废纸篓 + 移除账本条目（归档区不是只进不出）
#[tauri::command]
pub async fn archive_delete(id: String, confirmed: bool) -> Result<(), String> {
    if !confirmed {
        return Err("删除归档需要确认".into());
    }
    let _guard = ArchiveGuard::acquire()?;
    tauri::async_runtime::spawn_blocking(move || {
        let root = archive_root_or_err()?;
        let entry = prune_core::archive::find(&root, &id)
            .ok_or_else(|| "归档记录不存在（账本可能已被修改）".to_string())?;
        prune_core::archive::delete(&entry, &root)
    })
    .await
    .map_err(|e| format!("归档线程异常：{e}"))?
}

/// 取消当前归档/恢复。返回「是否真的有一个任务在跑」，前端据此决定文案。
#[tauri::command]
pub fn archive_cancel() -> bool {
    ARCHIVE_CANCEL.store(true, Ordering::SeqCst);
    ARCHIVE_BUSY.load(Ordering::SeqCst)
}

#[cfg(test)]
mod tests {
    use super::*;
    use prune_core::disk::whitelist::Level;

    fn item(path: &str, label: &str, bytes: u64) -> ReclaimItem {
        ReclaimItem {
            path: path.to_string(),
            label: label.to_string(),
            bytes,
            level: Level::L1,
        }
    }

    /// M21 口径回归：失败项不计，「执行前已不存在」的项也不计——只有真正被
    /// 移进废纸篓的条目才允许进入成绩单。
    #[test]
    fn cut_entries_counts_only_really_moved_items() {
        let items = vec![
            item("/c/a", "npm 缓存", 100),     // 真的移走了
            item("/c/b", "node_modules", 200), // 移动失败
            item("/c/c", "brew 缓存", 300),    // 报告过期，执行时已不存在
        ];
        let preexisting: HashSet<String> = ["/c/a", "/c/b"].iter().map(|s| s.to_string()).collect();
        let failed: HashSet<String> = ["/c/b"].iter().map(|s| s.to_string()).collect();

        let cuts = cut_entries(&items, &preexisting, &failed);
        assert_eq!(cuts, vec![("npm 缓存", 100)]);
    }

    /// 全部都没动过（报告整体过期）→ 一条都不记，不能靠 skipped 刷数字
    #[test]
    fn cut_entries_is_empty_when_nothing_existed() {
        let items = vec![item("/c/a", "npm 缓存", 100)];
        let cuts = cut_entries(&items, &HashSet::new(), &HashSet::new());
        assert!(cuts.is_empty());
    }
}
