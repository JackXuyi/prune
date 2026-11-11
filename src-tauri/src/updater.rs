//! GitHub 源自动升级（docs/updater-plan.md）。
//!
//! 设计要点：
//! - 更新逻辑全部落在 Rust 侧（`tauri-plugin-updater`），前端通过自定义命令
//!   调用即可——**自定义命令不走插件权限门**，capabilities 保持最小权限
//!   （仍只有 `core:default`），不需要为 updater JS API 放权。
//! - 托盘「检查更新」与面板入口复用同一套 `check_and_notify`，结果通过
//!   事件广播给所有窗口；面板隐藏时事件照收，重开面板时还有
//!   `update_pending` 兜底查询，状态不丢。
//! - 启动静默检查：只在新版本可用时发事件打扰，已是最新则完全沉默。
//! - 重启交给插件 `install()`（macOS/Linux 自动重启，Windows 跑完 NSIS
//!   安装器后自启），Rust 侧不做手动 restart，避免与插件语义打架。
//!
//! 事件契约（前端监听）：
//! - `update://available`   发现新版本（启动静默检查 / 手动检查共用）
//!   `{ status, version, currentVersion, notes, date }`
//! - `update://check-result` 手动检查的「无更新 / 出错」回执
//!   `{ status: "up-to-date" }` 或 `{ status: "error", message }`
//! - `update://progress`    下载进度 `{ downloaded, total }`
//! - `update://installed`   下载安装完成，应用即将重启

use std::sync::{Arc, Mutex};

use prune_core::i18n::err::err_text;
use prune_core::i18n::fill;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::UpdaterExt;

/// 待安装的更新（非 Clone，一次 take；升级成功后应用即重启）
pub struct UpdateState(Mutex<Option<tauri_plugin_updater::Update>>);

impl UpdateState {
    pub fn new() -> Self {
        Self(Mutex::new(None))
    }
}

impl Default for UpdateState {
    fn default() -> Self {
        Self::new()
    }
}

pub const EVENT_AVAILABLE: &str = "update://available";
pub const EVENT_CHECK_RESULT: &str = "update://check-result";
pub const EVENT_PROGRESS: &str = "update://progress";
pub const EVENT_INSTALLED: &str = "update://installed";

/// 从 Update 组装前端负载（payload 不直接序列化 Update：其字段类型随
/// 插件版本变化，手动取需要的字段更稳）
fn pending_payload(update: &tauri_plugin_updater::Update) -> serde_json::Value {
    serde_json::json!({
        "status": "available",
        "version": update.version,
        "currentVersion": update.current_version,
        "notes": update.body,
        "date": update.date.as_ref().map(|d| d.to_string()),
    })
}

/// 检查更新并按需通知。
///
/// - `manual = true`（托盘菜单 / 面板按钮）：无更新、出错也发回执事件，
///   让用户知道「检查过了」。
/// - `manual = false`（启动静默检查）：只有发现新版本才发事件，否则沉默。
///
/// 返回值与 `update://available` / `update://check-result` 的负载一致。
pub async fn check_and_notify(app: &AppHandle, manual: bool) -> Result<serde_json::Value, String> {
    let err_t = err_text(crate::i18n::current(app));
    let updater = app
        .updater()
        .map_err(|e| fill(err_t.updater_unavailable, &[("err", &e.to_string())]))?;

    match updater.check().await {
        Ok(Some(update)) => {
            let payload = pending_payload(&update);
            let state = app.state::<Arc<UpdateState>>();
            *state.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(update);
            eprintln!(
                "[updater] 发现新版本 {}（当前 {}）",
                payload["version"], payload["currentVersion"]
            );
            let _ = app.emit(EVENT_AVAILABLE, &payload);
            Ok(payload)
        }
        Ok(None) => {
            let state = app.state::<Arc<UpdateState>>();
            *state.0.lock().unwrap_or_else(|e| e.into_inner()) = None;
            if manual {
                let payload = serde_json::json!({ "status": "up-to-date" });
                let _ = app.emit(EVENT_CHECK_RESULT, &payload);
                return Ok(payload);
            }
            eprintln!("[updater] 启动检查：已是最新");
            Ok(serde_json::json!({ "status": "up-to-date" }))
        }
        Err(e) => {
            if manual {
                let payload = serde_json::json!({ "status": "error", "message": e.to_string() });
                let _ = app.emit(EVENT_CHECK_RESULT, &payload);
            }
            Err(e.to_string())
        }
    }
}

/// 启动静默检查：延迟 5 s（避开启动期采样高峰），失败不打扰用户。
pub async fn run_startup_check(app: AppHandle) {
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    if let Err(e) = check_and_notify(&app, false).await {
        eprintln!("[updater] 启动静默检查失败（不影响使用）: {e}");
    }
}

/// 手动检查更新（面板入口）。返回值直接给 UI 渲染。
#[tauri::command]
pub async fn update_check(app: AppHandle) -> Result<serde_json::Value, String> {
    check_and_notify(&app, true).await
}

/// 查询本次会话内已发现的待安装更新（面板重开后状态兜底）。
#[tauri::command]
pub fn update_pending(app: AppHandle) -> serde_json::Value {
    let state = app.state::<Arc<UpdateState>>();
    let guard = state.0.lock().unwrap_or_else(|e| e.into_inner());
    match guard.as_ref() {
        Some(update) => pending_payload(update),
        None => serde_json::json!({ "status": "none" }),
    }
}

/// 下载并安装待更新，完成后应用重启。
///
/// 平台语义（插件 2.14.1 文档）：Windows 启动 NSIS 安装器后自动退出应用；
/// macOS / Linux 装完新版本后需要手动重启 —— 统一走 `app.restart()`
/// （类型为 `!`，永远不返回，Windows 路径到不了这一步）。
#[tauri::command]
pub async fn update_install(app: AppHandle) -> Result<(), String> {
    let taken = {
        let state = app.state::<Arc<UpdateState>>();
        let mut guard = state.0.lock().unwrap_or_else(|e| e.into_inner());
        guard.take()
    };
    let err_t = err_text(crate::i18n::current(&app));
    let update = taken.ok_or_else(|| err_t.update_none_pending.to_string())?;

    eprintln!("[updater] 开始下载 {}", update.version);
    let app_for_progress = app.clone();
    let mut downloaded: u64 = 0;
    update
        .download_and_install(
            move |chunk, total| {
                downloaded += chunk as u64;
                let payload = serde_json::json!({ "downloaded": downloaded, "total": total });
                let _ = app_for_progress.emit(EVENT_PROGRESS, payload);
            },
            || {},
        )
        .await
        .map_err(|e| fill(err_t.update_install_failed, &[("err", &e.to_string())]))?;

    let _ = app.emit(EVENT_INSTALLED, ());
    eprintln!("[updater] 安装完成，重启应用");
    app.restart()
}
