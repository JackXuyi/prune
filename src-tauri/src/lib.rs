//! Prune 桌面端入口。业务逻辑一律落在 prune-core，这里只做壳与命令桥接。
//!
//! 模块：
//! - `tray.rs`    菜单栏 tray + 面板定位（M0 spike 结论已固化）
//! - `sampler.rs` 内存双频采样（M2/T-206）
//! - `port_sampler.rs` 端口采样（M7，独立通道 + 可见性门控）
//! - `scan.rs`    磁盘可回收扫描调度（M1/T-306）
//! - `commands.rs` IPC 读写（M2/T-207 + M1/T-306）

use std::sync::Arc;

use tauri::Manager;

mod commands;
mod port_sampler;
mod sampler;
mod scan;
mod tray;

// NSPopover 原生菜单栏面板（M3，docs/ns-popover-plan.md）
#[cfg(target_os = "macos")]
mod popover;

/// 占位命令：暴露核心版本，验证前端 ↔ Rust 链路。
#[tauri::command]
fn app_info() -> serde_json::Value {
    serde_json::json!({
        "name": "Prune",
        "coreVersion": prune_core::version(),
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            app_info,
            commands::mem_snapshot,
            commands::mem_processes,
            commands::mem_history,
            commands::mem_refresh,
            commands::open_main,
            commands::kill_process,
            commands::reveal_path,
            commands::disk_report,
            commands::disk_volume,
            commands::disk_scan,
            commands::reclaim_preview,
            commands::reclaim_children,
            commands::reclaim_execute,
            commands::port_snapshot,
            commands::port_refresh,
            commands::port_set_active,
            commands::app_disk
        ])
        .setup(|app| {
            // 资源守门员形态：常驻菜单栏，不占 Dock
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            tray::create(app.handle())?;

            // 内存序列库：落在 app data 目录；打不开就退回纯内存模式（不阻塞采样）
            let store = app.path().app_data_dir().ok().and_then(|dir| {
                let path = dir.join("prune.db");
                match prune_core::db::Store::open(&path) {
                    Ok(store) => Some(Arc::new(store)),
                    Err(e) => {
                        eprintln!("[m2-mem] ⚠️ 序列库打开失败（{}）: {e}", path.display());
                        None
                    }
                }
            });

            app.manage(Arc::new(sampler::MemState::new(store)));
            app.manage(Arc::new(scan::DiskState::new()));
            app.manage(Arc::new(port_sampler::PortState::new()));
            app.manage(Arc::new(commands::AppDiskState::new()));

            // 后台常采：无 WebView 也运行，前端打开时直接取最新快照
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                sampler::run(handle).await;
            });

            // 端口：只在端口 tab 可见时才按 30 s 轮采样（见 port_sampler 模块注释）
            let port_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                port_sampler::run_port(port_handle).await;
            });

            // 磁盘侧：启动 30 s 后首扫，此后每周静默刷新
            let disk_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                scan::run_background(disk_handle).await;
            });

            // 托盘 CPU 动画（M6-T2）：与采样循环解耦，读 MemState 共享状态驱动换帧
            let anim_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                tray::run_anim(anim_handle).await;
            });

            // 面板失焦即隐藏（技术方案 §6）
            if let Some(panel) = app.get_webview_window("panel") {
                tray::attach_blur_to_hide(&panel);
            }

            // 主窗口：Accessory 模式下程序化 show() 不切 Space（M3 验收发现的
            // 「打开主窗口没反应」根因）。MoveToActiveSpace 让 show 时窗口跟随
            // 当前 Space；FullScreenAuxiliary 保证全屏 App 里也能弹出。
            #[cfg(target_os = "macos")]
            if let Some(main) = app.get_webview_window("main") {
                if let Ok(ns_window) = main.ns_window() {
                    if !ns_window.is_null() {
                        use objc2::rc::Retained;
                        use objc2_app_kit::{NSWindow, NSWindowCollectionBehavior};
                        if let Some(win) =
                            unsafe { Retained::retain(ns_window as *mut NSWindow) }
                        {
                            win.setCollectionBehavior(
                                NSWindowCollectionBehavior::MoveToActiveSpace
                                    | NSWindowCollectionBehavior::FullScreenAuxiliary,
                            );
                        }
                    }
                }
            }

            // 常驻守门员形态：主窗口点红叉只隐藏不销毁，否则 open_main 会因
            // 窗口已销毁而永久失效（Accessory 模式没有 Dock 图标可再打开）
            if let Some(main) = app.get_webview_window("main") {
                let main_handle = main.clone();
                main.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = main_handle.hide();
                    }
                });
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
