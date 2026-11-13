//! NSPopover 原生菜单栏面板（docs/ns-popover-plan.md）。
//!
//! 思路：WKWebView 不重建只搬家 —— 从 panel 窗口的 contentView 摘下，
//! 挂到 NSPopover 的 contentViewController.view 上；原窗口从此不再显示。
//! 锚点 = 一个与 tray 图标等大的透明窗口（Tauri 不暴露 NSStatusItem），
//! `tray.rect()`（物理像素/左上原点）换算成 Cocoa 坐标（逻辑点/左下原点）后定位。
//!
//! 状态机：Uninit → 首次 toggle 时尝试 init（成功 Ready / 失败 Failed 终态），
//! Failed 恒走 tray.rs 的旧窗口路径，不反复重试。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use objc2::rc::Retained;
use objc2::runtime::{NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, ClassType, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSBackingStoreType, NSColor, NSPopover, NSPopoverBehavior,
    NSPopoverDelegate, NSView, NSViewController, NSWindow, NSWindowCollectionBehavior,
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
/// 内容圆角：webview 方角内容被裁成 popover 圆角
const CONTENT_CORNER_RADIUS: f64 = 10.0;

static LAST_CLOSED_MS: AtomicU64 = AtomicU64::new(0);
static SLOT: Mutex<Slot> = Mutex::new(Slot::Uninit);

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
            }
            Err(e) => {
                eprintln!("[ns-popover] 初始化失败，回退旧窗口路径: {e}");
                *slot = Slot::Failed;
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
        let vc: Retained<NSViewController> =
            NSViewController::initWithNibName_bundle(NSViewController::alloc(mtm), None, None);
        let host = vc.view();
        let content = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(PANEL_W, PANEL_H));
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
        popover.setContentSize(NSSize::new(PANEL_W, PANEL_H));
        popover.setBehavior(NSPopoverBehavior::Transient);

        let delegate = PopoverDelegate::new();
        let proto: &ProtocolObject<dyn NSPopoverDelegate> = ProtocolObject::from_ref(&*delegate);
        popover.setDelegate(Some(proto));

        eprintln!("[ns-popover] 初始化完成（{PANEL_W}×{PANEL_H}，锚点 {frame:?}）");
        Ok(Self {
            popover,
            anchor,
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
