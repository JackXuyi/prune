//! 开机自启动（docs/autostart-plan.md）。
//!
//! 用官方 `tauri-plugin-autostart` 插件，但**不走插件的 JS API**——
//! capabilities 纪律是最小权限（default.json 仅 core:default），前端直调
//! 插件需要追加 `autostart:allow-*` 权限；收口成两个自定义 command，
//! 与 updater 的「Rust 侧调用、不走 JS ACL」同一约定。
//!
//! 状态不落 settings.json：`is_enabled()` 直接问系统（macOS 读 LaunchAgent
//! plist 是否存在），用户在「系统设置 → 登录项」里手动增删也能反映出来，
//! 避免两份真相。

use tauri::AppHandle;
use tauri_plugin_autostart::ManagerExt;

/// 读当前自启动状态（直接问系统，不做本地缓存）
#[tauri::command]
pub fn autostart_get(app: AppHandle) -> bool {
    app.autolaunch().is_enabled().unwrap_or(false)
}

/// 开/关自启动。失败带原因返回，前端回滚开关并提示
#[tauri::command]
pub fn autostart_set(app: AppHandle, enabled: bool) -> Result<(), String> {
    let autolaunch = app.autolaunch();
    if enabled {
        autolaunch.enable().map_err(|e| e.to_string())
    } else {
        autolaunch.disable().map_err(|e| e.to_string())
    }
}
