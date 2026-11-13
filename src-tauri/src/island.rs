//! 灵动岛窗口（M15）。见 docs/m15-dynamic-island-plan.md。
//!
//! 三条不可让步的约束（决定了这里的每一行）：
//! 1. **必须 NSPanel**：普通 NSWindow 无论 level 多高都盖不住全屏 App 的 Space；
//! 2. **窗口 380×220 开死、永不 resize**：动画只改内部形状，resize 会撕裂 + 裁切；
//! 3. **不抢焦点**：`canBecomeKey = false`，点击岛不切 Space、不打断当前 App。
//!
//! hover 用 15 Hz 轮询鼠标位置 + 动态切换 `ignoresMouseEvents`，而不是 `NSTrackingArea`。
//! 原因：窗口比形状大，透明区必须穿透（否则用户点不到下面的窗口）；而整窗设
//! `ignoresMouseEvents = true` 又会让岛自己也点不到。轮询是唯一能同时满足两条的路，
//! 代价是 ≤ 66 ms 延迟 —— 相对 300 ms 的展开延迟可以忽略，且读鼠标位置不需要任何权限。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager, WebviewUrl};
use tauri_nspanel::{tauri_panel, CollectionBehavior, ManagerExt, PanelBuilder, PanelLevel};

use prune_core::island::{
    CpuAlert, CpuMonitor, CpuThresholds, LanWatchAlert, LanWatchMonitor, MemAlert, MemMonitor,
    MemThresholds, MemTrigger, PortGuardAlert, PortGuardKind, PortGuardMonitor, RateLimiter,
    CPU_COOLDOWN_MS, MEM_COOLDOWN_MS,
};
use prune_core::mem::PressureLevel;
use prune_core::net::ports as net_ports;
use prune_core::swap;

use crate::island_geo::{self, Placement, ScreenGeom, WINDOW_H, WINDOW_W};
use crate::sampler::MemState;
use crate::settings;

/// 与 App.tsx 的窗口分流一致
pub const ISLAND_LABEL: &str = "island";

/// 菜单栏是 status level(25)，岛必须再高一层才不会被压住
const ISLAND_LEVEL: i32 = 26;
/// 15 Hz：够跟手，又不至于让一个「资源监控工具」自己烧 CPU
const HOVER_TICK_MS: u64 = 66;
/// 2 s 采样一次；60 s 窗口 = 30 个点
const CPU_TICK_SECS: u64 = 2;
/// 显示窗口后等多久再发事件：让 WKWebView 从降频状态恢复
const EVENT_AFTER_SHOW_MS: u64 = 120;

/// 配置键（settings.json 扁平键，与 trayCpuAnim 同风格）
const K_ENABLED: &str = "island.enabled";
const K_CPU_PCT: &str = "island.cpu_pct";
const K_CPU_SUSTAIN: &str = "island.cpu_sustain_sec";
const K_CPU_TOLERANCE: &str = "island.cpu_tolerance_pct";
const K_SWAP_PCT: &str = "island.swap_pct";
const K_MEM_SUSTAIN: &str = "island.mem_sustain_sec";
const K_PORT_GUARD: &str = "island.port_guard";
const K_LAN_WATCH: &str = "island.lan_watch";
const K_MAX_PER_HOUR: &str = "island.max_per_hour";

/// 字节换算：岛上的读数口径用 GiB / MiB（与活动监视器一致）
const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
const MIB: u64 = 1024 * 1024;

tauri_panel! {
    panel!(IslandPanel {
        config: {
            can_become_key_window: false,
            is_floating_panel: true
        }
    })
}

/// 岛是否在屏上。hover 轮询靠它决定要不要读鼠标。
static VISIBLE: AtomicBool = AtomicBool::new(false);
/// 当前形状是不是展开态。命中测试必须用它，否则鼠标在展开区移动会被判成「离开」。
static EXPANDED: AtomicBool = AtomicBool::new(false);

/// 落点缓存。**只在主线程写**（AppKit 的 `NSScreen` 要求），后台循环只读。
///
/// 为什么要缓存而不是每次现算：`hover_loop` / `cpu_loop` 都跑在 tokio 线程上，
/// 在那里调 `NSScreen::screens` 拿不到 `MainThreadMarker`，会静默返回空 vec ——
/// 表现就是「岛永远不出现」，且没有任何报错。
#[derive(Debug, Clone, Copy)]
struct CachedPlace {
    place: Placement,
}

static CACHED: Mutex<Option<CachedPlace>> = Mutex::new(None);

/// 岛前端是否已经挂好监听。
///
/// panel 在 App 启动时就创建并隐藏，WKWebView 的加载时机不可控；如果第一次提报
/// 刚好发生在监听器注册之前，`emit_to` 会**静默丢弃**（Tauri 不重放历史事件），
/// 表现为「窗口 show 了但什么都没有」。
static FRONTEND_READY: AtomicBool = AtomicBool::new(false);

/// 前端没就绪时暂存最近一次提报，等它握手后补发。
static PENDING: Mutex<Option<serde_json::Value>> = Mutex::new(None);

// ───────────────────────── 配置 ─────────────────────────

/// 岛的全部可调项。放内存里，**后台循环不读文件**：
/// CPU 循环 2 s 一拍，每拍读一次 settings.json 没必要，而且配置文件被外部
/// 改坏时会出现「读写抖动」；统一由 `island_settings_set` 更新内存 + 落盘。
#[derive(Debug, Clone, Copy, PartialEq)]
struct Cfg {
    enabled: bool,
    cpu: CpuThresholds,
    mem: MemThresholds,
    port_guard: bool,
    lan_watch: bool,
    max_per_hour: u32,
}

const DEFAULT_CFG: Cfg = Cfg {
    enabled: true,
    cpu: CpuThresholds {
        threshold_pct: 90.0,
        sustain_sec: 60,
        tolerance_pct: 90.0,
    },
    mem: MemThresholds {
        swap_pct: 80.0,
        sustain_sec: 30,
    },
    port_guard: true,
    lan_watch: true,
    max_per_hour: 6,
};

static CFG: Mutex<Cfg> = Mutex::new(DEFAULT_CFG);

fn read_cfg(app: &AppHandle) -> Cfg {
    Cfg {
        enabled: settings::get_bool(app, K_ENABLED).unwrap_or(DEFAULT_CFG.enabled),
        cpu: CpuThresholds {
            threshold_pct: settings::get_f64(app, K_CPU_PCT)
                .unwrap_or(DEFAULT_CFG.cpu.threshold_pct as f64) as f32,
            sustain_sec: settings::get_u64(app, K_CPU_SUSTAIN)
                .unwrap_or(DEFAULT_CFG.cpu.sustain_sec),
            tolerance_pct: settings::get_f64(app, K_CPU_TOLERANCE)
                .unwrap_or(DEFAULT_CFG.cpu.tolerance_pct as f64) as f32,
        }
        .clamped(),
        mem: MemThresholds {
            swap_pct: settings::get_f64(app, K_SWAP_PCT).unwrap_or(DEFAULT_CFG.mem.swap_pct as f64)
                as f32,
            sustain_sec: settings::get_u64(app, K_MEM_SUSTAIN)
                .unwrap_or(DEFAULT_CFG.mem.sustain_sec),
        }
        .clamped(),
        max_per_hour: settings::get_u64(app, K_MAX_PER_HOUR)
            .unwrap_or(DEFAULT_CFG.max_per_hour as u64)
            .clamp(1, 30) as u32,
        port_guard: settings::get_bool(app, K_PORT_GUARD).unwrap_or(DEFAULT_CFG.port_guard),
        lan_watch: settings::get_bool(app, K_LAN_WATCH).unwrap_or(DEFAULT_CFG.lan_watch),
    }
}

/// 启动时从磁盘灌一次内存缓存
pub fn load_cfg(app: &AppHandle) {
    let cfg = read_cfg(app);
    if let Ok(mut guard) = CFG.lock() {
        *guard = cfg;
    }
}

fn cfg() -> Cfg {
    CFG.lock().map(|g| *g).unwrap_or(DEFAULT_CFG)
}

// ───────────────────────── 几何 ─────────────────────────

/// 从 `NSScreen` 读所有屏的原始几何。必须在主线程调用（AppKit 约束）。
fn screen_geoms() -> Vec<ScreenGeom> {
    use objc2_app_kit::NSScreen;
    use objc2_foundation::MainThreadMarker;

    let Some(mtm) = MainThreadMarker::new() else {
        return Vec::new();
    };
    let screens = NSScreen::screens(mtm);
    // screens[0] 是带菜单栏的主屏；它决定 Cocoa 左下原点 → Tauri 左上原点的翻转基准
    let primary_h = screens
        .iter()
        .next()
        .map(|s| s.frame().size.height)
        .unwrap_or(0.0);

    screens
        .iter()
        .map(|s| {
            let f = s.frame();
            let insets = s.safeAreaInsets();
            // objc2 把 nullable 的 NSRect 返回映射成 CGRect 本体（无刘海时为零矩形），
            // 所以要用宽度判断「有没有这块辅助区」，不能当成 Option 去 map
            let aux_l = s.auxiliaryTopLeftArea();
            let aux_r = s.auxiliaryTopRightArea();
            ScreenGeom {
                frame_x: f.origin.x,
                frame_y: f.origin.y,
                frame_w: f.size.width,
                frame_h: f.size.height,
                safe_top: insets.top,
                aux_left_w: (aux_l.size.width > 0.0).then_some(aux_l.size.width),
                aux_right_w: (aux_r.size.width > 0.0).then_some(aux_r.size.width),
                primary_h,
            }
        })
        .collect()
}

/// 找一块带物理刘海的屏并算出落点。返回 `None` = 这台机器现在没有可用的岛位
/// （纯外接屏环境），调用方据此静默跳过，不造假刘海。
///
/// **只能主线程调**（内部读 `NSScreen`）。
fn resolve_placement() -> Option<CachedPlace> {
    screen_geoms()
        .iter()
        .filter_map(island_geo::place)
        .next()
        .map(|place| CachedPlace { place })
}

/// 重建落点缓存（主线程）。屏幕参数变化、岛创建时各调一次，幂等。
fn refresh_placement() -> Option<CachedPlace> {
    let cached = resolve_placement();
    if let Ok(mut guard) = CACHED.lock() {
        *guard = cached;
    }
    cached
}

fn cached_place() -> Option<CachedPlace> {
    CACHED.lock().ok().and_then(|g| *g)
}

// ───────────────────────── 窗口 ─────────────────────────

pub fn setup(app: &AppHandle) -> Result<(), String> {
    // 先把配置灌进内存缓存，再建窗口：否则第一拍 CPU 采样会用到默认值
    load_cfg(app);

    let Some(cached) = refresh_placement() else {
        eprintln!("[m15-island] 没有带刘海的屏幕，灵动岛不启用");
        return Ok(());
    };
    let place = cached.place;

    let panel = PanelBuilder::<_, IslandPanel>::new(app, ISLAND_LABEL)
        .url(WebviewUrl::App("index.html".into()))
        .title("Prune Island")
        .size(tauri::LogicalSize::new(WINDOW_W, WINDOW_H).into())
        .position(tauri::LogicalPosition::new(place.x, place.y).into())
        .level(PanelLevel::Custom(ISLAND_LEVEL))
        .collection_behavior(
            CollectionBehavior::new()
                // 三件套：所有 Space 都在、能盖全屏、不参与 Cmd+Tab 循环。
                // **不要加 stationary** —— 它表示「不参与 Space 切换」，会让窗口
                // 在当前活跃 Space 里不显示（排查过一轮）
                .can_join_all_spaces()
                .full_screen_auxiliary()
                .ignores_cycle(),
        )
        .transparent(true)
        // 透明窗口必须同时声明非不透明：只设 transparent 时 AppKit 仍按
        // opaque 合成，WebView 画的黑色区域可能整块不出现在屏幕上
        .opaque(false)
        .has_shadow(false)
        .resizable(false)
        .hides_on_deactivate(false)
        .with_window(|w| {
            // **这一行才是真透明的开关**。PanelBuilder::transparent 只作用于
            // NSPanel（clearColor + setOpaque:false），不碰 WKWebView；而底层
            // WebviewWindowBuilder 默认 transparent=false → wry 的
            // drawsBackground=YES → WebView 层自己画一块不透明底色，表现为
            // 「胶囊下面一大片空白」。必须在这里再传一次，WebviewWindowBuilder
            // 的 transparent 会同时设 window 和 webview 两层。
            w.transparent(true)
                .decorations(false)
                .visible(false)
                .skip_taskbar(true)
                .shadow(false)
        })
        .build()
        .map_err(|e| format!("创建 island panel 失败: {e:?}"))?;

    // 初始隐藏 + 全穿透。鼠标进入形状时才切换成可交互（见 hover_loop）
    panel.hide();
    panel.set_ignores_mouse_events(true);

    eprintln!(
        "[m15-island] 就位 x={:.1} y={:.1} 刘海 {:.0}×{:.0}",
        place.x, place.y, place.notch_w, place.notch_h
    );
    Ok(())
}

/// 显示岛。AppKit 调用全部丢到主线程，后台循环只管标记状态。
fn show_panel(app: &AppHandle) {
    if cached_place().is_none() {
        return;
    }
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        match handle.get_webview_panel(ISLAND_LABEL) {
            Ok(panel) => {
                // orderFrontRegardless 而不是 show()：App 是 Accessory 模式、通常
                // 不在前台，普通 orderFront 压不过当前活跃窗口
                panel.order_front_regardless();
                eprintln!("[m15-island] show visible={}", panel.is_visible());
            }
            Err(e) => eprintln!("[m15-island] show: panel 不可用 {e:?}"),
        }
    });
    VISIBLE.store(true, Ordering::Release);
}

fn hide_panel(app: &AppHandle) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Ok(panel) = handle.get_webview_panel(ISLAND_LABEL) {
            panel.hide();
            panel.set_ignores_mouse_events(true);
        }
    });
    VISIBLE.store(false, Ordering::Release);
    EXPANDED.store(false, Ordering::Release);
}

// ───────────────────────── 鼠标 ─────────────────────────

/// CoreGraphics 光标位置。
///
/// 用 CGEvent 而不是 `NSEvent.mouseLocation`：后者要求主线程，15 Hz 跨线程调度
/// 不划算；CGEvent 的坐标本身就是**全局左上原点**，与 Tauri 窗口坐标系一致，
/// 省掉一次翻转。读位置不需要任何权限。
mod cursor {
    use std::ffi::c_void;

    #[repr(C)]
    struct CGPoint {
        x: f64,
        y: f64,
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventCreate(source: *const c_void) -> *mut c_void;
        fn CGEventGetLocation(event: *mut c_void) -> CGPoint;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRelease(cf: *const c_void);
    }

    pub fn location() -> Option<(f64, f64)> {
        unsafe {
            let event = CGEventCreate(std::ptr::null());
            if event.is_null() {
                return None;
            }
            let p = CGEventGetLocation(event);
            CFRelease(event as *const c_void);
            Some((p.x, p.y))
        }
    }
}

/// 15 Hz 跟踪鼠标，进出形状时切换穿透并通知前端。
///
/// 只在岛可见时读鼠标 —— 岛大部分时间不在屏上，这条循环的常态开销是 0。
async fn hover_loop(app: AppHandle) {
    let mut tick = tokio::time::interval(Duration::from_millis(HOVER_TICK_MS));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut inside = false;

    loop {
        tick.tick().await;
        if !VISIBLE.load(Ordering::Acquire) {
            inside = false;
            continue;
        }
        let Some(place) = cached_place().map(|c| c.place) else {
            continue;
        };
        let Some((mx, my)) = cursor::location() else {
            continue;
        };

        // CGEvent 与 Tauri 同为左上原点，窗口本地坐标 = 全局 − 窗口原点
        let lx = mx - place.x;
        let ly = my - place.y;
        let (sx, sy, sw, sh) = island_geo::shape_rect(EXPANDED.load(Ordering::Acquire));
        let hit = lx >= sx && lx <= sx + sw && ly >= sy && ly <= sy + sh;

        if hit == inside {
            continue;
        }
        inside = hit;

        let handle = app.clone();
        let _ = app.run_on_main_thread(move || {
            if let Ok(panel) = handle.get_webview_panel(ISLAND_LABEL) {
                // 形状之外整窗穿透：否则 380×220 的透明区会拦住下面窗口的点击
                panel.set_ignores_mouse_events(!hit);
            }
        });
        let _ = app.emit("island://hover", hit);
    }
}

// ───────────────────────── CPU 采样与提报 ─────────────────────────

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 屏幕布局指纹：显示器个数 + 各自原点/尺寸。
///
/// 不监听 `NSApplicationDidChangeScreenParametersNotification` 是有意的：改分辨率
/// 时那个通知会连发十几下，而且回调跑在 ObjC 里（panic 即 abort）。2 s 一次的轮询
/// 天然把整串合并成一次；`available_monitors` 走 `CGDisplay`，**不要求主线程**，
/// 可以在采样线程上直接调。
fn screen_signature(app: &AppHandle) -> u64 {
    use std::hash::{Hash, Hasher};

    let Ok(monitors) = app.available_monitors() else {
        return 0;
    };
    let mut h = std::collections::hash_map::DefaultHasher::new();
    monitors.len().hash(&mut h);
    for m in &monitors {
        let p = m.position();
        let s = m.size();
        (p.x, p.y, s.width, s.height).hash(&mut h);
    }
    h.finish()
}

/// 屏幕布局变了：重算落点，落点挪了就把窗口也挪过去。
///
/// 整段必须在主线程跑（读 `NSScreen`）。移动用 Tauri 的 `set_position`，不用
/// `setFrameOrigin` —— 后者是裸 ObjC 调用，出错就是 abort。
fn on_screen_changed(app: &AppHandle) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        let before = cached_place();
        let after = refresh_placement();
        let (Some(before), Some(after)) = (before, after) else {
            return;
        };
        if (before.place.x - after.place.x).abs() < 0.5
            && (before.place.y - after.place.y).abs() < 0.5
        {
            return;
        }
        let Some(win) = handle.get_webview_window(ISLAND_LABEL) else {
            eprintln!("[m15-island] 屏幕变化后找不到 island 窗口，跳过挪位");
            return;
        };
        if let Err(e) = win.set_position(tauri::LogicalPosition::new(after.place.x, after.place.y))
        {
            eprintln!("[m15-island] 屏幕变化后挪位失败: {e}");
        }
    });
}

/// 现在最烧 CPU 的几个进程。读 `MemState` 的快照（10 s 轮更新），
/// 不另外起进程枚举 —— 岛只是「指路」，细节在主窗口里看。
fn top_processes(app: &AppHandle, n: usize) -> Vec<serde_json::Value> {
    let Some(state) = app.try_state::<Arc<MemState>>() else {
        return Vec::new();
    };
    let guard = match state.processes.lock() {
        Ok(g) => g,
        Err(e) => e.into_inner(),
    };
    let mut ranked: Vec<(String, f32, i32)> = guard
        .iter()
        .filter_map(|p| p.cpu.map(|c| (p.name.clone(), c, p.pid)))
        .collect();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    ranked
        .into_iter()
        .take(n)
        .map(|(name, cpu, pid)| serde_json::json!({ "name": name, "cpu": cpu, "pid": pid }))
        .collect()
}

fn fire_cpu(app: &AppHandle, alert: CpuAlert) {
    let Some(cached) = cached_place() else {
        return;
    };
    let payload = serde_json::json!({
        "kind": "cpu",
        "avg": alert.avg_pct,
        "peak": alert.peak_pct,
        "ratio": alert.ratio_pct,
        "samples": alert.samples,
        // 刘海高度随机型变化，前端不能写死 33：内容必须从挖孔之下开始
        "notchH": cached.place.notch_h,
        "top": top_processes(app, 3),
    });
    deliver(app, payload);
}

/// 内存事件的展开态素材：换出速率 / 压缩器 / Top 3 换出受害者。
/// 全部来自 10 s 快照（`MemState.snapshot`），读不到就发 null —— UI 按「未知」
/// 隐藏，不要编 0（0 会被读成「确认无换出」）。
fn mem_snapshot_bits(app: &AppHandle) -> (Option<f64>, Option<u64>, Vec<serde_json::Value>) {
    let Some(state) = app.try_state::<Arc<MemState>>() else {
        return (None, None, Vec::new());
    };
    let guard = match state.snapshot.lock() {
        Ok(g) => g,
        Err(e) => e.into_inner(),
    };
    let Some(snap) = guard.as_ref() else {
        return (None, None, Vec::new());
    };
    let thrash = snap.thrash.map(|t| t.swapouts_per_sec);
    let compressor_mb = snap.compressor_bytes.map(|b| b / MIB);
    // top_swapped 已按 swapped 降序排好（mem/mod.rs 的组装逻辑），直接取前三
    let top = snap
        .top_swapped
        .iter()
        .take(3)
        .map(|p| {
            serde_json::json!({
                "name": p.name,
                "pid": p.pid,
                "swappedMb": p.swapped.map(|b| b / MIB),
                "totalMb": p.total() / MIB,
            })
        })
        .collect();
    (thrash, compressor_mb, top)
}

fn fire_mem(app: &AppHandle, alert: MemAlert, swap_used: Option<u64>) {
    let Some(cached) = cached_place() else {
        return;
    };
    let pressure = match alert.pressure {
        PressureLevel::Normal => "normal",
        PressureLevel::Warning => "warning",
        PressureLevel::Critical => "critical",
    };
    let trigger = match alert.trigger {
        MemTrigger::Pressure => "pressure",
        MemTrigger::Swap => "swap",
        MemTrigger::Both => "both",
    };
    let (swapouts_per_sec, compressor_mb, top) = mem_snapshot_bits(app);
    let payload = serde_json::json!({
        "kind": "mem",
        "pressure": pressure,
        "trigger": trigger,
        "swapUsedGb": swap_used
            .map(|b| ((b as f64 / GIB) * 10.0).round() / 10.0),
        "swapRatioPct": alert.swap_ratio_pct,
        "swapoutsPerSec": swapouts_per_sec,
        "compressorMb": compressor_mb,
        "notchH": cached.place.notch_h,
        "top": top,
    });
    deliver(app, payload);
}

/// 端口事件的 payload 组装 + 投递（M19）。判定结论全部在核心层算好，
/// 这里只做枚举 → 字符串的翻译与格式转换。
fn fire_port(app: &AppHandle, alert: PortGuardAlert) {
    let Some(cached) = cached_place() else {
        return;
    };
    let exposure = match alert.exposure {
        prune_core::net::BindScope::Any => "any",
        prune_core::net::BindScope::Interface => "interface",
        prune_core::net::BindScope::Loopback => "loopback",
        prune_core::net::BindScope::Unknown => "unknown",
    };
    let payload = serde_json::json!({
        "kind": "port",
        "port": alert.port,
        "comm": alert.comm,
        "pid": alert.pid,
        "projectLabel": alert.project_label,
        "exposure": exposure,
        "owners": alert.owners,
        "conflict": alert.kind == PortGuardKind::Conflict,
        "notchH": cached.place.notch_h,
    });
    deliver(app, payload);
}

/// 新面孔事件的 payload 组装 + 投递（M23）。判定结论全部在核心层算好，
/// 这里只做字段搬运与格式转换。
fn fire_lan(app: &AppHandle, alert: LanWatchAlert) {
    let Some(cached) = cached_place() else {
        return;
    };
    let payload = serde_json::json!({
        "kind": "lan",
        "ip": alert.ip,
        "label": alert.label,
        "devKind": alert.kind,
        "randomizedMac": alert.randomized_mac,
        "extra": alert.extra,
        "notchH": cached.place.notch_h,
    });
    deliver(app, payload);
}

/// 展示 + 广播。岛同时只显示一条：后到的 payload 直接覆盖前一条
/// （前端收到 island://alert 会重置相位机），这就是 §12.3 的「抢占」——
/// CPU 展示中内存触发、内存展示中 CPU 触发，都是后触发者覆盖前者。
fn deliver(app: &AppHandle, payload: serde_json::Value) {
    // 先显示，等一拍再发。窗口隐藏时 WKWebView 会被系统降频，事件可能在
    // 挂起期间被丢弃；显示后再发是最稳的顺序。
    show_panel(app);
    if FRONTEND_READY.load(Ordering::Acquire) {
        let handle = app.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(Duration::from_millis(EVENT_AFTER_SHOW_MS)).await;
            match handle.emit("island://alert", payload) {
                Ok(()) => eprintln!("[m15-island] alert 已广播"),
                Err(e) => eprintln!("[m15-island] alert 广播失败: {e}"),
            }
        });
    } else if let Ok(mut pending) = PENDING.lock() {
        // 前端还没挂监听：先存着，等 `island_ready` 补发（否则这次提报直接丢失）
        *pending = Some(payload);
    }
}

/// 频控三道闸（§12.4）。CPU / 内存各一闸 + 一道全局兜底闸：
/// 只拆 per-event 不拆全局的话，两类事件加起来能把岛刷成广告位；
/// 只留全局的话，CPU 报满额度后内存事件永远弹不出来。
/// 闸的判断顺序必须是「先查（would_allow）后记（allow）」——先记的话，
/// 全局闸拒绝时 per-event 闸的配额已被白吃一次。
#[derive(Debug)]
struct Gates {
    cpu: RateLimiter,
    mem: RateLimiter,
    global: RateLimiter,
}

impl Gates {
    fn new(max_per_hour: u32) -> Self {
        Self {
            cpu: RateLimiter::new(max_per_hour),
            mem: RateLimiter::new(max_per_hour),
            global: RateLimiter::new(max_per_hour),
        }
    }

    fn reconfigure(&mut self, max_per_hour: u32) {
        self.cpu.reconfigure(max_per_hour);
        self.mem.reconfigure(max_per_hour);
        self.global.reconfigure(max_per_hour);
    }

    /// 双闸放行检查（不消耗配额）。全部通过后调用方再逐闸 `allow` 记账。
    fn would_pass(&mut self, gate: fn(&mut Self) -> &mut RateLimiter, now: u64) -> bool {
        gate(self).would_allow(now) && self.global.would_allow(now)
    }

    fn record(&mut self, gate: fn(&mut Self) -> &mut RateLimiter, now: u64) {
        gate(self).allow(now);
        self.global.allow(now);
    }
}

fn cpu_gate(g: &mut Gates) -> &mut RateLimiter {
    &mut g.cpu
}

fn mem_gate(g: &mut Gates) -> &mut RateLimiter {
    &mut g.mem
}

/// 主采样循环：2 s 一拍，CPU 与内存两条判定链共用同一个节拍
/// （内存的压力 / swap 是 µs 级 sysctl，不值得另开循环）。
/// 屏幕布局轮询也搭在这拍上（见 `screen_signature`）。
async fn sample_loop(app: AppHandle) {
    let mut sys = sysinfo::System::new();
    // 首次 refresh 只是建基准，global_cpu_usage 此时是 0，不能当真实读数用
    sys.refresh_cpu_usage();
    tokio::time::sleep(Duration::from_secs(CPU_TICK_SECS)).await;

    let mut cpu_monitor = CpuMonitor::new();
    let mut mem_monitor = MemMonitor::new();
    let mut last_cfg = cfg();
    let mut gates = Gates::new(last_cfg.max_per_hour);
    let mut last_cpu_fire: u64 = 0;
    let mut last_mem_fire: u64 = 0;

    let mut tick = tokio::time::interval(Duration::from_secs(CPU_TICK_SECS));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    let mut last_screen = screen_signature(&app);

    loop {
        tick.tick().await;

        // 插拔外接屏 / 改分辨率后落点会失效，窗口会停在旧坐标上悬空
        let sig = screen_signature(&app);
        if sig != last_screen {
            last_screen = sig;
            on_screen_changed(&app);
        }

        let current = cfg();
        if current != last_cfg {
            // 任何配置变化都清判定状态：旧样本 / 旧计时是按旧阈值走的，沿用会误触发
            cpu_monitor.reset();
            mem_monitor.reset();
            gates.reconfigure(current.max_per_hour);
            // 用户刚改完阈值，不该被上一次触发的冷却挡住
            last_cpu_fire = 0;
            last_mem_fire = 0;
            last_cfg = current;
        }
        if !current.enabled {
            continue;
        }

        sys.refresh_cpu_usage();
        let cpu = sys.global_cpu_usage();
        let now = now_ms();

        // ── CPU 通道 ──
        if let Some(alert) = cpu_monitor.observe(now, cpu, &current.cpu) {
            // 冷却只防「降下去又升上来」的横跳，滞回防「持续高位的重复提报」
            let cooled = last_cpu_fire == 0 || now.saturating_sub(last_cpu_fire) >= CPU_COOLDOWN_MS;
            if cooled && gates.would_pass(cpu_gate, now) {
                gates.record(cpu_gate, now);
                last_cpu_fire = now;
                fire_cpu(&app, alert);
            }
        }

        // ── 内存通道 ──
        // 压力走 sysctl（None = 读不到，按 Normal 处理：不谎报，也不会卡住重武装）；
        // swap 走 vm.swapusage（None = 平台无此口径，比率传 -1 让通道 C 安静跳过）。
        let pressure = prune_core::mem::system::read_pressure().map(|p| p.level);
        let swap_usage = swap::read_swapusage();
        if pressure.is_some() || swap_usage.is_some() {
            let level = pressure.unwrap_or(PressureLevel::Normal);
            let ratio_pct = match swap_usage {
                Some((total, used, _)) if total > 0 => used as f32 / total as f32 * 100.0,
                _ => -1.0,
            };
            if let Some(alert) = mem_monitor.observe(now, level, ratio_pct, &current.mem) {
                let cooled =
                    last_mem_fire == 0 || now.saturating_sub(last_mem_fire) >= MEM_COOLDOWN_MS;
                if cooled && gates.would_pass(mem_gate, now) {
                    gates.record(mem_gate, now);
                    last_mem_fire = now;
                    let swap_used = swap_usage.map(|(_, used, _)| used);
                    fire_mem(&app, alert, swap_used);
                }
            }
        }
    }
}

/// 启动两个后台循环。由 `lib.rs` 的 setup 调用（在 panel 建好之后）。
pub fn spawn_loops(app: &AppHandle) {
    let hover = app.clone();
    tauri::async_runtime::spawn(async move { hover_loop(hover).await });
    let sample = app.clone();
    tauri::async_runtime::spawn(async move { sample_loop(sample).await });
    let guard = app.clone();
    tauri::async_runtime::spawn(async move { port_guard_loop(guard).await });
    let lan = app.clone();
    tauri::async_runtime::spawn(async move { lan_watch_loop(lan).await });
}

// ───────────────────────── 局域网新面孔（M23） ─────────────────────────

/// 守望节拍：60 s 一轮。数据源只有邻居表（arp / GetIpNetTable2）+ 默认网卡
/// 信息，~百毫秒级，**零主动探测、零 lsof**（M18 §1 的「不是扫描器」定位）。
/// 60 s × ~100 ms ≈ 0.17% 占空比，电池无感。
const LAN_WATCH_TICK_SECS: u64 = 60;
/// 新面孔事件独立预算上限：一天几条的量级，4/h 足够。
/// 实际预算取 `min(用户设置的每小时上限, 本值)` —— 设置项文案承诺的是
/// 「所有灵动岛事件的总量闸门」，独立预算不能反过来突破它。
const LAN_MAX_PER_HOUR: u32 = 4;

/// 端口事件的独立预算上限（6/h）；与 [`LAN_MAX_PER_HOUR`] 同样受用户上限收口。
const PORT_MAX_PER_HOUR: u32 = 6;

/// 独立预算 = min(用户上限, 事件自身的合理上限)。
///
/// 两道闸的语义：用户上限是**总量**闸门（跨越所有事件类型），事件自身的上限
/// 是**单类**闸门（防止一种事件占满总量）。取小值保证「用户说每小时最多 N 条」
/// 永远是硬上界，同时不会因为用户放宽到 30 就让端口事件泛滥。
fn event_budget(max_per_hour: u32, kind_cap: u32) -> u32 {
    max_per_hour.clamp(1, kind_cap)
}

/// 新面孔守望循环。判定在核心层（`LanWatchMonitor`：终身制已知集 +
/// 网段指纹重置），这里只管节拍、数据源与频控。
async fn lan_watch_loop(app: AppHandle) {
    let mut tick = tokio::time::interval(Duration::from_secs(LAN_WATCH_TICK_SECS));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    // 第一拍立即跑：尽快建基线（基线轮不弹，与端口保镖同纪律）

    let mut monitor = LanWatchMonitor::new();
    let mut last_cfg = cfg();
    let mut limiter = RateLimiter::new(event_budget(last_cfg.max_per_hour, LAN_MAX_PER_HOUR));
    loop {
        tick.tick().await;

        let current = cfg();
        if current != last_cfg {
            // 配置变化重建基线：重开开关的第一轮当作新起点，不补报中途错过的变化
            monitor.reset();
            limiter.reconfigure(event_budget(current.max_per_hour, LAN_MAX_PER_HOUR));
            last_cfg = current;
        }
        if !current.enabled || !current.lan_watch {
            continue;
        }

        let round = tauri::async_runtime::spawn_blocking(|| {
            use prune_core::net::lan as lan_mod;

            let Some(scope) = lan_mod::local_scope() else {
                return None; // 未联网 / 无默认路由 / 平台不支持：安静跳过本拍
            };
            let gateway = prune_core::net::diag::default_gateway()
                .and_then(|g| g.gateway.parse::<std::net::Ipv4Addr>().ok());
            let nbrs = lan_mod::neighbors();
            let mask: u32 = if scope.prefix == 0 {
                0
            } else {
                u32::MAX << (32 - scope.prefix as u32)
            };
            let network = u32::from(scope.addr) & mask;
            let cidr = format!("{}/{}", std::net::Ipv4Addr::from(network), scope.prefix);
            let devices = lan_mod::merge(
                &nbrs,
                &[],
                &[],
                &[],
                gateway,
                Some(scope.addr),
                Some((std::net::Ipv4Addr::from(network), scope.prefix)),
            );
            Some((cidr, devices))
        })
        .await
        .ok()
        .flatten();

        let Some((cidr, devices)) = round else {
            continue;
        };
        let now = now_ms();
        for alert in monitor.observe(Some(&cidr), &devices) {
            if limiter.allow(now) {
                fire_lan(&app, alert);
            } else {
                eprintln!("[m23-lan] 新面孔事件被频控拦截 ip={}", alert.ip);
            }
        }
    }
}

// ───────────────────────── 端口冲突保镖（M19） ─────────────────────────

/// 守望节拍：30 s 一轮。端口变化频率比内存低两个数量级（M7 §5.1 的结论），
/// 30 s 是「dev server 起来后多久想知道」与「常驻开销」的折中：
/// 热扫 ~50 ms / 30 s = 0.17% 占空比。
const PORT_GUARD_TICK_SECS: u64 = 30;
/// 端口 tab 的快照比这新鲜就直接复用（PORT_INTERVAL_SECS=30 的定时轮保证它最多旧 30 s，
/// 20 s 是保守线：拿到的快照肯定不比守望轮自己扫的旧）
const PORT_SNAPSHOT_FRESH_MS: u64 = 20_000;

/// 端口守望循环：关注端口空闲→被占、同端口多持有者 → 灵动岛提报。
///
/// 判定在核心层（`PortGuardMonitor`：基线 + 边沿检测 + 每端口冷却），
/// 这里只管节拍、快照来源与频控。三条纪律：
/// 1. 岛关着 / 端口保镖关着 → 整拍跳过，常驻开销 0；
/// 2. 端口 tab 活跃且快照新鲜 → 复用 PortState，不起第二轮 lsof；
/// 3. 自己扫时复用 PortState 的 ProjectCache（cwd 查询 20 ms/进程，缓存跨轮共享）。
async fn port_guard_loop(app: AppHandle) {
    let mut tick = tokio::time::interval(Duration::from_secs(PORT_GUARD_TICK_SECS));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    // 第一拍立即跑：尽快建基线（基线轮不弹，S3）。tokio 的首拍本身就是
    // 立刻到期的，所以循环外不要再 tick 一次——那会把首拍吃掉，基线推迟 30 s。

    let mut monitor = PortGuardMonitor::new();
    let mut last_cfg = cfg();
    let mut limiter = RateLimiter::new(event_budget(last_cfg.max_per_hour, PORT_MAX_PER_HOUR));

    loop {
        tick.tick().await;

        let current = cfg();
        if current != last_cfg {
            // 配置变化重建基线：重开开关的第一轮当作新起点，不补报中途错过的变化
            monitor.reset();
            limiter.reconfigure(event_budget(current.max_per_hour, PORT_MAX_PER_HOUR));
            last_cfg = current;
        }
        if !current.enabled || !current.port_guard {
            continue;
        }

        let snap = match port_snapshot_source(&app).await {
            Some(s) => s,
            None => continue,
        };
        let now = now_ms();
        let alerts = monitor.observe(now, &snap.entries);
        for alert in alerts {
            if limiter.allow(now) {
                fire_port(&app, alert);
            } else {
                // 预算吃满：放弃本条（冷却机制保证 10 分钟内同端口不会再攒出一条）
                eprintln!("[m19-port] 端口事件被频控拦截 port={}", alert.port);
            }
        }
    }
}

/// 取一轮端口聚合结果：优先复用端口 tab 的热快照，否则自己扫。
async fn port_snapshot_source(app: &AppHandle) -> Option<prune_core::net::PortSnapshot> {
    let state = app.try_state::<std::sync::Arc<crate::port_sampler::PortState>>()?;
    // 端口 tab 活跃时的定时轮 30 s 一轮：快照比 20 s 新，说明刚扫过，直接复用
    if let Some(age) = state.age() {
        if age.as_millis() as u64 <= PORT_SNAPSHOT_FRESH_MS {
            return state.snapshot();
        }
    }
    let state_clone: std::sync::Arc<crate::port_sampler::PortState> = state.inner().clone();
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        // Windows 的 iphlpapi 只给 pid：进程身份从内存轮的全量进程表借（与 port_round 同源）
        let identities: Option<std::collections::HashMap<i32, prune_core::net::service::ProcIdentity>> =
            handle
                .try_state::<std::sync::Arc<crate::sampler::MemState>>()
                .map(|ms| {
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
        let mut cache = state_clone
            .project_cache()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        net_ports::scan_cached(identities.as_ref(), &mut cache)
    })
    .await
    .ok()
}

// ───────────────────────── 命令 ─────────────────────────

/// 前端在展开/收起动画**开始前**调用，让命中测试立刻切到目标形状。
#[tauri::command]
pub fn island_set_expanded(expanded: bool) {
    EXPANDED.store(expanded, Ordering::Release);
}

#[tauri::command]
pub fn island_hide(app: AppHandle) {
    hide_panel(&app);
}

/// 前端挂好监听后握手。补发在握手之前发生的那次提报（最多一次）。
#[tauri::command]
pub fn island_ready(app: AppHandle) -> bool {
    FRONTEND_READY.store(true, Ordering::Release);
    eprintln!("[m15-island] 前端已就绪");
    let pending = PENDING.lock().ok().and_then(|mut g| g.take());
    match pending {
        Some(payload) => {
            eprintln!("[m15-island] 补发一条待处理提报");
            let _ = app.emit("island://alert", payload);
            true
        }
        None => false,
    }
}

/// 岛当前在这台机器上是否可用。设置面板据此显示「本机屏幕无刘海，灵动岛不启用」，
/// 而不是让用户对着一个永远不响的开关调阈值。
#[derive(Debug, Clone, Copy, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IslandStatus {
    pub available: bool,
    pub notch_w: f64,
    pub notch_h: f64,
}

#[tauri::command]
pub fn island_status() -> IslandStatus {
    match cached_place() {
        Some(c) => IslandStatus {
            available: true,
            notch_w: c.place.notch_w,
            notch_h: c.place.notch_h,
        },
        None => IslandStatus {
            available: false,
            notch_w: 0.0,
            notch_h: 0.0,
        },
    }
}

/// 前端设置面板读写用的完整配置。字段名走 camelCase，前端不用做命名转换。
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IslandSettings {
    pub enabled: bool,
    pub cpu_pct: f64,
    pub cpu_sustain_sec: u64,
    pub cpu_tolerance_pct: f64,
    pub swap_pct: f64,
    pub mem_sustain_sec: u64,
    pub port_guard: bool,
    pub lan_watch: bool,
    pub max_per_hour: u64,
}

#[tauri::command]
pub fn island_settings_get(app: AppHandle) -> IslandSettings {
    let c = read_cfg(&app);
    IslandSettings {
        enabled: c.enabled,
        cpu_pct: c.cpu.threshold_pct as f64,
        cpu_sustain_sec: c.cpu.sustain_sec,
        cpu_tolerance_pct: c.cpu.tolerance_pct as f64,
        swap_pct: c.mem.swap_pct as f64,
        mem_sustain_sec: c.mem.sustain_sec,
        port_guard: c.port_guard,
        lan_watch: c.lan_watch,
        max_per_hour: c.max_per_hour as u64,
    }
}

/// 写配置。**先钳制再落盘**，并把钳制后的值回给前端 —— 用户设 0 会被改成 60 并看见。
#[tauri::command]
pub fn island_settings_set(
    app: AppHandle,
    settings: IslandSettings,
) -> Result<IslandSettings, String> {
    let cpu = CpuThresholds {
        threshold_pct: settings.cpu_pct as f32,
        sustain_sec: settings.cpu_sustain_sec,
        tolerance_pct: settings.cpu_tolerance_pct as f32,
    }
    .clamped();
    // 钳制上限 95 是 swap 通道的「关闭位」（§12.6）
    let mem = MemThresholds {
        swap_pct: settings.swap_pct as f32,
        sustain_sec: settings.mem_sustain_sec,
    }
    .clamped();
    let max_per_hour = settings.max_per_hour.clamp(1, 30);

    settings::set_many(
        &app,
        &[
            (K_ENABLED, serde_json::json!(settings.enabled)),
            (K_CPU_PCT, serde_json::json!(cpu.threshold_pct)),
            (K_CPU_SUSTAIN, serde_json::json!(cpu.sustain_sec)),
            (K_CPU_TOLERANCE, serde_json::json!(cpu.tolerance_pct)),
            (K_SWAP_PCT, serde_json::json!(mem.swap_pct)),
            (K_MEM_SUSTAIN, serde_json::json!(mem.sustain_sec)),
            (K_PORT_GUARD, serde_json::json!(settings.port_guard)),
            (K_LAN_WATCH, serde_json::json!(settings.lan_watch)),
            (K_MAX_PER_HOUR, serde_json::json!(max_per_hour)),
        ],
    )?;

    // 内存缓存立刻生效；sample_loop 下一拍发现 cfg 变了会自动清空判定状态，
    // port_guard_loop 下一拍发现变了会重建端口基线
    if let Ok(mut guard) = CFG.lock() {
        *guard = Cfg {
            enabled: settings.enabled,
            cpu,
            mem,
            port_guard: settings.port_guard,
            lan_watch: settings.lan_watch,
            max_per_hour: max_per_hour as u32,
        };
    }

    Ok(IslandSettings {
        enabled: settings.enabled,
        cpu_pct: cpu.threshold_pct as f64,
        cpu_sustain_sec: cpu.sustain_sec,
        cpu_tolerance_pct: cpu.tolerance_pct as f64,
        swap_pct: mem.swap_pct as f64,
        mem_sustain_sec: mem.sustain_sec,
        port_guard: settings.port_guard,
        lan_watch: settings.lan_watch,
        max_per_hour,
    })
}
