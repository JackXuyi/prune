//! 无 Tauri 的设置读取（W2/D-10）：CLI、MCP 与 GUI 读**同一份** settings.json。
//!
//! 为什么需要这个模块：`src-tauri/src/settings.rs` 的每个函数都要求 `&AppHandle`，
//! 而 CLI / MCP 没有 Tauri 运行时。若各自拼路径、各自写默认值，就会出现
//! 「GUI 里把 swap 阈值改成 70%，`prune watch` 仍按 80% 判」——这类不一致在 issue
//! 里最难查，因为两边都「看起来正常」。
//!
//! 规则与 GUI 完全一致（见 `src-tauri/src/settings.rs` 头部）：
//! - **读**：新路径（app 专属目录）优先，新文件里缺的键用旧全局文件补；
//! - **写**：本模块**只读**。CLI 从不修改设置文件 —— 改配置只有两条路：
//!   GUI 设置页，或用户自己编辑 JSON。工具悄悄改配置是信任问题，不是便利问题。

use std::path::PathBuf;

use serde_json::Value;

/// app 专属目录名（与 `paths::lan_cache_path` / `default_db_path` 同一口径）
pub const BUNDLE_DIR: &str = "com.xuyi.prune";
const SETTINGS_FILE: &str = "settings.json";

/// 新路径：`<config_dir>/com.xuyi.prune/settings.json`
/// （macOS: `~/Library/Application Support/com.xuyi.prune/`）
pub fn settings_path() -> Option<PathBuf> {
    Some(
        crate::paths::config_dir()?
            .join(BUNDLE_DIR)
            .join(SETTINGS_FILE),
    )
}

/// 旧路径：全局 config 目录下的 `settings.json`（M13 之前所有版本写在这里）
fn legacy_path() -> Option<PathBuf> {
    Some(crate::paths::config_dir()?.join(SETTINGS_FILE))
}

/// 读整个设置对象（新路径优先，旧文件补缺失键）。
///
/// 文件不存在 / 不是合法 JSON / 根不是对象 → 返回空对象，与 GUI 的
/// `read_root` 行为一致：调用方拿到的永远是 object，不需要再判空。
pub fn read_root() -> Value {
    read_root_at(settings_path(), legacy_path())
}

/// 可注入路径的实现（测试用；生产走 [`read_root`]）
fn read_root_at(new: Option<PathBuf>, legacy: Option<PathBuf>) -> Value {
    let mut root = read_json(new).unwrap_or_else(|| Value::Object(Default::default()));
    if !root.is_object() {
        root = Value::Object(Default::default());
    }
    if let (Some(obj), Some(old)) = (
        root.as_object_mut(),
        read_json(legacy)
            .as_ref()
            .and_then(|v| v.as_object())
            .cloned(),
    ) {
        for (k, v) in old {
            obj.entry(k).or_insert(v);
        }
    }
    root
}

fn read_json(path: Option<PathBuf>) -> Option<Value> {
    let raw = std::fs::read_to_string(path?).ok()?;
    serde_json::from_str(&raw).ok()
}

/// 读浮点配置。JSON 整数（`90`）同样返回 `Some(90.0)`；nil / 字符串返回 None。
/// 阈值一律走这里，不要用 `get_u64`（它对小数返回 None）。
pub fn get_f64(key: &str) -> Option<f64> {
    pick_f64(&read_root(), key)
}

pub fn get_u64(key: &str) -> Option<u64> {
    pick_u64(&read_root(), key)
}

pub fn get_bool(key: &str) -> Option<bool> {
    pick_bool(&read_root(), key)
}

pub fn get_str(key: &str) -> Option<String> {
    pick_str(&read_root(), key)
}

/// 读任意 JSON 值（如 `netSites` 数组）。键不存在返回 None，不是 Null。
pub fn get_value(key: &str) -> Option<Value> {
    read_root().get(key).cloned()
}

// 纯取值函数与文件读取分开：测试不必造临时 HOME，只喂 Value 即可。

fn pick_f64(root: &Value, key: &str) -> Option<f64> {
    root.get(key)?.as_f64()
}

fn pick_u64(root: &Value, key: &str) -> Option<u64> {
    root.get(key)?.as_u64()
}

fn pick_bool(root: &Value, key: &str) -> Option<bool> {
    root.get(key)?.as_bool()
}

fn pick_str(root: &Value, key: &str) -> Option<String> {
    root.get(key)?.as_str().map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tmp(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "prune-config-test-{}-{name}.json",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&p);
        p
    }

    #[test]
    fn new_file_wins_and_legacy_fills_missing_keys() {
        let new = tmp("new");
        let old = tmp("old");
        std::fs::write(&new, json!({ "island.swap_pct": 70.0 }).to_string()).unwrap();
        std::fs::write(
            &old,
            json!({ "island.swap_pct": 88.0, "island.cpu_pct": 75.0 }).to_string(),
        )
        .unwrap();

        let root = read_root_at(Some(new.clone()), Some(old.clone()));
        // 新文件有的键以新文件为准
        assert_eq!(pick_f64(&root, "island.swap_pct"), Some(70.0));
        // 新文件没有的键从旧文件补
        assert_eq!(pick_f64(&root, "island.cpu_pct"), Some(75.0));

        let _ = std::fs::remove_file(&new);
        let _ = std::fs::remove_file(&old);
    }

    #[test]
    fn missing_or_broken_file_yields_empty_object() {
        let broken = tmp("broken");
        std::fs::write(&broken, "{ not json").unwrap();
        let root = read_root_at(Some(broken.clone()), None);
        assert!(root.is_object());
        assert_eq!(pick_f64(&root, "island.swap_pct"), None);
        let _ = std::fs::remove_file(&broken);
    }

    #[test]
    fn non_object_root_is_normalized() {
        let weird = tmp("array");
        std::fs::write(&weird, "[1,2,3]").unwrap();
        let root = read_root_at(Some(weird.clone()), None);
        assert!(root.is_object());
        let _ = std::fs::remove_file(&weird);
    }

    #[test]
    fn pick_helpers_do_not_confuse_int_and_float() {
        let root = json!({ "a": 90, "b": 90.5, "c": "x", "d": true });
        assert_eq!(pick_f64(&root, "a"), Some(90.0));
        assert_eq!(pick_f64(&root, "b"), Some(90.5));
        assert_eq!(pick_u64(&root, "b"), None, "小数不该被当成整数");
        assert_eq!(pick_str(&root, "c"), Some("x".to_string()));
        assert_eq!(pick_bool(&root, "d"), Some(true));
        assert_eq!(pick_f64(&root, "missing"), None);
    }
}
