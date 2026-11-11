//! M0 #2 spike：Tauri tray 面板定位验证。
//!
//! 验证点（docs/prune-tauri-technical-plan.md §6 / §10）：
//! 1. `tray.rect()` 能否取到菜单栏图标坐标（多屏 / 不同 macOS 版本）
//! 2. 坐标系：实测为**物理像素 + 主屏左上原点**（见 position_panel 注释），与 Tauri
//!    窗口定位同系，无需 Cocoa 翻转 —— 这是 M0 #2 的核心结论
//! 3. 取不到 rect 时的降级：屏幕右上角
//! 4. Accessory 模式（无 Dock 图标）+ 无边框透明面板 + 失焦隐藏
//!
//! 运行 `pnpm tauri dev` 后观察 stdout 的 `[m0-tray]` 日志，并目测面板落点。

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use tauri::menu::{CheckMenuItem, IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, PhysicalPosition, Position, Rect, WebviewWindow, WindowEvent};

#[cfg(target_os = "macos")]
use crate::popover;

use crate::sampler::MemState;
use crate::tray_icon::{self, IconTheme};
use prune_core::locale::Locale;
use prune_core::mem::PressureLevel;
use prune_core::pet::{PetMood, PetMoodMachine};

/// tray 图标唯一 ID
pub const TRAY_ID: &str = "prune-tray";
/// 面板窗口 label（与 tauri.conf.json windows 数组一致）
pub const PANEL_LABEL: &str = "panel";
/// 「CPU 动画」右键菜单项 ID（M6-T3）
pub const MENU_TOGGLE_CPU_ANIM: &str = "toggle-cpu-anim";
/// 「菜单栏数字」右键菜单项 ID（M13：网速 / 内存直接显示在图标旁）
pub const MENU_TOGGLE_MENU_NUMBERS: &str = "toggle-menubar-numbers";
/// 「检查更新」右键菜单项 ID（docs/updater-plan.md）
pub const MENU_CHECK_UPDATE: &str = "check-update";
/// 「顶部图标」子菜单 ID 与子项 ID 前缀（M11）
const MENU_ICON_THEME: &str = "tray-icon-theme";
const MENU_ICON_PREFIX: &str = "tray-icon:";
/// 设置键名（与 tray_icon 共用 settings.json，只改本键；路径见 crate::settings）
const SETTING_CPU_ANIM: &str = "trayCpuAnim";
/// 「菜单栏数字」开关的持久化键（M13，与其它模块共用 settings.json，只改本键）
const SETTING_MENU_NUMBERS: &str = "trayMenuBarNumbers";
/// LAST_FRAME 的「静态原图」哨兵值
const FRAME_STATIC: usize = usize::MAX;
/// LAST_FRAME 的「主题已换、必须重发」哨兵值
const FRAME_DIRTY: usize = usize::MAX - 1;

/// CPU 动画开关（M6-T3）：默认开，启动时从 settings.json 加载
static CPU_ANIM_ENABLED: AtomicBool = AtomicBool::new(true);
/// 菜单栏数字开关（M13）：**默认关**，启动时从 settings.json 加载。
///
/// 默认关的原因是实测结论（2026-11-05）：完整标题 `↓2.1M↑180K·12G` 会让
/// 状态项宽到 186pt，在已经排满的菜单栏里被 macOS 判成放不下而隐藏
/// （`onScreen=false`，表现为「图标先出现再消失」）。默认开等于默认踩雷，
/// 需要的人自己开，且开启后走 [`compose_numbers`] 的分档降级。
///
/// Windows 上 set_title 不生效，开关仅影响 tooltip 是否带数字
static MENU_NUMBERS_ENABLED: AtomicBool = AtomicBool::new(false);
/// 菜单栏标题的网速缓存（down/up B/s）；None = 网速轮还没跑出第一拍
static TITLE_NET: Mutex<Option<(f64, f64)>> = Mutex::new(None);
/// 菜单栏标题的内存缓存（已用字节）；None = 快照轮还没跑出第一拍 / 口径未知
static TITLE_MEM: Mutex<Option<u64>> = Mutex::new(None);
/// 当前顶部图标主题（M11），启动时从 settings.json 加载
static TRAY_THEME: AtomicU8 = AtomicU8::new(0);
/// 宠物心情（M20）：watcher 每拍更新，动画循环逐拍读。Leisure=0
static MOOD: AtomicU8 = AtomicU8::new(0);
/// 最近设置的帧索引；相同帧跳过 set（往返节拍里 f1/f2 会出现两次，省 ~30% 主线程跳转）
static LAST_FRAME: AtomicUsize = AtomicUsize::new(FRAME_STATIC);
/// 面板尺寸（与 tauri.conf.json 一致，popover 内容尺寸同源）
///
/// 只有 macOS 的 NSPopover 需要显式尺寸（Windows 由窗口配置决定），
/// 因此加 cfg 门，避免 Windows 侧出现「定义未使用」告警。
///
/// 高度 432 是内容自然高 426 + 6 余量（M16 v5 贴高 + v6 拆方向行）：
/// p-2 16 + 边框 2 + header 44 + 内容区 335（pt-1 4 + 六行 48×6 +
/// 入口 31 + pb-3 12）+ footer 29。面板只有一屏统计、没有 tab，多余高度
/// 全是空白，所以贴合内容而不是固定 560。
#[cfg(target_os = "macos")]
pub const PANEL_W: f64 = 380.0;
#[cfg(target_os = "macos")]
pub const PANEL_H: f64 = 432.0;
/// 面板与菜单栏图标的间距（逻辑像素）
const MARGIN: f64 = 8.0;

/// 面板「刚显示」的时刻（Unix 毫秒）。用于 blur 宽限期：
/// Windows 上点击托盘时焦点先落在任务栏（explorer），show + set_focus 之后
/// 可能仍收到一次迟到的 Focused(false)，把刚弹出的面板立刻藏掉。
const BLUR_GRACE_MS: u64 = 300;
static PANEL_SHOWN_AT_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// 双击判定窗口（毫秒）。
///
/// macOS 上 Tauri **不派发** `TrayIconEvent::DoubleClick`（该变体注明 Windows Only），
/// 所以双击只能由「两次左键 Up 的间隔」自行判定。350 ms 的取值权衡：
/// 系统默认双击阈值约 300 ms（用户在设置里可放宽到 900+），宁可窄一点也不误判——
/// 用户「连点两下想重开面板」是真实高频动作，误判成双击会莫名弹出大窗口。
const DOUBLE_CLICK_WINDOW_MS: u64 = 350;
/// 最近一次左键 Up 的时刻（Unix 毫秒，0 = 没有上一次）
static LAST_LEFT_CLICK_MS: AtomicU64 = AtomicU64::new(0);
/// 最近一次「双击进主窗口」的时刻，用于 Windows 的 Click×2 + DoubleClick 去重
static LAST_MAIN_OPEN_MS: AtomicU64 = AtomicU64::new(0);

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 两次事件是否落在同一时间窗内（可测纯函数）。
///
/// `prev_ms == 0` 表示「没有上一次」，恒 false —— 让「应用刚启动的第一次点击」
/// 不会被 `now - 0` 的巨值意外判成同窗。时钟回拨时 `saturating_sub` 得 0，
/// 结果为 true（保守方向：最多多认一次双击）。
fn within_window(prev_ms: u64, now_ms: u64, window_ms: u64) -> bool {
    prev_ms != 0 && now_ms.saturating_sub(prev_ms) < window_ms
}

/// 右键菜单里「顶部图标」的勾选项句柄：切换主题后要把勾搬到新项上。
///
/// 菜单项本身是 Arc 包的内核句柄，克隆成本低；这里持有它只为了 `set_checked`。
///
/// `themes` 用 `RwLock` 是因为切语言要重建菜单：`manage` 只允许在 `create()` 里调
/// 一次（重复 `manage` 同类型在 Tauri 2 会覆盖，旧句柄悬空），所以重建只能替换
/// 内部这个 `Vec`。
struct TrayMenuState {
    themes: RwLock<Vec<(IconTheme, CheckMenuItem<tauri::Wry>)>>,
}

/// 托盘菜单文案。
///
/// 用结构体而不是 `t("tray.open_main")` 字符串查表：Rust 没有 TS 那种模板类型，
/// 字段名拼错只能运行时发现；结构体能保证漏字段编译失败。
pub struct TrayText {
    pub open_main: &'static str,
    pub check_update: &'static str,
    pub cpu_anim: &'static str,
    pub menu_numbers: &'static str,
    pub icon_theme: &'static str,
    pub quit: &'static str,
}

pub const TRAY_ZH: TrayText = TrayText {
    open_main: "打开主窗口",
    check_update: "检查更新",
    cpu_anim: "CPU 动画",
    menu_numbers: "菜单栏网速",
    icon_theme: "顶部图标",
    quit: "退出 Prune",
};

pub const TRAY_EN: TrayText = TrayText {
    open_main: "Open Main Window",
    check_update: "Check for Updates",
    cpu_anim: "CPU Animation",
    menu_numbers: "Menu Bar Speed",
    icon_theme: "Tray Icon",
    quit: "Quit Prune",
};

pub fn tray_text(l: Locale) -> &'static TrayText {
    match l {
        Locale::Zh => &TRAY_ZH,
        Locale::En => &TRAY_EN,
    }
}

/// 组装菜单（create 与 rebuild 共用，保证菜单项 id 逐字一致）。
///
/// **必须在主线程调用**：macOS 的 NSMenu 只在主线程安全。
fn build_menu(
    app: &AppHandle,
    locale: Locale,
) -> tauri::Result<(Menu<tauri::Wry>, Vec<(IconTheme, CheckMenuItem<tauri::Wry>)>)> {
    let text = tray_text(locale);
    // 勾选态从当前原子量读，不从 settings 重读 —— 本次会话可能改过但还没落盘
    let anim_on = CPU_ANIM_ENABLED.load(Ordering::Relaxed);
    let numbers_on = MENU_NUMBERS_ENABLED.load(Ordering::Relaxed);
    let theme = current_theme();

    let open_main = MenuItem::with_id(app, "open-main", text.open_main, true, None::<&str>)?;
    let check_update =
        MenuItem::with_id(app, MENU_CHECK_UPDATE, text.check_update, true, None::<&str>)?;
    let cpu_anim = CheckMenuItem::with_id(
        app,
        MENU_TOGGLE_CPU_ANIM,
        text.cpu_anim,
        true,
        anim_on,
        None::<&str>,
    )?;
    // 菜单栏网速：set_title 只有 macOS 生效，Windows 上置灰避免「点了没反应」
    let menu_numbers = CheckMenuItem::with_id(
        app,
        MENU_TOGGLE_MENU_NUMBERS,
        text.menu_numbers,
        cfg!(target_os = "macos"),
        numbers_on && cfg!(target_os = "macos"),
        None::<&str>,
    )?;

    // 顶部图标子菜单：一项一套皮肤，勾选态表达当前选择
    let mut theme_items: Vec<(IconTheme, CheckMenuItem<tauri::Wry>)> = Vec::new();
    for t in IconTheme::ALL {
        let item = CheckMenuItem::with_id(
            app,
            format!("{MENU_ICON_PREFIX}{}", t.id()),
            t.label(locale),
            true,
            t == theme,
            None::<&str>,
        )?;
        theme_items.push((t, item));
    }
    let theme_refs: Vec<&dyn IsMenuItem<tauri::Wry>> = theme_items
        .iter()
        .map(|(_, item)| item as &dyn IsMenuItem<tauri::Wry>)
        .collect();
    let icon_menu =
        Submenu::with_id_and_items(app, MENU_ICON_THEME, text.icon_theme, true, &theme_refs)?;

    let quit = MenuItem::with_id(app, "quit", text.quit, true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &open_main,
            &check_update,
            &PredefinedMenuItem::separator(app)?,
            &cpu_anim,
            &menu_numbers,
            &icon_menu,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;
    Ok((menu, theme_items))
}

/// 重建菜单（切语言时调用）。**必须在主线程执行**（见 [`build_menu`]）。
///
/// 失败只 log，不回滚语言：菜单错语言比「语言切换失败」轻得多。
pub fn rebuild_menu(app: &AppHandle) -> tauri::Result<()> {
    let locale = crate::i18n::current(app);
    let (menu, items) = build_menu(app, locale)?;
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        tray.set_menu(Some(menu))?;
    }
    // on_menu_event 绑在 tray 上（见 create），换菜单不需要重绑
    if let Some(state) = app.try_state::<TrayMenuState>() {
        *state.themes.write().unwrap_or_else(|e| e.into_inner()) = items;
    }
    eprintln!("[i18n] 托盘菜单已重建: {locale:?}");
    Ok(())
}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    // 开关持久化状态先行加载（决定菜单勾选态与动画行为）
    let anim_on = load_cpu_anim_setting(app);
    CPU_ANIM_ENABLED.store(anim_on, Ordering::Relaxed);
    // 菜单栏数字开关（M13）
    let numbers_on = load_menu_numbers_setting(app);
    MENU_NUMBERS_ENABLED.store(numbers_on, Ordering::Relaxed);
    // 顶部图标主题：决定 tray 初始图标与菜单勾选态（M11）
    let theme = tray_icon::load(app);
    TRAY_THEME.store(theme.as_u8(), Ordering::Relaxed);
    // create 里刚设置静态原图，与哨兵一致，避免动画首轮重复 set
    LAST_FRAME.store(FRAME_STATIC, Ordering::Relaxed);

    let locale = crate::i18n::current(app);
    let (menu, theme_items) = build_menu(app, locale)?;
    app.manage(TrayMenuState {
        themes: RwLock::new(theme_items),
    });

    let tray = TrayIconBuilder::with_id(TRAY_ID)
        .icon(theme.static_icon())
        .tooltip("Prune")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| {
            let id = event.id().as_ref();
            // 主题项走前缀分派，避免每加一套皮肤就往 match 里塞一条
            if let Some(raw) = id.strip_prefix(MENU_ICON_PREFIX) {
                if let Some(theme) = IconTheme::parse(raw) {
                    switch_theme(app, theme);
                }
                return;
            }
            match id {
                "open-main" => show_main(app),
                MENU_CHECK_UPDATE => {
                    // 异步检查，不阻塞菜单线程；结果经事件广播（见 updater 模块注释）
                    let handle = app.clone();
                    tauri::async_runtime::spawn(async move {
                        if let Err(e) = crate::updater::check_and_notify(&handle, true).await {
                            eprintln!("[updater] 托盘手动检查失败: {e}");
                        }
                    });
                }
                MENU_TOGGLE_CPU_ANIM => toggle_cpu_anim(app),
                MENU_TOGGLE_MENU_NUMBERS => toggle_menu_numbers(app),
                "quit" => app.exit(0),
                _ => {}
            }
        })
        .on_tray_icon_event(|tray, event| {
            let app = tray.app_handle();
            match event {
                // 左键：单击弹出/收起面板，双击直接进主窗口（右键交给菜单）
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } => {
                    let now = now_ms();
                    let prev = LAST_LEFT_CLICK_MS.swap(now, Ordering::Relaxed);
                    if within_window(prev, now, DOUBLE_CLICK_WINDOW_MS) {
                        // 第一击已经把面板弹出来了，这里先收起再开主窗口
                        // （清零时间戳，避免三连击被算成第二次双击）
                        LAST_LEFT_CLICK_MS.store(0, Ordering::Relaxed);
                        open_main_from_tray(app);
                    } else {
                        toggle_panel(app);
                    }
                }
                // Windows 会额外派发一个 DoubleClick（macOS 不会）：
                // 与上面的间隔判定重复触发时由 open_main_from_tray 内部去重
                TrayIconEvent::DoubleClick {
                    button: MouseButton::Left,
                    ..
                } => {
                    LAST_LEFT_CLICK_MS.store(0, Ordering::Relaxed);
                    open_main_from_tray(app);
                }
                _ => {}
            }
        })
        .build(app)?;
    // 模板图标是 macOS 专属机制：黑图 + 系统反色。所有主题都是单色字形，
    // 走 is_template() 统一决策；哪天加回彩色皮肤必须显式传 false，
    // 否则彩色会被系统当 alpha 掩码反色成黑白剪影。
    tray.set_icon_as_template(theme.is_template()).ok();
    eprintln!(
        "[m13-tray] ✅ tray 已创建: theme={}（{} 帧）, 菜单栏数字={}, locale={:?}",
        theme.id(),
        theme.frames().len(),
        MENU_NUMBERS_ENABLED.load(Ordering::Relaxed),
        locale
    );
    Ok(())
}

/// 当前生效的图标主题（动画循环每拍读一次，切皮肤立即生效）
fn current_theme() -> IconTheme {
    IconTheme::from_u8(TRAY_THEME.load(Ordering::Relaxed))
}

/// 当前宠物心情（M20）。watcher 每拍更新，动画 / tooltip 随时读。
fn current_mood() -> PetMood {
    PetMood::from_u8(MOOD.load(Ordering::Relaxed))
}

/// 切换顶部图标：落盘 → 立即重发静态图 → 搬勾选态
fn switch_theme(app: &AppHandle, theme: IconTheme) {
    if current_theme() != theme {
        TRAY_THEME.store(theme.as_u8(), Ordering::Relaxed);
        if let Err(e) = tray_icon::save(app, theme) {
            eprintln!("[m11-tray] ⚠️ 主题持久化失败（本次会话内仍生效）: {e}");
        }
        // 预置成非法值，让 set_frame 跳过「同帧不重发」的快路径，
        // 真正把新主题的静态图下发一次（否则图标停在旧皮肤上）
        LAST_FRAME.store(FRAME_DIRTY, Ordering::Relaxed);
        set_frame(app, None);
        eprintln!(
            "[m11-tray] 顶部图标已切换: {}（{}）",
            theme.label(crate::i18n::current(app)),
            theme.id()
        );
    }
    sync_checks(app, theme);
}

fn sync_checks(app: &AppHandle, active: IconTheme) {
    let Some(state) = app.try_state::<TrayMenuState>() else {
        return;
    };
    let themes = state.themes.read().unwrap_or_else(|e| e.into_inner());
    for (theme, item) in themes.iter() {
        let _ = item.set_checked(*theme == active);
    }
}

// ---- M6 · CPU 动画（docs/m6-tray-anim-plan.md）----
// 帧资源随主题走（见 tray_icon.rs）：剪刀是黑色模板帧，鲸鱼是彩色游泳帧。

/// 档位映射（M21 重标定；M22 六改按小猫 12 帧周期重算）→ **每帧**停留毫秒数。
///
/// M11 起不再有「静止档」：皮的表意是「一直在动」（小猫前跃），
/// 停住不动的图标等于一张静态头像，看不出 CPU 忙闲。CPU 未知时也保持
/// 慢速跳跃（480 ms/帧），只比最闲档慢一档。
///
/// 旧版（M6）800→95 ms 只有 8 倍，且中段各档只差 1.2~1.4 倍——人眼对
/// <1.5 倍的节奏差不敏感，15%~60% 区间看起来一样快。M21 重标定拉开档差；
/// M22 六改帧数 8 → 12 后按 8/12 缩放各档并略收紧，**一轮时长基本不变但
/// 帧率提高 1.5 倍**（满载约 23 fps）：闲 480 → 满载 43 ms（约 11 倍），
/// 相邻档 ≈1.3~1.5 倍递减。一轮时长 = 帧数 × 本值：小猫最闲 5.8 s 一跃
/// （悠然），满载 0.52 s 一跃（连续扑跳，且比 8 帧时顺滑得多）。
fn anim_beat_ms(usage: Option<f32>) -> u64 {
    match usage {
        None => 480, // CPU 未知：慢速但仍在跳
        Some(u) if u < 5.0 => 480,
        Some(u) if u < 15.0 => 375,
        Some(u) if u < 30.0 => 255,
        Some(u) if u < 45.0 => 175,
        Some(u) if u < 60.0 => 120,
        Some(u) if u < 75.0 => 78,
        Some(u) if u < 88.0 => 52,
        Some(_) => 43,
    }
}

/// 从共享采样状态读整机 CPU（T0：不新增 IPC、不新增事件）
fn current_cpu_usage(app: &AppHandle) -> Option<f32> {
    let state = app.try_state::<Arc<MemState>>()?;
    let guard = state.snapshot.lock().unwrap_or_else(|e| e.into_inner());
    guard.as_ref().and_then(|s| s.cpu.as_ref()).map(|c| c.usage)
}

// ---- M21 · 动画专用新鲜 CPU 源 ----
//
// 动画读 MemState 的 10 s 均值有两个体感问题：一是滞后——开个重负载任务
// 要等最多 10 s 图标才加速，「因」和「果」断了就不直观；二是 10 s 窗口把
// 短促的尖峰负载抹平了。这里自建一个 1 Hz 的轻量 CPU 轮（sysinfo 只刷
// CPU，µs 级开销），EMA 平滑后放进原子量供动画逐帧读。
//
// tooltip 刻意不改：它显示的是与面板同源的 10 s 均值，两个通道各司其职
// ——图标给「此刻的体感」，tooltip 给「面板里的那个数」。

/// EMA 平滑后的整机 CPU（×100 定点）；`VALID=false` 时不可信（首轮基准未建立）
static FRESH_CPU: AtomicU32 = AtomicU32::new(0);
static FRESH_CPU_VALID: AtomicBool = AtomicBool::new(false);

/// 读取动画用的新鲜 CPU；watcher 未就绪时退回 10 s 均值（宁可旧也不停摆）
fn fresh_cpu_usage(app: &AppHandle) -> Option<f32> {
    if FRESH_CPU_VALID.load(Ordering::Relaxed) {
        return Some(FRESH_CPU.load(Ordering::Relaxed) as f32 / 100.0);
    }
    current_cpu_usage(app)
}

/// 动画专用 CPU watcher：1 Hz 一拍，EMA 平滑（α≈0.4）。
/// 独立 `sysinfo::System` 只刷 CPU——与采样主循环完全解耦，
/// 这里挂了不影响采样，采样忙不影响图标的响应速度。
///
/// M20 起兼任**宠物状态观察**：同一拍把 CPU EMA + swap + 内核压力 + 磁盘余量
/// 喂给 `PetMoodMachine`，结果存 `MOOD` 供动画循环换帧集。swap / 压力是子进程
/// / sysctl 读数，2 s 一采（缓存值每拍都喂，避免观察机的退出计时被打断）；
/// 磁盘 statvfs µs 级，10 s 一采。
pub async fn run_cpu_watch() {
    let mut sys = sysinfo::System::new();
    // 首次 refresh 只是建基准（global_cpu_usage 此时恒 0），不能当读数用
    sys.refresh_cpu_usage();
    let mut machine = PetMoodMachine::new();
    let mut tick: u64 = 0;
    // 慢变量的最近读数缓存（None = 还没采到 / 平台无此口径）
    let mut last_swap_ratio: Option<f32> = None;
    let mut last_pressure: Option<PressureLevel> = None;
    let mut last_disk_free: Option<f32> = None;
    loop {
        tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
        sys.refresh_cpu_usage();
        let u = sys.global_cpu_usage();
        // 整数 EMA：ema = 0.4·new + 0.6·ema（×100 定点，四舍五入）
        let new_x100 = (u * 100.0).round() as u32;
        let old = FRESH_CPU.load(Ordering::Relaxed);
        let ema = if FRESH_CPU_VALID.load(Ordering::Relaxed) {
            (40 * new_x100 + 60 * old + 50) / 100
        } else {
            new_x100 // 首个有效样本直接落位
        };
        FRESH_CPU.store(ema, Ordering::Relaxed);
        FRESH_CPU_VALID.store(true, Ordering::Relaxed);

        // ── 宠物状态观察（M20）──
        tick += 1;
        if tick % 2 == 0 {
            // swap：sysctl 子进程，2 s 一次（与灵动岛 MemMonitor 同频率）
            if let Some((total, used, _)) = prune_core::swap::read_swapusage() {
                last_swap_ratio = (total > 0)
                    .then(|| used as f32 / total as f32 * 100.0)
                    .or(last_swap_ratio);
            }
            if let Some(p) = prune_core::mem::system::read_pressure() {
                last_pressure = Some(p.level);
            }
        }
        if tick % 10 == 0 {
            // 磁盘：statvfs µs 级，10 s 一次足够（慢性信号）
            if let Some(v) = prune_core::disk::volume_info(&prune_core::paths::home_dir_or_root()) {
                if v.total_bytes > 0 {
                    last_disk_free = Some(v.user_available_bytes as f32 / v.total_bytes as f32);
                }
            }
        }
        let mood = machine.observe(
            now_ms(),
            Some(ema as f32 / 100.0),
            last_swap_ratio,
            last_pressure,
            last_disk_free,
        );
        let prev = MOOD.swap(mood.as_u8(), Ordering::Relaxed);
        if prev != mood.as_u8() {
            eprintln!(
                "[m20-pet] 心情变化: {:?} → {:?}",
                PetMood::from_u8(prev),
                mood
            );
        }
    }
}

/// 换帧。macOS 必须用组合 API 原子换帧——先 `set_icon` 再
/// `set_icon_as_template` 会渲染两次导致可见闪烁（tauri 文档明示）；
/// 其他平台该 API 内部自动降级为 `set_icon`。
///
/// 第二个参数（是否模板）必须跟随主题：彩色图传 true 会变黑白剪影（M11）；
/// 当前两套主题在 macOS 上都是模板图，参数恒由 is_template() 给出。
fn set_icon_frame(
    tray: &tauri::tray::TrayIcon,
    icon: tauri::image::Image<'static>,
    as_template: bool,
) -> tauri::Result<()> {
    tray.set_icon_with_as_template(Some(icon), as_template)
}

/// 当前心情下应使用的帧集（M20）：Cat 皮 + Pant/Alert → 心情帧集；
/// 其余组合 → 主题帧集。剪刀皮永远走自己的帧（它不是宠物）。
fn frames_for(theme: IconTheme, mood: PetMood) -> &'static [tauri::image::Image<'static>] {
    if theme == IconTheme::Cat && matches!(mood, PetMood::Pant | PetMood::Alert) {
        tray_icon::mood_frames(mood)
    } else {
        theme.frames()
    }
}

/// 心情帧集的节奏（M20）：喘气 ~250 ms/帧（1 s 一次呼吸循环），
/// 炸毛 480 ms/帧（原地警戒的缓慢起伏）。悠闲 / 狂奔不走这里（CPU 节奏）。
fn mood_beat_ms(mood: PetMood) -> u64 {
    match mood {
        PetMood::Pant => 250,
        PetMood::Alert => 480,
        _ => 480,
    }
}

/// 切到指定帧（None = 回静态原图）。失败只记日志，不 panic、不中断循环；
/// 相同帧跳过 set。
fn set_frame(app: &AppHandle, frame: Option<usize>) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    let theme = current_theme();
    let mood = current_mood();
    let target = frame.unwrap_or(FRAME_STATIC);
    if LAST_FRAME.load(Ordering::Relaxed) == target {
        return;
    }
    // 切主题 / 换心情的瞬间 beats 还指向上一个帧集，越界时静默跳过这一帧，
    // 下一拍 current 已经变了，不会卡在坏状态上
    let icon = match frame {
        Some(i) => match frames_for(theme, mood).get(i) {
            Some(img) => img.clone(),
            None => return,
        },
        None => theme.static_icon(),
    };
    match set_icon_frame(&tray, icon, theme.is_template()) {
        Ok(()) => LAST_FRAME.store(target, Ordering::Relaxed),
        Err(e) => eprintln!("[m6-tray] 换帧失败（不中断动画）: {e}"),
    }
}

/// CPU 动画循环（T2）：tokio spawn 于 setup，与采样循环解耦——
/// 采样失败不影响图标，图标失败不影响采样（方案 §4.2 约束 4）。
///
/// 播放顺序来自主题的 [`IconTheme::beats`]：剪刀 6 拍往返开合，小猫 6 拍
/// 前跃循环（抬前脚 → 腾空前扑 → 落地回位）。节奏按 CPU 逐帧重算 —— CPU 来自
/// M21 的 1 Hz EMA watcher（`fresh_cpu_usage`），最快 1 s 跟手；逐帧读与
/// 逐轮读等价，但切换主题/开关时能立刻跟上，不用等一轮跑完。
pub async fn run_anim(app: AppHandle) {
    let mut last_mood = current_mood();
    loop {
        if !CPU_ANIM_ENABLED.load(Ordering::Relaxed) {
            set_frame(&app, None); // 关闭：回静态原图
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            continue;
        }
        // M20：心情变了必须强制重发——同索引指向的是另一套帧的图，
        // 「同帧跳过」的快路径会把新心情卡在旧画面上
        let mood = current_mood();
        if mood != last_mood {
            last_mood = mood;
            LAST_FRAME.store(FRAME_DIRTY, Ordering::Relaxed);
        }
        let theme = current_theme();
        let mood_frames = theme == IconTheme::Cat && matches!(mood, PetMood::Pant | PetMood::Alert);
        let beats: &[usize] = if mood_frames {
            tray_icon::mood_beats(mood)
        } else {
            theme.beats()
        };
        for &beat in beats {
            // 关掉动画、换皮肤或换心情：立刻中断本轮，重新取帧集
            if !CPU_ANIM_ENABLED.load(Ordering::Relaxed)
                || current_theme() != theme
                || current_mood() != mood
            {
                break;
            }
            set_frame(&app, Some(beat));
            let ms = if mood_frames {
                mood_beat_ms(mood)
            } else {
                anim_beat_ms(fresh_cpu_usage(&app))
            };
            tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
        }
    }
}

/// 每个快照轮调用（fast_round 发出事件后）：tooltip 升级为 `Prune · CPU 12%`。
/// 动画关闭时数字仍然更新——图标给量感，tooltip 给精确值，两通道各司其职。
///
/// M13-T5 起 tooltip 承载**完整**读数（下载 + 上传 + 内存）：菜单栏标题为了不
/// 被系统隐藏，只放得下 `↓2.1M` 一个指标，剩下的在悬停时补齐。
pub fn update_tooltip(app: &AppHandle, usage: Option<f32>) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    let mut tip = match usage {
        Some(u) => format!("Prune · CPU {u:.0}%"),
        None => "Prune".to_string(),
    };
    // 心情词（M20）：只对猫皮生效——剪刀不是宠物，没有心情可标。
    // 悠闲不标（正常态不值得占字符），与菜单栏数字的宽度纪律同源。
    if current_theme() == IconTheme::Cat {
        if let Some(word) = current_mood().label(crate::i18n::current(app)) {
            tip.push_str(&format!(" · {word}"));
        }
    }
    if MENU_NUMBERS_ENABLED.load(Ordering::Relaxed) {
        if let Some(full) = cached_numbers_full() {
            tip.push_str(&full);
        }
    }
    if let Err(e) = tray.set_tooltip(Some(tip)) {
        eprintln!("[m6-tray] tooltip 更新失败: {e}");
    }
}

/// tooltip 的数字段：**完整**读数，不受菜单栏宽度预算限制
/// （菜单栏标题只放得下 `↓2.1M`，上传与内存在这里补齐）
fn cached_numbers_full() -> Option<String> {
    compose_full(
        *TITLE_NET.lock().unwrap_or_else(|e| e.into_inner()),
        *TITLE_MEM.lock().unwrap_or_else(|e| e.into_inner()),
    )
}

// ---- M13 · 菜单栏数字（网速 / 内存）----

/// 速率格式化（纯函数）：B/s → `0K` / `9.8K` / `180K` / `2.1M` / `12M`。
/// <1 KB/s 显示 `0K`（一秒几字节没有表意价值）；<10 一位小数、其余取整。
pub(crate) fn format_speed(bps: f64) -> String {
    if bps < 1024.0 {
        return "0K".to_string();
    }
    let k = bps / 1024.0;
    if k < 1024.0 {
        return trim_one_decimal(k, "K");
    }
    trim_one_decimal(k / 1024.0, "M")
}

/// 内存格式化（纯函数）：字节 → `12G` / `7.4G`。≥10 G 取整，<10 G 一位小数。
pub(crate) fn format_mem_gb(bytes: u64) -> String {
    let g = bytes as f64 / (1024.0 * 1024.0 * 1024.0);
    if g >= 10.0 {
        format!("{:.0}G", g)
    } else {
        trim_one_decimal(g, "G")
    }
}

/// 一位小数格式化并去掉无意义的 `.0`（9.0 → `9`，9.8 → `9.8`）。
///
/// ≥100 时强制取整（123.4M → `123M`）：菜单栏标题的宽度是逐个字符算的，
/// 多一位小数就多约 10pt，足以把状态项顶出菜单栏（M13-T5 实测）。
fn trim_one_decimal(v: f64, unit: &str) -> String {
    if v >= 100.0 {
        return format!("{v:.0}{unit}");
    }
    let s = format!("{v:.1}{unit}");
    if let Some(stripped) = s.strip_suffix(&format!(".0{unit}")) {
        return format!("{stripped}{unit}");
    }
    s
}

/// 菜单栏标题的**字符预算**（M13-T5 · B）。
///
/// 实测（2026-11-05，1710pt 宽的 13" 屏，菜单栏已排满）：
/// - 图标本体 34pt，标题每字符约 **10.5pt**
/// - 20 字符（` ↓2.1M ↑180K · 12G`，186pt）必被隐藏
/// - 5 字符（`↓2.1M`，87pt）稳定可见；9 字符（`↓2.1M·12G`，116pt）被隐藏
///
/// 所以预算取 **6 字符**（约 95pt）：只保留下载速度这一个指标，
/// 上传与内存改由 tooltip 承载。原因很实际 —— 双指标最少也要 9 字符，
/// 在满员菜单栏里会「有时可见、有时被隐藏」，视觉上是闪烁，比没有更糟。
const TITLE_BUDGET_CHARS: usize = 6;

/// 由两路缓存组装数字后缀（纯函数），**带宽度分档**（M13-T5 · B）。
///
/// 候选按「信息量优先」排序，取第一个不超 [`TITLE_BUDGET_CHARS`] 的档；
/// 全都超预算就退回最短的一档（有数字总比一片空白强）。
///
/// | 档 | 示例 | 字符数 | 说明 |
/// |---|---|---|---|
/// | 完整 | `↓2.1M↑180K·12G` | 15 | 去掉了 M13 原设计的空格，省约 40pt |
/// | 去上传 | `↓2.1M·12G` | 10 | 上传日常最不重要，先砍它 |
/// | 仅网速 | `↓2.1M↑180K` | 12 | 内存读数缺失/被砍 |
/// | 仅下载 | `↓2.1M` | 6 | 兜底 |
/// | 仅内存 | `12G` | 3 | 网速轮没读出数时的唯一选择 |
fn compose_numbers(net: Option<(f64, f64)>, mem: Option<u64>) -> Option<String> {
    let down = net.map(|(d, _)| format_speed(d));
    let up = net.map(|(_, u)| format_speed(u));
    let mem = mem.map(format_mem_gb);

    let mut candidates: Vec<String> = Vec::with_capacity(5);
    if let (Some(d), Some(u), Some(m)) = (&down, &up, &mem) {
        candidates.push(format!("↓{d}↑{u}·{m}"));
        candidates.push(format!("↓{d}·{m}"));
        candidates.push(format!("↓{d}↑{u}"));
        candidates.push(format!("↓{d}"));
        candidates.push(m.clone());
    } else if let (Some(d), Some(u)) = (&down, &up) {
        candidates.push(format!("↓{d}↑{u}"));
        candidates.push(format!("↓{d}"));
    } else if let Some(m) = &mem {
        candidates.push(m.clone());
    }
    if candidates.is_empty() {
        return None;
    }
    let fit = candidates
        .iter()
        .find(|c| c.chars().count() <= TITLE_BUDGET_CHARS)
        .or_else(|| candidates.iter().min_by_key(|c| c.chars().count()))?;
    Some(fit.clone())
}

/// 完整读数（纯函数），给 tooltip 用：` · ↓2.1M↑180K · 12G`。
/// 与 [`compose_numbers`] 的区别是**不做宽度降档** —— tooltip 不受菜单栏空间限制。
fn compose_full(net: Option<(f64, f64)>, mem: Option<u64>) -> Option<String> {
    let mut parts: Vec<String> = Vec::with_capacity(2);
    if let Some((down, up)) = net {
        parts.push(format!("↓{}↑{}", format_speed(down), format_speed(up)));
    }
    if let Some(bytes) = mem {
        parts.push(format_mem_gb(bytes));
    }
    if parts.is_empty() {
        return None;
    }
    Some(format!(" · {}", parts.join(" · ")))
}

/// 当前缓存下的数字后缀（菜单栏标题用，带宽度降档）
fn numbers_suffix() -> Option<String> {
    compose_numbers(
        *TITLE_NET.lock().unwrap_or_else(|e| e.into_inner()),
        *TITLE_MEM.lock().unwrap_or_else(|e| e.into_inner()),
    )
}

/// 网速轮每拍调用：更新缓存 → 重刷标题
pub(crate) fn update_title_network(app: &AppHandle, bps: Option<(f64, f64)>) {
    *TITLE_NET.lock().unwrap_or_else(|e| e.into_inner()) = bps;
    refresh_title(app);
}

/// 快照轮（10 s）调用：更新内存缓存 → 重刷标题。0 视为「口径未知」（旧快照
/// 反序列化缺字段时为 0，见 MemSnapshot::physical_used_bytes 的 serde default）
pub(crate) fn update_title_mem(app: &AppHandle, used_bytes: Option<u64>) {
    let used = used_bytes.filter(|b| *b > 0);
    *TITLE_MEM.lock().unwrap_or_else(|e| e.into_inner()) = used;
    refresh_title(app);
}

/// 重刷菜单栏标题。开关关闭 → 清空；macOS 写 set_title，
/// 其它平台 set_title 无效，数字改走 tooltip（见 update_tooltip）。
fn refresh_title(app: &AppHandle) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    let title = if MENU_NUMBERS_ENABLED.load(Ordering::Relaxed) {
        numbers_suffix()
    } else {
        None
    };
    if let Err(e) = tray.set_title(title) {
        eprintln!("[m13-tray] 标题更新失败: {e}");
    }
    // tooltip 与标题同步刷新：菜单栏只放得下下载速度，上传/内存在悬停里补齐。
    // 全平台统一走这条路径（Windows 上 set_title 不渲染，tooltip 就是唯一出口）
    update_tooltip(app, current_cpu_usage(app));
}

/// 读「菜单栏数字」开关；文件不存在/缺键/解析失败一律默认**关**（M13-T5，见
/// [`MENU_NUMBERS_ENABLED`] 的注释：默认开会在满员菜单栏里被系统隐藏）
fn load_menu_numbers_setting(app: &AppHandle) -> bool {
    crate::settings::get_bool(app, SETTING_MENU_NUMBERS).unwrap_or(false)
}

/// 只改本键，保留 settings.json 里其它模块的键
fn save_menu_numbers_setting(app: &AppHandle, enabled: bool) -> Result<(), String> {
    crate::settings::set(app, SETTING_MENU_NUMBERS, serde_json::Value::Bool(enabled))
}

fn toggle_menu_numbers(app: &AppHandle) {
    let next = !MENU_NUMBERS_ENABLED.load(Ordering::Relaxed);
    MENU_NUMBERS_ENABLED.store(next, Ordering::Relaxed);
    if let Err(e) = save_menu_numbers_setting(app, next) {
        eprintln!("[m13-tray] ⚠️ 设置持久化失败（本次会话内仍生效）: {e}");
    }
    refresh_title(app);
    eprintln!(
        "[m13-tray] 菜单栏数字已{}",
        if next { "开启" } else { "关闭" }
    );
}

// ---- M6-T3 · 开关持久化 ----

/// 读开关状态；文件不存在/缺键/解析失败一律默认开
fn load_cpu_anim_setting(app: &AppHandle) -> bool {
    crate::settings::get_bool(app, SETTING_CPU_ANIM).unwrap_or(true)
}

/// 只改本键，保留 settings.json 里其它模块的键
fn save_cpu_anim_setting(app: &AppHandle, enabled: bool) -> Result<(), String> {
    crate::settings::set(app, SETTING_CPU_ANIM, serde_json::Value::Bool(enabled))
}

fn toggle_cpu_anim(app: &AppHandle) {
    let next = !CPU_ANIM_ENABLED.load(Ordering::Relaxed);
    CPU_ANIM_ENABLED.store(next, Ordering::Relaxed);
    if let Err(e) = save_cpu_anim_setting(app, next) {
        eprintln!("[m6-tray] ⚠️ 设置持久化失败（本次会话内仍生效）: {e}");
    }
    if next {
        eprintln!("[m6-tray] CPU 动画已开启");
    } else {
        // 立即回静态原图，不等循环醒来；LAST_FRAME 不预置哨兵，
        // 让 set_frame 真正执行一次 set（当前停在动画帧上）
        set_frame(app, None);
        eprintln!("[m6-tray] CPU 动画已关闭");
    }
}

/// 面板可见则隐藏，否则定位并显示。
/// M3 起优先走 NSPopover（原生箭头 + Transient 收起），初始化失败降级旧窗口路径。
fn toggle_panel(app: &AppHandle) {
    let Some(panel) = app.get_webview_window(PANEL_LABEL) else {
        eprintln!("[m0-tray] ❌ 找不到 panel 窗口");
        return;
    };
    // popover 的 ObjC 调用必须在主线程（tray 事件线程不保证）
    #[cfg(target_os = "macos")]
    {
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || {
            popover::toggle(&handle, &panel);
        });
    }
    #[cfg(not(target_os = "macos"))]
    legacy_toggle(app, &panel);
}

/// 双击图标的落点：收起面板 + 打开主窗口。
///
/// 去重的必要性来自平台差异 —— Windows 一次双击会连发 `Click`×2 与 `DoubleClick`，
/// 若不去重会在 350 ms 内重复定位 + focus 主窗口。macOS 只有 `Click`×2，
/// 由 [`within_window`] 的间隔判定认双击。
fn open_main_from_tray(app: &AppHandle) {
    let now = now_ms();
    let prev = LAST_MAIN_OPEN_MS.swap(now, Ordering::Relaxed);
    if within_window(prev, now, DOUBLE_CLICK_WINDOW_MS) {
        return;
    }
    close_panel(app);
    show_main(app);
}

/// 收起面板（不做 toggle 语义）：双击进主窗口前清掉第一击弹出的那一层。
fn close_panel(app: &AppHandle) {
    let Some(panel) = app.get_webview_window(PANEL_LABEL) else {
        return;
    };
    #[cfg(target_os = "macos")]
    {
        let _ = app.run_on_main_thread(move || {
            popover::hide(&panel);
        });
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = panel.hide();
    }
}

/// 降级路径（M0 spike 原实现）：手动定位无边框窗口
pub(crate) fn legacy_toggle(app: &AppHandle, panel: &WebviewWindow) {
    if panel.is_visible().unwrap_or(false) {
        let _ = panel.hide();
        return;
    }
    position_panel(app, panel);
    PANEL_SHOWN_AT_MS.store(now_ms(), std::sync::atomic::Ordering::Release);
    let _ = panel.show();
    let _ = panel.set_focus();
}

/// 将面板定位到 tray 图标正下方（右对齐），失败则降级到屏幕右上角
///
/// 实测结论（2026-10-23，macOS + 外接屏 2x）：
///   tray.rect() = position(2066, 0) size(68, 76)
///   → y=0 且尺寸为偶数大值，说明是**物理像素 + 主屏左上原点**，
///     既不是 Cocoa 左下原点，也不是逻辑点。Tauri 窗口定位同为物理像素、
///     左上原点、全局坐标系，因此可以直接用物理值算，无需翻转。
fn position_panel(app: &AppHandle, panel: &WebviewWindow) {
    let panel_logical_w = panel
        .outer_size()
        .map(|s| s.to_logical::<f64>(panel.scale_factor().unwrap_or(1.0)))
        .map(|s| s.width)
        .unwrap_or(380.0);

    let raw_rect: Option<Rect> = app
        .tray_by_id(TRAY_ID)
        .and_then(|t| t.rect().ok().flatten());

    let Some(rect) = raw_rect else {
        eprintln!("[m0-tray] ⚠️ tray.rect() 返回 None —— 命中降级路径（屏幕右上角）");
        fallback_top_right(app, panel, panel_logical_w);
        return;
    };

    let (rx, ry) = pos_xy(&rect.position);
    let (rw, rh) = size_wh(&rect.size);
    eprintln!("[m0-tray] tray.rect() 原始值: position=({rx}, {ry}) size=({rw}, {rh})");

    // 找到 tray 图标所在的屏幕（多屏时决定 scale 与边界钳制）
    let monitors = app.available_monitors().unwrap_or_default();
    let host = monitors.iter().find(|m| {
        let p = m.position();
        let s = m.size();
        let (mx, my) = (p.x as f64, p.y as f64);
        rx >= mx && rx <= mx + s.width as f64 && ry >= my && ry <= my + s.height as f64
    });

    let scale = host
        .map(|m| m.scale_factor())
        .unwrap_or_else(|| panel.scale_factor().unwrap_or(1.0));

    // rect 若为逻辑点位（当前 macOS 未出现，保留兜底），先换算成物理像素
    let (rx, ry, rw, rh) = match rect.position {
        Position::Logical(_) => (rx * scale, ry * scale, rw * scale, rh * scale),
        Position::Physical(_) => (rx, ry, rw, rh),
    };

    let panel_w = panel_logical_w * scale;
    let margin = MARGIN * scale;
    let mut x = rx + rw - panel_w - margin; // 右对齐图标右缘
    let mut y = ry + rh + margin; // 紧贴图标下方

    if let Some(m) = host {
        let (mx, my) = (m.position().x as f64, m.position().y as f64);
        let (mw, _mh) = (m.size().width as f64, m.size().height as f64);
        x = x.max(mx + margin).min(mx + mw - panel_w - margin);
        y = y.max(my + margin);
    }

    eprintln!(
        "[m0-tray] 换算（物理像素/左上原点, scale {scale}）: panel=({x:.0}, {y:.0}) 尺寸 {panel_w:.0}px 宽"
    );
    let _ = panel.set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32));
}

/// 降级路径：定位到屏幕右上角（主屏，菜单栏下方）
fn fallback_top_right(app: &AppHandle, panel: &WebviewWindow, panel_w: f64) {
    let Some(primary) = app.primary_monitor().ok().flatten() else {
        return;
    };
    let scale = primary.scale_factor();
    let screen_w_logical = primary.size().width as f64 / scale;
    let x = screen_w_logical - panel_w - MARGIN;
    let y = MARGIN * 4.0; // 粗略落在菜单栏下方
    eprintln!("[m0-tray] 降级定位: panel=({x:.0}, {y:.0})（屏幕右上角）");
    let _ = panel.set_position(PhysicalPosition::new(
        (x * scale) as i32,
        (y * scale) as i32,
    ));
}

fn pos_xy(p: &Position) -> (f64, f64) {
    match p {
        Position::Physical(v) => (v.x as f64, v.y as f64),
        Position::Logical(v) => (v.x, v.y),
    }
}

fn size_wh(s: &tauri::Size) -> (f64, f64) {
    match s {
        tauri::Size::Physical(v) => (v.width as f64, v.height as f64),
        tauri::Size::Logical(v) => (v.width, v.height),
    }
}

/// 打开主窗口（唯一入口：面板按钮 / tray 菜单都走这里）。
///
/// 关键点：Accessory 应用程序化 `show()` 不会触发 Space 切换，主窗口会停在
/// 启动时的 Space 上——用户在当前 Space 看到的就是「点了没反应」。
/// 因此打开前先把窗口居中定位到菜单栏图标所在屏幕，配合 setup 时的
/// `MoveToActiveSpace` collection behavior，保证出现在用户眼前。
pub fn show_main(app: &AppHandle) {
    let Some(main) = app.get_webview_window("main") else {
        eprintln!("[m0-tray] ❌ 找不到 main 窗口");
        return;
    };
    position_main(app, &main);
    let _ = main.show();
    let _ = main.set_focus();
}

/// 把主窗口定位到菜单栏图标所在屏幕的居中位置（逻辑同 position_panel 的坐标系换算）
fn position_main(app: &AppHandle, main: &WebviewWindow) {
    let Some(rect) = app
        .tray_by_id(TRAY_ID)
        .and_then(|t| t.rect().ok().flatten())
    else {
        eprintln!("[m0-tray] ⚠️ tray.rect() 不可用，主窗口保持原位置");
        return;
    };
    let (rx, ry) = pos_xy(&rect.position);
    let (rw, _rh) = size_wh(&rect.size);

    let monitors = app.available_monitors().unwrap_or_default();
    let host = monitors.iter().find(|m| {
        let p = m.position();
        let s = m.size();
        let (mx, my) = (p.x as f64, p.y as f64);
        rx >= mx && rx <= mx + s.width as f64 && ry >= my && ry <= my + s.height as f64
    });
    let Some(host) = host else {
        eprintln!("[m0-tray] ⚠️ 未找到 tray 图标所在屏幕，主窗口保持原位置");
        return;
    };

    let scale = host.scale_factor();
    let (rx, rw) = match rect.position {
        Position::Physical(_) => (rx, rw),
        Position::Logical(_) => (rx * scale, rw * scale),
    };

    let (mw, mh) = match main.outer_size() {
        Ok(s) => (s.width as f64, s.height as f64),
        Err(_) => (960.0 * scale, 640.0 * scale),
    };

    let (mx, my) = {
        let p = host.position();
        (p.x as f64, p.y as f64)
    };
    let (sw, sh) = {
        let s = host.size();
        (s.width as f64, s.height as f64)
    };

    // 水平居中于图标所在屏幕，垂直居中，边界钳制不越屏
    let x = (rx + rw / 2.0 - mw / 2.0).max(mx).min(mx + sw - mw);
    let y = (my + sh / 2.0 - mh / 2.0).max(my);

    eprintln!("[m0-tray] 主窗口定位: ({x:.0}, {y:.0}) scale {scale}（tray 所在屏幕）");
    let _ = main.set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32));
}

/// 在 setup 中挂上「面板失焦 → 隐藏」。
///
/// 带 `BLUR_GRACE_MS` 宽限期：显示后一小段时间内的失焦视为平台焦点竞态
/// （Windows 任务栏抢焦点），忽略之；真实点击别处的失焦仍立即隐藏。
pub fn attach_blur_to_hide(panel: &WebviewWindow) {
    let handle = panel.clone();
    panel.on_window_event(move |event| {
        if let WindowEvent::Focused(false) = event {
            let shown_at = PANEL_SHOWN_AT_MS.load(std::sync::atomic::Ordering::Acquire);
            if shown_at > 0 && now_ms().saturating_sub(shown_at) < BLUR_GRACE_MS {
                return;
            }
            let _ = handle.hide();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{anim_beat_ms, compose_numbers, format_mem_gb, format_speed};

    /// CPU 越高吃得越快：档位必须严格递减，且首尾差 ≥10 倍（M21 重标定后
    /// 实际 ~15 倍）。差太小的话菜单栏里只是一团在抖，分不出「闲」和「忙」。
    #[test]
    fn eating_gets_faster_as_cpu_rises() {
        let mut prev = u64::MAX;
        for u in [5.0f32, 20.0, 40.0, 55.0, 70.0, 80.0, 95.0] {
            let ms = anim_beat_ms(Some(u));
            assert!(ms < prev, "CPU {u}% 的帧间隔没有变快：{ms} ≥ {prev}");
            prev = ms;
        }
        assert!(
            anim_beat_ms(Some(0.0)) >= anim_beat_ms(Some(99.0)) * 10,
            "满载与空闲的节奏差不足 10 倍"
        );
    }

    /// 小猫皮的表意就是「在跳」，CPU 未知也不能停成一张静态图
    #[test]
    fn unknown_cpu_keeps_chewing() {
        let ms = anim_beat_ms(None);
        assert!(ms > 0 && ms <= anim_beat_ms(Some(0.0)));
    }

    /// 速率格式化：<1 KB/s 归零；<10 一位小数；≥10 取整；.0 必须剥掉
    #[test]
    fn speed_formatting() {
        assert_eq!(format_speed(0.0), "0K");
        assert_eq!(format_speed(999.0), "0K");
        assert_eq!(format_speed(1024.0), "1K");
        assert_eq!(format_speed(1024.0 * 9.8), "9.8K");
        assert_eq!(format_speed(1024.0 * 180.0), "180K");
        assert_eq!(format_speed(1024.0 * 1024.0 * 2.1), "2.1M");
        assert_eq!(format_speed(1024.0 * 1024.0 * 12.0), "12M");
        assert_eq!(format_speed(1024.0 * 1024.0 * 1024.0 * 3.0), "3072M");
    }

    /// 内存格式化：≥10 G 取整，<10 G 一位小数并剥 .0
    #[test]
    fn mem_formatting() {
        let g = 1024.0 * 1024.0 * 1024.0;
        assert_eq!(format_mem_gb(0), "0G");
        assert_eq!(format_mem_gb((7.4 * g) as u64), "7.4G");
        assert_eq!(format_mem_gb((12.0 * g) as u64), "12G");
        assert_eq!(format_mem_gb((16.04 * g) as u64), "16G");
    }

    /// 标题组装：谁有读数显示谁；全都没有必须返回 None（清空标题而不是留 `·`）
    #[test]
    fn numbers_composition() {
        let g = 1024.0 * 1024.0 * 1024.0;
        let m = 1024.0 * 1024.0;
        // 双指标最短也要 9 字符，超预算 → 只留下载速度（菜单栏塞得下的那一档）
        assert_eq!(
            compose_numbers(Some((2.1 * m, 180.0 * 1024.0)), Some((12.4 * g) as u64)),
            Some("↓2.1M".to_string())
        );
        // 数值短的时候「下载 + 内存」刚好 6 字符：优先保内存、砍上传
        // （上传日常最不重要，且波动最大；内存更能说明「机器还剩多少余地」）
        assert_eq!(
            compose_numbers(Some((1.0 * m, 1024.0)), Some((2.0 * g) as u64)),
            Some("↓1M·2G".to_string())
        );
        assert_eq!(
            compose_numbers(None, Some((12.0 * g) as u64)),
            Some("12G".to_string())
        );
        assert_eq!(
            compose_numbers(Some((1024.0, 512.0)), None),
            Some("↓1K↑0K".to_string())
        );
        assert_eq!(compose_numbers(None, None), None);
    }

    /// 完整读数（tooltip 用）不受预算限制：上传与内存都要在
    #[test]
    fn full_numbers_keep_everything() {
        let g = 1024.0 * 1024.0 * 1024.0;
        let m = 1024.0 * 1024.0;
        assert_eq!(
            super::compose_full(Some((2.1 * m, 180.0 * 1024.0)), Some((12.4 * g) as u64)),
            Some(" · ↓2.1M↑180K · 12G".to_string())
        );
        assert_eq!(super::compose_full(None, None), None);
    }

    /// 宽度分档（M13-T5 · B）：数值再长也要落到预算内，
    /// 这是「图标先出现再消失」的防线 —— 186pt 宽的状态项会被系统隐藏
    #[test]
    fn numbers_fall_back_until_within_budget() {
        let m = 1024.0 * 1024.0;
        let g = 1024.0 * 1024.0 * 1024.0;
        // 完整 16 字符、去上传 11 字符都超预算 → 只剩下载
        // 同时验证 ≥100M 取整（123.4M → 123M），位数不再随读数漂移
        let title = compose_numbers(Some((123.4 * m, 56.7 * m)), Some((7.4 * g) as u64))
            .expect("有读数就必须有标题");
        assert_eq!(title, "↓123M");
        assert!(title.chars().count() <= super::TITLE_BUDGET_CHARS);
    }

    /// 双击判定（M25）：间隔窗内算同一次手势，没有「上一次」不算
    #[test]
    fn double_click_window() {
        let w = super::DOUBLE_CLICK_WINDOW_MS;
        // 典型双击：两击间隔 120 ms
        assert!(super::within_window(1000, 1120, w));
        // 手慢（系统把双击阈值调宽到 900 ms 的用户）仍会被认成两次单击 ——
        // 宁可多弹一次面板，也不要在 350 ms 之外误开主窗口
        assert!(!super::within_window(1000, 1000 + w, w));
        // 首次点击（无上一次）
        assert!(!super::within_window(0, 1_000_000, w));
        // 时钟回拨：saturating_sub → 0，保守认成同窗，不会 panic
        assert!(super::within_window(2000, 1000, w));
    }
}
