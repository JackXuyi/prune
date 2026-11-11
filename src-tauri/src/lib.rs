//! Prune 桌面端入口。业务逻辑一律落在 prune-core，这里只做壳与命令桥接。
//!
//! 模块：
//! - `tray.rs`    菜单栏 tray + 面板定位（M0 spike 结论已固化）
//! - `sampler.rs` 内存双频采样（M2/T-206）
//! - `port_sampler.rs` 端口采样（M7，独立通道 + 可见性门控）
//! - `net_sampler.rs` 应用网速采样（M10，1 s 轮询 + 可见性门控）
//! - `scan.rs`    磁盘可回收扫描调度（M1/T-306）
//! - `commands.rs` IPC 读写（M2/T-207 + M1/T-306）

use std::sync::Arc;

use tauri::Manager;

mod commands;
mod i18n;
mod lan;
mod net_diag;
mod net_sampler;
mod port_sampler;
mod sampler;
mod scan;
mod settings;
mod tray;
mod tray_icon;
mod updater;

// NSPopover 原生菜单栏面板（M3，docs/ns-popover-plan.md）
#[cfg(target_os = "macos")]
mod popover;

// 灵动岛（M15，docs/m15-dynamic-island-plan.md）。非 macOS 走 stub 空实现，
// 保证命令注册与模块路径在两端一致
#[cfg(target_os = "macos")]
mod island;
#[cfg(not(target_os = "macos"))]
#[path = "island_stub.rs"]
mod island;
#[cfg(target_os = "macos")]
mod island_geo;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // updater：GitHub 源自动升级（docs/updater-plan.md），endpoint 在 tauri.conf.json
    let builder = tauri::Builder::default().plugin(tauri_plugin_updater::Builder::new().build());

    // NSPanel 插件（M15）：必须注册，否则 PanelBuilder::build 里 `state()` 找不到
    // WebviewPanelManager，会在主线程 panic（且无法 unwind，直接 abort）
    #[cfg(target_os = "macos")]
    let builder = builder.plugin(tauri_nspanel::init());

    builder
        .invoke_handler(tauri::generate_handler![
            updater::update_check,
            updater::update_pending,
            updater::update_install,
            i18n::locale_get,
            i18n::locale_set,
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
            commands::cut_report,
            commands::leak_cases,
            commands::port_snapshot,
            commands::port_refresh,
            commands::port_set_active,
            commands::app_disk,
            commands::app_bundles,
            commands::app_icons,
            commands::app_history,
            commands::app_history_daily,
            net_sampler::net_speed_snapshot,
            net_sampler::net_speed_set_active,
            net_sampler::net_total_snapshot,
            net_sampler::net_total_history,
            net_sampler::net_traffic_recent,
            net_sampler::net_total_set_active,
            net_diag::net_diag_run,
            net_diag::net_diag_speed_run,
            net_diag::net_diag_capability,
            net_diag::net_diag_copy_report,
            net_diag::clipboard_write,
            lan::lan_snapshot,
            lan::lan_scan,
            lan::lan_capability,
            island::island_set_expanded,
            island::island_hide,
            island::island_ready,
            island::island_settings_get,
            island::island_settings_set,
            island::island_status
        ])
        .setup(|app| {
            // 资源守门员形态：常驻菜单栏，不占 Dock
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            // 语言必须在建托盘之前解析好：托盘菜单文案活在 Rust 进程里，
            // 建完再改就得重建整份菜单（见 tray::rebuild_menu）
            let resolved = i18n::resolve(app.handle());
            let app_locale = i18n::AppLocale::new();
            app_locale.set(resolved.locale);
            eprintln!(
                "[i18n] 启动语言: {:?}（{}，系统 {:?}）",
                resolved.locale,
                resolved.source.as_str(),
                resolved.system
            );
            app.manage(app_locale);

            tray::create(app.handle())?;

            // 灵动岛（M15）：建 NSPanel 并启动 hover / CPU 两条后台循环。
            // 失败只记日志——岛是增强项，不能因为它起不来就让整个 App 打不开
            // （纯外接屏、非 macOS、AppKit 调用失败都会走到这里）
            if let Err(e) = island::setup(app.handle()) {
                eprintln!("[m15-island] 初始化失败（不影响主功能）: {e}");
            }
            island::spawn_loops(app.handle());

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

            app.manage(Arc::new(sampler::MemState::new(store.clone())));
            // M21：store 同时注册为独立状态，清理落库（cut_log）与成绩单命令直接访问，
            // 不再借道 MemState/TotalNetState
            app.manage(store.clone());
            app.manage(Arc::new(scan::DiskState::new()));
            app.manage(Arc::new(port_sampler::PortState::new()));
            app.manage(Arc::new(commands::AppDiskState::new()));
            app.manage(Arc::new(net_sampler::NetSpeedState::new()));
            // M14：每日流量也落在同一个 prune.db（`net_daily`）。库打不开就传 None，
            // 网络 tab 的「今日」显示「不可用」，实时速率与会话累计照常工作
            app.manage(Arc::new(net_sampler::TotalNetState::new(store)));
            // 网络诊断（docs/net-diag-plan.md）：按需短任务，只存最近一轮结果供报告导出
            app.manage(net_diag::NetDiagState::new());
            // 局域网设备（docs/m18-lan-devices-spec.md §10）：同上，外加 scanning 门
            app.manage(lan::LanState::new());
            app.manage(Arc::new(updater::UpdateState::new()));

            // 启动静默检查更新（延迟 5 s，发现新版才广播事件，否则沉默）
            let update_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                updater::run_startup_check(update_handle).await;
            });

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

            // 网速：只在「应用资源」tab 可见时 1 s 轮采样（不可见时连子进程都不 spawn）
            let net_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                net_sampler::run_net_speed(net_handle).await;
            });

            // D3：后台 60 s 低频按应用流量采样（只喂小时历史，不推 UI）
            let hist_net_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                net_sampler::run_app_net_history(hist_net_handle).await;
            });

            // 菜单栏数字的总网速（M13）：2 s 常驻轻采样（sysinfo，无子进程），
            // 与上面按进程的 tab 采样完全解耦
            let total_net_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                net_sampler::run_total_speed(total_net_handle).await;
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

            // 动画专用新鲜 CPU 源（M21）：1 Hz 轻采样 + EMA，让游速跟手不滞后
            tauri::async_runtime::spawn(tray::run_cpu_watch());

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
                        if let Some(win) = unsafe { Retained::retain(ns_window as *mut NSWindow) } {
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
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|handle, event| {
            // M14：退出前把「今日流量」的最后一段增量落库。
            // 不落也不会丢太多（最多 60 s），但正常退出时没必要留这个缺口
            if let tauri::RunEvent::Exit = event {
                if let Some(state) = handle.try_state::<Arc<net_sampler::TotalNetState>>() {
                    state.flush_on_exit();
                }
            }
        });
}
