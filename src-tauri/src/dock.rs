//! Dock 图标跟随主窗口（docs/dock-icon-plan.md）。
//!
//! 守门员形态：启动即 Accessory，常驻菜单栏不占 Dock；主窗口打开时临时
//! 显示 Dock 图标，主窗口隐藏后收回。打开路径唯一收口 `tray::show_main`，
//! 关闭路径唯一收口 `lib.rs` 的 CloseRequested——两边各调一次本模块即可，
//! 前端零改动。
//!
//! 用 `set_activation_policy`（NSApp.setActivationPolicy）而不是 tao 的
//! `set_dock_visibility`：后者底层走 `TransformProcessType`，快速切换会触发
//! macOS 残留多个 Dock 图标的系统级 bug（tao dock.rs 注释承认并只能做 1 s
//! 去抖规避）。本项目启动本就用 setActivationPolicy，全程同一机制不混用。
//! 防连点去重由 [`DOCK_REGULAR`] 的 swap 短路承担，切换频率天然很低。
//!
//! 图标：dev 模式跑的是裸二进制（无 .app bundle），切到 Regular 后 Dock 只能
//! 显示 macOS 通用「exec」图标。`set_app_icon` 在启动时把打包的 PNG 显式设给
//! NSApp，dev 补齐缺失的 bundle 图标，release 下重复设置同款图标，无害。

#[cfg(target_os = "macos")]
mod imp {
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicBool, Ordering};

    use objc2::AnyThread;
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::{NSData, MainThreadMarker};
    use tauri::AppHandle;

    /// 512² PNG，与 bundle.icon 的 icon.icns 同源（`tauri icon` 生成）。
    /// `include_bytes!` 编译进二进制，不依赖运行时文件路径。
    static APP_ICON_PNG: &[u8] = include_bytes!("../icons/icon.png");

    /// 当前是否处于「Dock 图标可见」状态。swap 一次完成判断 + 置位：
    /// 短时间内多次打开主窗口（面板连点、三连击去重兜底）只切一次策略，
    /// 避免 Dock 图标反复弹跳、菜单栏闪烁。
    static DOCK_REGULAR: AtomicBool = AtomicBool::new(false);

    /// 主窗口即将显示：确保 Dock 图标出现。调用方须在 show/set_focus 之前调——
    /// 顺序反了窗口已显示但 App 仍是 Accessory，set_focus 抢不到前台焦点。
    pub fn ensure_dock_visible(app: &AppHandle) {
        if DOCK_REGULAR.swap(true, Ordering::Relaxed) {
            return; // 已是 Regular，短路
        }
        if let Err(e) = app.set_activation_policy(tauri::ActivationPolicy::Regular) {
            eprintln!("[dock] 显示 Dock 图标失败: {e}");
            // 失败回滚状态，下次还能重试
            DOCK_REGULAR.store(false, Ordering::Relaxed);
        }
    }

    /// 主窗口已隐藏：收回 Dock 图标，回到纯菜单栏常驻形态。
    pub fn ensure_dock_hidden(app: &AppHandle) {
        if !DOCK_REGULAR.swap(false, Ordering::Relaxed) {
            return; // 已是 Accessory，短路
        }
        if let Err(e) = app.set_activation_policy(tauri::ActivationPolicy::Accessory) {
            eprintln!("[dock] 收回 Dock 图标失败: {e}");
        }
    }

    /// 启动时显式设置应用图标（见模块注释「图标」节）。
    /// `run_on_main_thread` 保证 NSApp 调用在主线程；调用点在 setup，本就在
    /// 主线程，这里多一层派发是防未来调用方线程变化。
    pub fn set_app_icon(app: &AppHandle) {
        let result = app.run_on_main_thread(|| unsafe {
            let Some(mtm) = MainThreadMarker::new() else { return };
            let data = NSData::dataWithBytes_length(
                APP_ICON_PNG.as_ptr().cast::<c_void>(),
                APP_ICON_PNG.len(),
            );
            let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) else {
                eprintln!("[dock] ❌ 应用图标 PNG 解码失败");
                return;
            };
            NSApplication::sharedApplication(mtm).setApplicationIconImage(Some(&image));
        });
        if let Err(e) = result {
            eprintln!("[dock] 设置应用图标失败: {e}");
        }
    }
}

// 非 macOS 无 Dock 概念：空实现保证调用点两端一致（同 island 模块的 stub 模式）
#[cfg(not(target_os = "macos"))]
mod imp {
    use tauri::AppHandle;

    pub fn ensure_dock_visible(_app: &AppHandle) {}
    pub fn ensure_dock_hidden(_app: &AppHandle) {}
    pub fn set_app_icon(_app: &AppHandle) {}
}

pub use imp::*;
