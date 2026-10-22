//! M0 #2 spike：Tauri tray 面板定位验证。
//!
//! 验证点（docs/prune-tauri-technical-plan.md §6 / §10）：
//! 1. `tray.rect()` 能否取到菜单栏图标坐标（多屏 / 不同 macOS 版本）
//! 2. 坐标系换算：tray-icon 返回的 rect 疑似 Cocoa 坐标（主屏左下原点，y 向上），
//!    而 Tauri 窗口定位用主屏左上原点、y 向下 —— 需要翻转验证
//! 3. 取不到 rect 时的降级：屏幕右上角
//! 4. Accessory 模式（无 Dock 图标）+ 无边框透明面板 + 失焦隐藏
//!
//! 运行 `pnpm tauri dev` 后观察 stdout 的 `[m0-tray]` 日志，并目测面板落点。

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{
    AppHandle, Manager, PhysicalPosition, Position, Rect, WebviewWindow, WindowEvent,
};

/// tray 图标唯一 ID
const TRAY_ID: &str = "prune-tray";
/// 面板窗口 label（与 tauri.conf.json windows 数组一致）
const PANEL_LABEL: &str = "panel";
/// 面板与菜单栏图标的间距（逻辑像素）
const MARGIN: f64 = 8.0;

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let open_main = MenuItem::with_id(app, "open-main", "打开主窗口", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出 Prune", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[&open_main, &PredefinedMenuItem::separator(app)?, &quit],
    )?;

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(app.default_window_icon().expect("内置图标缺失").clone())
        .tooltip("Prune")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open-main" => {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }
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
    Ok(())
}

/// 面板可见则隐藏，否则定位并显示
fn toggle_panel(app: &AppHandle) {
    let Some(panel) = app.get_webview_window(PANEL_LABEL) else {
        eprintln!("[m0-tray] ❌ 找不到 panel 窗口");
        return;
    };
    if panel.is_visible().unwrap_or(false) {
        let _ = panel.hide();
        return;
    }
    position_panel(app, &panel);
    let _ = panel.show();
    let _ = panel.set_focus();
}

/// 将面板定位到 tray 图标正下方（右对齐），失败则降级到屏幕右上角
fn position_panel(app: &AppHandle, panel: &WebviewWindow) {
    let panel_size = panel
        .outer_size()
        .map(|s| s.to_logical::<f64>(panel.scale_factor().unwrap_or(1.0)))
        .unwrap_or(tauri::LogicalSize::new(380.0, 560.0));

    let raw_rect: Option<Rect> = app
        .tray_by_id(TRAY_ID)
        .and_then(|t| t.rect().ok().flatten());

    let Some(rect) = raw_rect else {
        eprintln!("[m0-tray] ⚠️ tray.rect() 返回 None —— 命中降级路径（屏幕右上角）");
        fallback_top_right(app, panel, panel_size.width);
        return;
    };

    let (rx, ry) = pos_xy(&rect.position);
    let (rw, rh) = size_wh(&rect.size);
    eprintln!(
        "[m0-tray] tray.rect() 原始值: position=({rx}, {ry}) size=({rw}, {rh})"
    );

    // 主屏逻辑高度（Cocoa 坐标翻转基准）
    let primary = app.primary_monitor().ok().flatten();
    let Some(primary) = primary else {
        eprintln!("[m0-tray] ⚠️ 拿不到主屏信息 —— 命中降级路径");
        fallback_top_right(app, panel, panel_size.width);
        return;
    };
    let scale = primary.scale_factor();
    let primary_h_logical = primary.size().height as f64 / scale;
    eprintln!(
        "[m0-tray] 主屏: 物理高 {} px · scale {scale} · 逻辑高 {primary_h_logical} pt",
        primary.size().height
    );

    // 假设 rect 为 Cocoa 坐标（主屏左下原点、y 向上、逻辑 pt）：
    //   tray 图标底边（顶左系） = primary_h - ry
    //   panel 顶边 = tray 底边 + MARGIN
    //   panel 右对齐 tray 图标右缘
    let panel_x = rx + rw - panel_size.width - MARGIN;
    let panel_y = primary_h_logical - ry + MARGIN;
    eprintln!(
        "[m0-tray] 换算（假设 Cocoa 左下原点）: panel=({panel_x:.0}, {panel_y:.0}) 尺寸 {:.0}x{:.0}",
        panel_size.width, panel_size.height
    );

    let _ = panel.set_position(tauri::LogicalPosition::new(panel_x, panel_y));
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

/// 在 setup 中挂上「面板失焦 → 隐藏」
pub fn attach_blur_to_hide(panel: &WebviewWindow) {
    let handle = panel.clone();
    panel.on_window_event(move |event| {
        if let WindowEvent::Focused(false) = event {
            let _ = handle.hide();
        }
    });
}
