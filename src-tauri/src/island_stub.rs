//! 非 macOS 平台的空实现。
//!
//! 灵动岛依赖 `tauri-nspanel`（仅 macOS）与 `NSScreen.safeAreaInsets`（仅 macOS），
//! 所以 Windows 上保留同名命令但什么都不做 —— 这样 `lib.rs` 不用为命令注册写
//! 条件编译（`generate_handler!` 不接受带 `#[cfg]` 的列表项）。
//!
//! 与 macOS 版保持**完全相同的函数签名与类型**，否则命令注册会在 Windows 上编译失败。

use tauri::AppHandle;

pub fn setup(_app: &AppHandle) -> Result<(), String> {
    Ok(())
}

pub fn spawn_loops(_app: &AppHandle) {}

pub fn load_cfg(_app: &AppHandle) {}

pub fn is_enabled(_app: &AppHandle) -> bool {
    false
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IslandSettings {
    pub enabled: bool,
    pub cpu_pct: f64,
    pub cpu_sustain_sec: u64,
    pub cpu_tolerance_pct: f64,
    pub swap_pct: f64,
    pub max_per_hour: u64,
}

#[derive(Debug, Clone, Copy, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IslandStatus {
    pub available: bool,
    pub notch_w: f64,
    pub notch_h: f64,
}

#[tauri::command]
pub fn island_status() -> IslandStatus {
    IslandStatus {
        available: false,
        notch_w: 0.0,
        notch_h: 0.0,
    }
}

#[tauri::command]
pub fn island_set_expanded(_expanded: bool) {}

#[tauri::command]
pub fn island_hide(_app: AppHandle) {}

#[tauri::command]
pub fn island_ready(_app: AppHandle) -> bool {
    false
}

#[tauri::command]
pub fn island_reposition(_app: AppHandle) {}

#[tauri::command]
pub fn island_settings_get(_app: AppHandle) -> IslandSettings {
    IslandSettings {
        enabled: false,
        cpu_pct: 90.0,
        cpu_sustain_sec: 60,
        cpu_tolerance_pct: 90.0,
        swap_pct: 80.0,
        max_per_hour: 6,
    }
}

#[tauri::command]
pub fn island_settings_set(
    _app: AppHandle,
    settings: IslandSettings,
) -> Result<IslandSettings, String> {
    Ok(settings)
}
