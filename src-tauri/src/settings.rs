//! 设置文件读写（M13-T5）：统一到 **app 专属目录**，并兼容旧版全局路径。
//!
//! 为什么需要这个模块：M13 之前 `tray.rs` / `tray_icon.rs` 各自用
//! `app.path().config_dir()` 拼路径 —— 那是**全局** `~/Library/Application Support`，
//! 不是 app 专属目录。后果有两个：
//! 1. `settings.json` 落在全局目录里，污染了别的应用也可能读的位置；
//! 2. 与 `~/Library/Application Support/<bundle-id>/settings.json` 形成两份配置，
//!    改一份另一份不生效（排查 M13 图标消失时踩到的坑）。
//!
//! 现在的规则：
//! - **写**：只写 `app_config_dir()/settings.json`
//! - **读**：新路径优先；新文件里**缺的键**用旧全局文件补 —— 老用户升级后
//!   主题、CPU 动画、菜单栏数字等设置不会突然回到默认值
//! - 旧文件不再写、也不删（用户手动清理无副作用）

use tauri::{AppHandle, Manager};

const SETTINGS_FILE: &str = "settings.json";

/// 新路径：app 专属目录（macOS: `~/Library/Application Support/<bundle-id>/`）
pub fn path(app: &AppHandle) -> Option<std::path::PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|d| d.join(SETTINGS_FILE))
}

/// 旧路径：全局 config 目录（M13 之前所有版本写在这里）
fn legacy_path(app: &AppHandle) -> Option<std::path::PathBuf> {
    app.path().config_dir().ok().map(|d| d.join(SETTINGS_FILE))
}

fn read_json(path: Option<std::path::PathBuf>) -> Option<serde_json::Value> {
    let raw = std::fs::read_to_string(path?).ok()?;
    serde_json::from_str(&raw).ok()
}

/// 读整个设置对象：新路径优先，旧全局文件里**新文件没有的键**补进来。
pub fn read_root(app: &AppHandle) -> serde_json::Value {
    let mut root = read_json(path(app)).unwrap_or_else(|| serde_json::json!({}));
    if !root.is_object() {
        root = serde_json::json!({});
    }
    let legacy = read_json(legacy_path(app));
    if let (Some(new_obj), Some(old_obj)) = (root.as_object_mut(), legacy.as_ref().and_then(|v| v.as_object())) {
        for (k, v) in old_obj {
            new_obj.entry(k.clone()).or_insert_with(|| v.clone());
        }
    }
    root
}

pub fn get_str(app: &AppHandle, key: &str) -> Option<String> {
    read_root(app).get(key)?.as_str().map(str::to_string)
}

pub fn get_bool(app: &AppHandle, key: &str) -> Option<bool> {
    read_root(app).get(key)?.as_bool()
}

/// 读浮点配置。`as_f64` 对 JSON 整数（`90`）同样返回 `Some(90.0)`，
/// 对小数只接受 `90.5` 这类；因此阈值一律走这里，不要用 `get_u64`（它对小数返回 None）。
pub fn get_f64(app: &AppHandle, key: &str) -> Option<f64> {
    read_root(app).get(key)?.as_f64()
}

/// 读整数配置。负数 / 越界由调用方钳制（本模块不理解业务范围）。
pub fn get_u64(app: &AppHandle, key: &str) -> Option<u64> {
    read_root(app).get(key)?.as_u64()
}

/// 只改一个键，保留其它键；永远写新路径。
pub fn set(app: &AppHandle, key: &str, value: serde_json::Value) -> Result<(), String> {
    set_many(app, &[(key, value)])
}

/// 批量改键，**只读写一次文件**。
///
/// 逐键调 [`set`] 时每键都要 read → parse → write 一遍；改 6 个配置项就是 6 次
/// 覆盖写，中途失败会留下「前 3 项新、后 3 项旧」的半套配置。
pub fn set_many(app: &AppHandle, pairs: &[(&str, serde_json::Value)]) -> Result<(), String> {
    let path = path(app).ok_or_else(|| "app_config_dir 不可用".to_string())?;
    let mut root = read_root(app);
    for (key, value) in pairs {
        root[*key] = value.clone();
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let body = serde_json::to_string_pretty(&root).map_err(|e| e.to_string())?;
    std::fs::write(&path, body).map_err(|e| e.to_string())
}
