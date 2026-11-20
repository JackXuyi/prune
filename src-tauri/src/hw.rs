//! 硬件模块开关（f26 D7 + m21 外设）：六个模块各一个 settings.json 布尔键，默认全开。
//!
//! 前端 SettingsBoard「硬件模块」分组读写这两个命令；采样侧在每轮采样前
//! 自行查键（crate::settings::get_bool），关掉即停采。

use serde::Serialize;
use tauri::AppHandle;

use crate::settings;

/// 白名单：允许读写的键。不做开放代理——settings.json 里还有别的键。
pub const HW_KEYS: &[&str] = &[
    "hw.diskIo.enabled",
    "hw.smart.enabled",
    "hw.power.enabled",
    "hw.cpuTemp.enabled",
    "hw.periph.enabled",
    "net.publicIp.enabled",
];

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HwFlag {
    pub key: String,
    pub enabled: bool,
}

fn flag(handle: &AppHandle, key: &str) -> HwFlag {
    HwFlag {
        key: key.to_string(),
        enabled: settings::get_bool(handle, key).unwrap_or(true),
    }
}

#[tauri::command]
pub fn hw_flags(handle: AppHandle) -> Vec<HwFlag> {
    HW_KEYS.iter().map(|k| flag(&handle, k)).collect()
}

#[tauri::command]
pub fn hw_flag_set(handle: AppHandle, key: String, enabled: bool) -> Result<(), String> {
    if !HW_KEYS.contains(&key.as_str()) {
        return Err(format!("unknown hw flag: {key}"));
    }
    settings::set(&handle, &key, serde_json::Value::Bool(enabled))
}
