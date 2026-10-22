//! Prune 桌面端入口。业务逻辑一律落在 prune-core，这里只做壳与命令桥接。
//!
//! M0 阶段模块：tray.rs（菜单栏 tray + 面板定位 spike）。
//! M1 起增加 sampler.rs（后台采样线程）与 commands.rs。

use tauri::Manager;

mod tray;

/// 占位命令：暴露核心版本，验证前端 ↔ Rust 链路。
/// M1 起替换为真实的扫描 / 探测命令。
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
        .invoke_handler(tauri::generate_handler![app_info])
        .setup(|app| {
            // 资源守门员形态：常驻菜单栏，不占 Dock
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            tray::create(app.handle())?;

            // 面板失焦即隐藏（技术方案 §6）
            if let Some(panel) = app.get_webview_window("panel") {
                tray::attach_blur_to_hide(&panel);
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
