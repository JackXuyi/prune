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

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{
    AppHandle, Manager, PhysicalPosition, Position, Rect, WebviewWindow, WindowEvent,
};

#[cfg(target_os = "macos")]
use crate::popover;

use crate::sampler::{MemState, FAST_INTERVAL_SECS};

/// tray 图标唯一 ID
pub const TRAY_ID: &str = "prune-tray";
/// 面板窗口 label（与 tauri.conf.json windows 数组一致）
pub const PANEL_LABEL: &str = "panel";
/// 「CPU 动画」右键菜单项 ID（M6-T3）
pub const MENU_TOGGLE_CPU_ANIM: &str = "toggle-cpu-anim";
/// 设置文件名与键名（config_dir()/settings.json，多模块共享，只改本键）
const SETTINGS_FILE: &str = "settings.json";
const SETTING_CPU_ANIM: &str = "trayCpuAnim";
/// LAST_FRAME 的「静态原图」哨兵值
const FRAME_STATIC: usize = usize::MAX;

/// CPU 动画开关（M6-T3）：默认开，启动时从 settings.json 加载
static CPU_ANIM_ENABLED: AtomicBool = AtomicBool::new(true);
/// 最近设置的帧索引；相同帧跳过 set（往返节拍里 f1/f2 会出现两次，省 ~30% 主线程跳转）
static LAST_FRAME: AtomicUsize = AtomicUsize::new(FRAME_STATIC);
/// 面板尺寸（与 tauri.conf.json 一致，popover 内容尺寸同源）
///
/// 只有 macOS 的 NSPopover 需要显式尺寸（Windows 由窗口配置决定），
/// 因此加 cfg 门，避免 Windows 侧出现「定义未使用」告警。
#[cfg(target_os = "macos")]
pub const PANEL_W: f64 = 380.0;
#[cfg(target_os = "macos")]
pub const PANEL_H: f64 = 560.0;
/// 面板与菜单栏图标的间距（逻辑像素）
const MARGIN: f64 = 8.0;

/// 面板「刚显示」的时刻（Unix 毫秒）。用于 blur 宽限期：
/// Windows 上点击托盘时焦点先落在任务栏（explorer），show + set_focus 之后
/// 可能仍收到一次迟到的 Focused(false)，把刚弹出的面板立刻藏掉。
const BLUR_GRACE_MS: u64 = 300;
static PANEL_SHOWN_AT_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    // 开关持久化状态先行加载（决定菜单勾选态与动画行为）
    let anim_on = load_cpu_anim_setting(app);
    CPU_ANIM_ENABLED.store(anim_on, Ordering::Relaxed);
    // create 里刚设置静态原图，与哨兵一致，避免动画首轮重复 set
    LAST_FRAME.store(FRAME_STATIC, Ordering::Relaxed);

    let open_main = MenuItem::with_id(app, "open-main", "打开主窗口", true, None::<&str>)?;
    let cpu_anim = CheckMenuItem::with_id(
        app,
        MENU_TOGGLE_CPU_ANIM,
        "CPU 动画",
        true,
        anim_on,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, "quit", "退出 Prune", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &open_main,
            &PredefinedMenuItem::separator(app)?,
            &cpu_anim,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;

    let tray = TrayIconBuilder::with_id(TRAY_ID)
        .icon(tray_icon())
        .tooltip("Prune")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open-main" => show_main(app),
            MENU_TOGGLE_CPU_ANIM => toggle_cpu_anim(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            // 左键单击：弹出/收起面板（右键交给菜单）
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                toggle_panel(tray.app_handle());
            }
        })
        .build(app)?;
    // 模板图标是 macOS 专属机制：黑图 + 系统反色。其他平台关掉（否则彩色
    // 图标被当模板处理再被忽略，行为依赖平台默认值），并显式留彩色原图。
    tray.set_icon_as_template(cfg!(target_os = "macos")).ok();
    Ok(())
}

/// 托盘图标：macOS 用黑色模板图（系统自动适配深浅色菜单栏）；
/// 其他平台用彩色应用图标（深色任务栏下黑色模板图不可见）。
/// 内嵌字节解析失败直接 panic——这是编译期打包产物，坏不了。
fn tray_icon() -> tauri::image::Image<'static> {
    #[cfg(target_os = "macos")]
    {
        tauri::image::Image::from_bytes(include_bytes!("../icons/tray.png"))
            .expect("内置 tray 图标损坏")
    }
    #[cfg(not(target_os = "macos"))]
    {
        tauri::image::Image::from_bytes(include_bytes!("../icons/32x32.png"))
            .expect("内置应用图标损坏")
    }
}

// ---- M6 · CPU 动画（docs/m6-tray-anim-plan.md）----

/// 动画帧（编译期内嵌，OnceLock 启动时解析一次）。
/// macOS 用 48×48 黑色模板帧，其他平台用 32×32 品牌色帧（W3 教训）。
fn anim_frames() -> &'static [tauri::image::Image<'static>; 4] {
    static FRAMES: std::sync::OnceLock<Box<[tauri::image::Image<'static>; 4]>> =
        std::sync::OnceLock::new();
    FRAMES.get_or_init(|| {
        #[cfg(target_os = "macos")]
        let bytes: [&[u8]; 4] = [
            include_bytes!("../icons/tray-anim/macos/f0.png"),
            include_bytes!("../icons/tray-anim/macos/f1.png"),
            include_bytes!("../icons/tray-anim/macos/f2.png"),
            include_bytes!("../icons/tray-anim/macos/f3.png"),
        ];
        #[cfg(not(target_os = "macos"))]
        let bytes: [&[u8]; 4] = [
            include_bytes!("../icons/tray-anim/win/f0.png"),
            include_bytes!("../icons/tray-anim/win/f1.png"),
            include_bytes!("../icons/tray-anim/win/f2.png"),
            include_bytes!("../icons/tray-anim/win/f3.png"),
        ];
        Box::new(std::array::from_fn(|i| {
            tauri::image::Image::from_bytes(bytes[i]).expect("内置托盘动画帧损坏")
        }))
    })
}

/// 档位映射（方案 §3.1，整机 CPU 近 10 s 均值）→ 帧间隔 ms。
/// None = 静止档：CPU 未知（**绝不拿未知值驱动动画**）或 <10%。
fn anim_beat_ms(usage: Option<f32>) -> Option<u64> {
    match usage {
        None => None,
        Some(u) if u < 10.0 => None,
        Some(u) if u < 40.0 => Some(500), // 2 fps · 慢
        Some(u) if u < 70.0 => Some(200), // 5 fps · 中
        Some(_) => Some(100),             // 10 fps · 快
    }
}

/// 从共享采样状态读整机 CPU（T0：不新增 IPC、不新增事件）
fn current_cpu_usage(app: &AppHandle) -> Option<f32> {
    let state = app.try_state::<Arc<MemState>>()?;
    let guard = state.snapshot.lock().unwrap_or_else(|e| e.into_inner());
    guard.as_ref().and_then(|s| s.cpu.as_ref()).map(|c| c.usage)
}

/// 换帧。macOS 必须用组合 API 原子换帧——先 `set_icon` 再
/// `set_icon_as_template` 会渲染两次导致可见闪烁（tauri 文档明示）；
/// 其他平台该 API 内部自动降级为 `set_icon`。
fn set_icon_frame(
    tray: &tauri::tray::TrayIcon,
    icon: tauri::image::Image<'static>,
) -> tauri::Result<()> {
    tray.set_icon_with_as_template(Some(icon), true)
}

/// 切到指定帧（None = 回静态原图）。失败只记日志，不 panic、不中断循环；
/// 相同帧跳过 set。
fn set_frame(app: &AppHandle, frame: Option<usize>) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else { return };
    let target = frame.unwrap_or(FRAME_STATIC);
    if LAST_FRAME.load(Ordering::Relaxed) == target {
        return;
    }
    let icon = match frame {
        Some(i) => anim_frames()[i].clone(),
        None => tray_icon(),
    };
    match set_icon_frame(&tray, icon) {
        Ok(()) => LAST_FRAME.store(target, Ordering::Relaxed),
        Err(e) => eprintln!("[m6-tray] 换帧失败（不中断动画）: {e}"),
    }
}

/// CPU 动画循环（T2）：tokio spawn 于 setup，与采样循环解耦——
/// 采样失败不影响图标，图标失败不影响采样（方案 §4.2 约束 4）。
///
/// 帧序列 f0→f1→f2→f3→f2→f1（6 拍往返）；每播完一个周期重读一次 CPU，
/// 周期内不换速，避免视觉撕裂（约束 5）。静止档只随 10 s 快照轮醒来，不空转。
pub async fn run_anim(app: AppHandle) {
    const CYCLE: [usize; 6] = [0, 1, 2, 3, 2, 1];
    loop {
        if !CPU_ANIM_ENABLED.load(Ordering::Relaxed) {
            set_frame(&app, None); // 关闭：回静态原图
            tokio::time::sleep(std::time::Duration::from_secs(FAST_INTERVAL_SECS)).await;
            continue;
        }
        match anim_beat_ms(current_cpu_usage(&app)) {
            None => {
                // 静止档：闭合帧 + 睡到下一个快照轮（验收 #1：无任何定时器空转）
                set_frame(&app, Some(0));
                tokio::time::sleep(std::time::Duration::from_secs(FAST_INTERVAL_SECS)).await;
            }
            Some(beat_ms) => {
                for &beat in &CYCLE {
                    if !CPU_ANIM_ENABLED.load(Ordering::Relaxed) {
                        break;
                    }
                    set_frame(&app, Some(beat));
                    tokio::time::sleep(std::time::Duration::from_millis(beat_ms)).await;
                }
            }
        }
    }
}

/// 每个快照轮调用（fast_round 发出事件后）：tooltip 升级为 `Prune · CPU 12%`。
/// 动画关闭时数字仍然更新——图标给量感，tooltip 给精确值，两通道各司其职。
pub fn update_tooltip(app: &AppHandle, usage: Option<f32>) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else { return };
    let tip = match usage {
        Some(u) => format!("Prune · CPU {u:.0}%"),
        None => "Prune".to_string(),
    };
    if let Err(e) = tray.set_tooltip(Some(tip)) {
        eprintln!("[m6-tray] tooltip 更新失败: {e}");
    }
}

// ---- M6-T3 · 开关持久化 ----

fn settings_path(app: &AppHandle) -> Option<std::path::PathBuf> {
    app.path().config_dir().ok().map(|d| d.join(SETTINGS_FILE))
}

/// 读开关状态；文件不存在/缺键/解析失败一律默认开
fn load_cpu_anim_setting(app: &AppHandle) -> bool {
    let Some(path) = settings_path(app) else {
        return true;
    };
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
        .and_then(|v| v.get(SETTING_CPU_ANIM).and_then(|x| x.as_bool()))
        .unwrap_or(true)
}

/// 只改本键，保留 settings.json 里其它模块的键
fn save_cpu_anim_setting(app: &AppHandle, enabled: bool) -> Result<(), String> {
    let path = settings_path(app).ok_or_else(|| "config_dir 不可用".to_string())?;
    let mut root: serde_json::Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    root[SETTING_CPU_ANIM] = serde_json::Value::Bool(enabled);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let body = serde_json::to_string_pretty(&root).map_err(|e| e.to_string())?;
    std::fs::write(&path, body).map_err(|e| e.to_string())
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
    let Some(rect) = app.tray_by_id(TRAY_ID).and_then(|t| t.rect().ok().flatten()) else {
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

    eprintln!(
        "[m0-tray] 主窗口定位: ({x:.0}, {y:.0}) scale {scale}（tray 所在屏幕）"
    );
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
