//! NSPopover 原生菜单栏面板（docs/ns-popover-plan.md）。
//!
//! 思路：WKWebView 不重建只搬家 —— 从 panel 窗口的 contentView 摘下，
//! 挂到 NSPopover 的 contentViewController.view 上；原窗口从此不再显示。
//! 锚点 = 一个与 tray 图标等大的透明窗口（Tauri 不暴露 NSStatusItem），
//! `tray.rect()`（物理像素/左上原点）换算成 Cocoa 坐标（逻辑点/左下原点）后定位。
//!
//! 状态机：Uninit → 首次 toggle 时尝试 init（成功 Ready / 失败 Failed 终态），
//! Failed 恒走 tray.rs 的旧窗口路径，不反复重试。

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use objc2::rc::Retained;
use objc2::runtime::{NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, ClassType, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAppearance, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSAutoresizingMaskOptions,
    NSBackingStoreType, NSColor, NSPopover, NSPopoverBehavior, NSPopoverDelegate, NSView,
    NSViewController, NSWindow, NSWindowCollectionBehavior, NSWindowOrderingMode,
    NSWindowStyleMask,
};
use objc2_foundation::{NSNotification, NSObject, NSPoint, NSRect, NSRectEdge, NSSize};
use tauri::{AppHandle, Manager, Position, WebviewWindow};

use crate::tray::{legacy_toggle, PANEL_H, PANEL_LABEL, PANEL_W, TRAY_ID};

/// Transient 收起后多久内忽略「再次打开」请求
/// （点图标收起的瞬间，Transient 也会因「点击外部」收起一次 → 抑制连点重开）
const REOPEN_GRACE_MS: u64 = 300;
/// kCGStatusWindowLevel（与菜单栏图标同层）
const STATUS_BAR_LEVEL: isize = 25;
/// 内容圆角：与 MenuBarPanel 卡片的 14px 圆角对齐，webview 方角内容被裁成
/// 与卡片一致的圆角；卡片已无外层 padding，边缘直接贴合 popover 内容区
const CONTENT_CORNER_RADIUS: f64 = 14.0;
/// popover 实色底衬：与 index.css 的 `--color-surface-raised` 同源。
/// NSPopover 默认用半透明材质，浅色主题下压在深色壁纸上会显出一块灰色底
/// （截图里的灰色箭头与灰色边框），与面板白卡撞色；铺一层实色底衬即可。
const LIGHT_SURFACE_RGB: (u8, u8, u8) = (0xff, 0xff, 0xff);
const DARK_SURFACE_RGB: (u8, u8, u8) = (0x14, 0x18, 0x1d);
/// 前端解析后的主题对应的表面色（0xRRGGBB），供主线程取用
static THEME_RGB: AtomicU32 = AtomicU32::new(pack_rgb(DARK_SURFACE_RGB));

const fn pack_rgb((r, g, b): (u8, u8, u8)) -> u32 {
    ((r as u32) << 16) | ((g as u32) << 8) | b as u32
}

fn is_light_theme() -> bool {
    THEME_RGB.load(Ordering::Acquire) == pack_rgb(LIGHT_SURFACE_RGB)
}

/// 当前主题对应的 popover 原生外观：影响箭头/边框等 webview 管不到的部分
fn appearance() -> Retained<NSAppearance> {
    // AppKit 的外观常量是 extern static，读取必须进 unsafe；只取引用不改它
    let name: &objc2_app_kit::NSAppearanceName = unsafe {
        if is_light_theme() {
            NSAppearanceNameAqua
        } else {
            NSAppearanceNameDarkAqua
        }
    };
    NSAppearance::appearanceNamed(name)
        .unwrap_or_else(|| unsafe { msg_send![NSAppearance::class(), new] })
}

fn surface_color() -> Retained<NSColor> {
    let packed = THEME_RGB.load(Ordering::Acquire);
    let r = ((packed >> 16) & 0xff) as f64 / 255.0;
    let g = ((packed >> 8) & 0xff) as f64 / 255.0;
    let b = (packed & 0xff) as f64 / 255.0;
    NSColor::colorWithSRGBRed_green_blue_alpha(r, g, b, 1.0)
}

/// 记录前端主题；实际改 UI 必须在主线程，由 `apply_theme` 完成。
pub fn remember_theme(theme: &str) {
    let rgb = match theme {
        "light" => LIGHT_SURFACE_RGB,
        _ => DARK_SURFACE_RGB,
    };
    THEME_RGB.store(pack_rgb(rgb), Ordering::Release);
}

/// 主题变化后重刷 popover 外观与底衬（主线程调用）。
pub fn apply_theme() {
    let mut slot = SLOT.lock().unwrap_or_else(|e| e.into_inner());
    if let Slot::Ready(ui) = &mut *slot {
        ui.0.apply_theme();
    }
}

static LAST_CLOSED_MS: AtomicU64 = AtomicU64::new(0);
static SLOT: Mutex<Slot> = Mutex::new(Slot::Uninit);

/// 面板当前是否走 popover 路径（Ready = true）。`panel_fit_height` 命令据此
/// 决定缩放 popover 还是缩放降级窗口——两条路径的几何句柄不同。
static POPOVER_MODE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 前端最近一次上报的内容自然高（px，逻辑点取整；0 = 尚未上报）。
///
/// 面板 webview 在 app 启动时就挂载（隐藏窗口），此时 popover 还没初始化，
/// 上报只能落在隐藏窗口上（legacy set_size）。init 时用它作初始 contentSize——
/// 否则用户第一次点开面板永远回到常量 432，W2 本地服务段（~90px）被裁掉，
/// 这正是真机「面板底部有遮挡」的根因。
static LAST_FIT_PX: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// 记录最近一次贴高上报（commands 层在分流前调用；原子写，线程安全）
pub fn remember_fit(px: f64) {
    let v = px.round().clamp(0.0, u32::MAX as f64) as u32;
    LAST_FIT_PX.store(v, Ordering::Release);
}

/// init 用初始高度：最近上报值；从未上报过则回退常量 PANEL_H
fn last_fit_px() -> f64 {
    let v = LAST_FIT_PX.load(Ordering::Acquire);
    if v > 0 {
        v as f64
    } else {
        PANEL_H
    }
}

/// MainThreadOnly 的 ObjC 句柄不能进 static（不 Send）。
/// SAFETY: 本模块所有 ObjC 访问都经 `run_on_main_thread` 串行到主线程，
/// 该包装只解除「存进 Mutex」的 Send 要求，不改变访问线程。
struct MainThreadBox<T>(T);

unsafe impl<T> Send for MainThreadBox<T> {}

enum Slot {
    Uninit,
    Ready(MainThreadBox<PopoverUi>),
    Failed,
}

struct PopoverUi {
    popover: Retained<NSPopover>,
    /// 锚点窗口：与 tray 图标等大的透明窗口，仅作 popover 的几何参照
    anchor: Retained<NSWindow>,
    /// 承载 webview 的 host view：借它的 window 找到 popover 的 frameView 铺底衬
    host: Retained<NSView>,
    /// 实色底衬（覆盖 popover 的箭头/边框/边距），主题切换时改它的 layer 颜色
    backdrop: Option<Retained<NSView>>,
    /// NSPopover 对 delegate 持 weak 引用，必须自持
    _delegate: Retained<PopoverDelegate>,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Transient 抖动抑制：距上次收起不足 grace 则不重开（可测纯函数）
fn should_reopen(last_closed_ms: u64, now_ms: u64, grace_ms: u64) -> bool {
    now_ms.saturating_sub(last_closed_ms) >= grace_ms
}

/// `tray.rect()`（物理像素、主屏左上原点）→ 锚点窗口的 Cocoa frame（逻辑点、左下原点）
///
/// M0 spike 实测：tray.rect() = position(2066, 0) size(68, 76)（2x 屏）。
/// 例：scale=2、主屏逻辑高 1107 → x=1033, y_bottom=1107−0−38=1069。
fn anchor_frame(
    rx: f64,
    ry: f64,
    rw: f64,
    rh: f64,
    scale: f64,
    primary_height_pts: f64,
) -> (f64, f64, f64, f64) {
    let x = rx / scale;
    let w = rw / scale;
    let h = rh / scale;
    let y_bottom = primary_height_pts - ry / scale - h;
    (x, y_bottom, w, h)
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "PrunePopoverDelegate"]
    struct PopoverDelegate;

    impl PopoverDelegate {
        /// Transient 收起（点外部）与程序化 close() 都会走到这里 —— 记录时间戳供抖动抑制
        #[unsafe(method(popoverDidClose:))]
        fn popover_did_close(&self, _notification: &NSNotification) {
            LAST_CLOSED_MS.store(now_ms(), Ordering::Release);
        }
    }

    unsafe impl NSObjectProtocol for PopoverDelegate {}
    unsafe impl NSPopoverDelegate for PopoverDelegate {}
);

impl PopoverDelegate {
    fn new() -> Retained<Self> {
        // NSObject 的类方法 +new（alloc + init 默认实例；本类无自定义 ivars）
        unsafe { msg_send![Self::class(), new] }
    }
}

/// 供 tray.rs 左键点击调用（经 run_on_main_thread 保证在主线程）
pub fn toggle(app: &AppHandle, panel: &WebviewWindow) {
    let mut slot = SLOT.lock().unwrap_or_else(|e| e.into_inner());
    match &mut *slot {
        Slot::Ready(ui) => ui.0.toggle(app),
        Slot::Failed => legacy_toggle(app, panel),
        Slot::Uninit => match PopoverUi::init(app) {
            Ok(mut ui) => {
                ui.toggle(app);
                *slot = Slot::Ready(MainThreadBox(ui));
                POPOVER_MODE.store(true, Ordering::Release);
            }
            Err(e) => {
                eprintln!("[ns-popover] 初始化失败，回退旧窗口路径: {e}");
                *slot = Slot::Failed;
                POPOVER_MODE.store(false, Ordering::Release);
                legacy_toggle(app, panel);
            }
        },
    }
}

/// 供 tray.rs 双击图标调用：确保面板处于收起态（本来就没显示则 no-op）。
///
/// 不做 toggle 语义 —— 双击的第一击已经把面板弹出来了，这里只负责让它退场，
/// 不必知道它此前是开是关。
pub fn hide(panel: &WebviewWindow) {
    let mut slot = SLOT.lock().unwrap_or_else(|e| e.into_inner());
    match &mut *slot {
        Slot::Ready(ui) => ui.0.hide(),
        // 降级路径下就是普通窗口
        Slot::Failed => {
            let _ = panel.hide();
        }
        // 从未初始化过 → 面板从未显示过
        Slot::Uninit => {}
    }
}

/// 面板是否在 popover 路径上（供 `panel_fit_height` 分流）
pub fn is_popover_mode() -> bool {
    POPOVER_MODE.load(Ordering::Acquire)
}

/// 贴高（W2 菜单栏「本地服务」段引入动态高度）：前端量取内容自然高后上报，
/// popover 内容尺寸随之缩放。WKWebView 带 width/height 可变 autoresizing
/// mask（见 init 步骤 3），host 视图被 popover 按新 contentSize 重排时自动跟随。
///
/// 必须在主线程调用（经 commands 层 `run_on_main_thread` 串行进来）。
pub fn fit_height(height: f64) {
    let slot = SLOT.lock().unwrap_or_else(|e| e.into_inner());
    if let Slot::Ready(ui) = &*slot {
        ui.0.set_content_height(height);
    }
}

impl PopoverUi {
    /// 一次性初始化：搬 webview + 建锚点窗口 + 建 popover
    fn init(app: &AppHandle) -> Result<Self, String> {
        // ObjC 对象全是 MainThreadOnly：调用方已保证主线程，这里再断言一次
        let mtm = MainThreadMarker::new().ok_or("popover 初始化不在主线程")?;
        let Some(panel) = app.get_webview_window(PANEL_LABEL) else {
            return Err(format!("找不到 {PANEL_LABEL} 窗口"));
        };

        // 1) 取 WKWebView（Tauri 2 macOS 专有 API；它本质是 NSView 子类）
        let ptr = panel
            .ns_view()
            .map_err(|e| format!("ns_view() 失败: {e}"))?;
        if ptr.is_null() {
            return Err("ns_view() 返回空指针".into());
        }
        let webview: Retained<NSView> =
            unsafe { Retained::retain(ptr as *mut NSView).ok_or("ns_view retain 失败")? };

        // 2) 锚点窗口（位置按当前 tray.rect() 一次定位，此后每次 show 前刷新）
        let Some((frame, _scale, _primary_h)) = anchor_metrics(app) else {
            return Err("tray.rect() 不可用，无法锚定".into());
        };
        let anchor = build_anchor(frame, mtm);

        // 3) WKWebView 搬进 NSViewController.view
        //    初始高度直接用前端最近上报的内容自然高（last_fit_px）——面板
        //    webview 启动时已挂载并上报过贴高，但只能落在隐藏窗口上；init
        //    若硬设回常量，内容高与常量不符就会留下空白（或裁切），且 RO
        //    因表面自然高没变不会再触发。last_fit_px 只在**从未上报**时
        //    回退 PANEL_H 兜底；正常路径一定贴合内容，不收下限。
        let init_h = last_fit_px();
        let vc: Retained<NSViewController> =
            NSViewController::initWithNibName_bundle(NSViewController::alloc(mtm), None, None);
        let host = vc.view();
        let content = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(PANEL_W, init_h));
        {
            webview.removeFromSuperview();
            webview.setFrame(content);
            // 容器缩放时 webview 跟随（相当于四边贴边约束）
            webview.setAutoresizingMask(
                NSAutoresizingMaskOptions::ViewWidthSizable
                    | NSAutoresizingMaskOptions::ViewHeightSizable,
            );
            host.setFrame(content);
            host.setWantsLayer(true);
            if let Some(layer) = host.layer() {
                layer.setCornerRadius(CONTENT_CORNER_RADIUS);
                layer.setMasksToBounds(true);
            }
            host.addSubview(&webview);
        }

        // 4) popover 本体（objc2 未生成 initWithContentViewController，用 +new 后设属性）
        let popover: Retained<NSPopover> = unsafe { msg_send![NSPopover::class(), new] };
        popover.setContentViewController(Some(&vc));
        popover.setContentSize(NSSize::new(PANEL_W, init_h));
        popover.setBehavior(NSPopoverBehavior::Transient);
        popover.setAppearance(Some(&appearance()));

        let delegate = PopoverDelegate::new();
        let proto: &ProtocolObject<dyn NSPopoverDelegate> = ProtocolObject::from_ref(&*delegate);
        popover.setDelegate(Some(proto));

        eprintln!("[ns-popover] 初始化完成（{PANEL_W}×{init_h:.0}，锚点 {frame:?}）");
        Ok(Self {
            popover,
            anchor,
            host,
            backdrop: None,
            _delegate: delegate,
        })
    }

    /// 收起（双击进主窗口时用）。delegate 会记 LAST_CLOSED_MS，
    /// 顺带把 300 ms 内的重开请求一起抑制掉 —— 用户此时已经去了主窗口。
    fn hide(&mut self) {
        if self.popover.isShown() {
            self.popover.close();
        }
    }

    /// 贴高：只改 popover 的 contentSize，host 与 webview 靠 autoresizing 跟随
    fn set_content_height(&self, height: f64) {
        self.popover.setContentSize(NSSize::new(PANEL_W, height));
    }

    fn toggle(&mut self, app: &AppHandle) {
        if self.popover.isShown() {
            // delegate.popoverDidClose 会更新 LAST_CLOSED_MS
            self.popover.close();
            return;
        }
        // 菜单栏图标可能被用户挪动 → 每次显示前重定位锚点
        if let Some((frame, _scale, _primary_h)) = anchor_metrics(app) {
            {
                self.anchor.setFrame_display(nsrect(frame), false);
                self.anchor.orderFrontRegardless();
            }
        }
        if !should_reopen(
            LAST_CLOSED_MS.load(Ordering::Acquire),
            now_ms(),
            REOPEN_GRACE_MS,
        ) {
            return; // Transient 收起引发的连点，忽略
        }
        let anchor_view = self.anchor.contentView().expect("锚点窗口必有 contentView");
        {
            self.popover.showRelativeToRect_ofView_preferredEdge(
                anchor_view.bounds(),
                &anchor_view,
                NSRectEdge::MinY, // 弹层在锚点下方，箭头向上指向图标
            );
        }
        self.install_backdrop();
    }

    /// 在 popover 的 frameView（contentView 的父视图）下面铺一层实色底衬。
    ///
    /// NSPopover 默认画半透明材质：浅色主题、深色壁纸下会露出灰色箭头和灰色
    /// 边框（webview 只覆盖 contentView，管不到这些）。底衬放在 contentView
    /// 下面，箭头区域一并被填成主题色；webview 自身透明，所以底衬同时接管
    /// 面板 p-2 边距与贴高多出的呼吸区，不再露出系统灰底。
    fn install_backdrop(&mut self) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let Some(window) = self.host.window() else {
            return;
        };
        let Some(content) = window.contentView() else {
            return;
        };
        // SAFETY: AppKit 视图层级只在主线程访问；本方法由 toggle/apply_theme 串行调用
        let Some(frame_view) = (unsafe { content.superview() }) else {
            return;
        };

        if let Some(old) = self.backdrop.take() {
            old.removeFromSuperview();
        }
        let backdrop = NSView::new(mtm);
        backdrop.setFrame(frame_view.bounds());
        backdrop.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        backdrop.setWantsLayer(true);
        if let Some(layer) = backdrop.layer() {
            layer.setBackgroundColor(Some(&surface_color().CGColor()));
        }
        frame_view.addSubview_positioned_relativeTo(
            &backdrop,
            NSWindowOrderingMode::Below,
            Some(&content),
        );
        self.backdrop = Some(backdrop);
    }

    /// 主题切换：外观 + 已铺的底衬一起刷新（主线程调用）
    fn apply_theme(&mut self) {
        self.popover.setAppearance(Some(&appearance()));
        if let Some(backdrop) = &self.backdrop {
            if let Some(layer) = backdrop.layer() {
                layer.setBackgroundColor(Some(&surface_color().CGColor()));
            }
        }
    }
}

// ---------- 几何与坐标换算 ----------

/// tray.rect() 原始值；末位标记是否为逻辑坐标（实测为物理，保留兜底）
type RawRect = (f64, f64, f64, f64, bool);

fn tray_rect_raw(app: &AppHandle) -> Option<RawRect> {
    let tray = app.tray_by_id(TRAY_ID)?;
    let rect = tray.rect().ok()??;
    let (x, y, pos_logical) = match rect.position {
        Position::Physical(p) => (p.x as f64, p.y as f64, false),
        Position::Logical(l) => (l.x, l.y, true),
    };
    let (w, h, size_logical) = match rect.size {
        tauri::Size::Physical(s) => (s.width as f64, s.height as f64, false),
        tauri::Size::Logical(s) => (s.width, s.height, true),
    };
    Some((x, y, w, h, pos_logical || size_logical))
}

/// `(anchor_frame, scale, primary_height_pts)` 的返回类型别名。
///
/// 四元组是 x / y / w / h，单独看签名分不出哪四个数——别名把语义钉在这里，
/// 也避免 clippy type_complexity。
type AnchorMetrics = ((f64, f64, f64, f64), f64, f64);

/// (anchor_frame, scale, primary_height_pts)
fn anchor_metrics(app: &AppHandle) -> Option<AnchorMetrics> {
    let (mut rx, mut ry, mut rw, mut rh, is_logical) = tray_rect_raw(app)?;
    // tray 图标所在屏幕决定 scale（多屏缩放不同），主屏决定高度换算
    let monitors = app.available_monitors().unwrap_or_default();
    let host = monitors.iter().find(|m| {
        let p = m.position();
        let s = m.size();
        rx >= p.x as f64 && rx <= (p.x + s.width as i32) as f64
    });
    let primary = app.primary_monitor().ok().flatten();
    let scale = host
        .map(|m| m.scale_factor())
        .or_else(|| primary.as_ref().map(|m| m.scale_factor()))
        .unwrap_or(1.0);
    let primary_h = primary
        .map(|m| m.size().height as f64 / m.scale_factor())
        .unwrap_or(1080.0);
    if is_logical {
        rx *= scale;
        ry *= scale;
        rw *= scale;
        rh *= scale;
    }
    let frame = anchor_frame(rx, ry, rw, rh, scale, primary_h);
    Some((frame, scale, primary_h))
}

fn nsrect((x, y, w, h): (f64, f64, f64, f64)) -> NSRect {
    NSRect::new(NSPoint::new(x, y), NSSize::new(w, h))
}

/// 透明锚点窗口：borderless / 不可成为 key / 不拦点击 / 跨空间跟随
fn build_anchor(frame: (f64, f64, f64, f64), mtm: MainThreadMarker) -> Retained<NSWindow> {
    unsafe {
        let win = NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            nsrect(frame),
            NSWindowStyleMask::Borderless,
            NSBackingStoreType::Buffered,
            false,
        );
        win.setReleasedWhenClosed(false);
        win.setOpaque(false);
        win.setBackgroundColor(Some(&NSColor::clearColor()));
        win.setIgnoresMouseEvents(true);
        win.setLevel(STATUS_BAR_LEVEL);
        win.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::FullScreenAuxiliary,
        );
        win.orderFrontRegardless();
        win
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backdrop_colors_match_css_tokens() {
        // 与 index.css 的 --color-surface-raised 两个主题值对齐
        assert_eq!(pack_rgb(LIGHT_SURFACE_RGB), 0xffffff);
        assert_eq!(pack_rgb(DARK_SURFACE_RGB), 0x14181d);
    }

    #[test]
    fn anchor_frame_2x_primary() {
        // M0 spike 实测：2x 屏，tray.rect() = (2066, 0, 68, 76)，主屏逻辑高 1107
        let (x, y, w, h) = anchor_frame(2066.0, 0.0, 68.0, 76.0, 2.0, 1107.0);
        assert_eq!(x, 1033.0);
        assert_eq!(y, 1069.0, "1107 - 0 - 38");
        assert_eq!(w, 34.0);
        assert_eq!(h, 38.0);
    }

    #[test]
    fn anchor_frame_1x() {
        let (x, y, w, h) = anchor_frame(100.0, 0.0, 22.0, 24.0, 1.0, 900.0);
        assert_eq!((x, y, w, h), (100.0, 876.0, 22.0, 24.0));
    }

    #[test]
    fn anchor_frame_secondary_monitor_negative_x() {
        // 左侧副屏：物理 x 为负，Cocoa 全局坐标同样允许负值
        let (x, _y, _w, _h) = anchor_frame(-2000.0, 0.0, 44.0, 48.0, 2.0, 1080.0);
        assert_eq!(x, -1000.0);
    }

    #[test]
    fn reopen_grace_suppresses_double_click() {
        // Transient 收起（记 t0）后 300ms 内的重开请求 → 忽略
        assert!(!should_reopen(1000, 1250, 300));
        assert!(should_reopen(1000, 1350, 300));
        // 系统时钟回拨：saturating_sub 得 0 → 持续抑制直到追上（保守方向，不会误重开）
        assert!(!should_reopen(2000, 1000, 300));
    }
}
