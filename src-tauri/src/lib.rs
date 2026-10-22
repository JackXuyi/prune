//! Prune 桌面端入口。业务逻辑一律落在 prune-core，这里只做壳与命令桥接。

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
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
