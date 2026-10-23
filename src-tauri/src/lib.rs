//! Prune 桌面端入口。业务逻辑一律落在 prune-core，这里只做壳与命令桥接。
//!
//! 模块：
//! - `tray.rs`    菜单栏 tray + 面板定位（M0 spike 结论已固化）
//! - `sampler.rs` 内存双频采样（M2/T-206）
//! - `commands.rs` IPC 读写（M2/T-207）

use std::sync::Arc;

use tauri::Manager;

mod commands;
mod sampler;
mod tray;

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
            commands::open_main
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

            // 后台常采：无 WebView 也运行，前端打开时直接取最新快照
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                sampler::run(handle).await;
            });

            // 面板失焦即隐藏（技术方案 §6）
            if let Some(panel) = app.get_webview_window("panel") {
                tray::attach_blur_to_hide(&panel);
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
