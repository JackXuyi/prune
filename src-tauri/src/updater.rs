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
use prune_core::locale::Locale;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
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

/// 托盘「检查更新」原生弹窗文案（WorkBuddy 式：点了就弹结果）。
///
/// 与前端 `update` namespace 分开维护：弹窗在 Rust 侧直接弹，不经过 WebView，
/// 前端文案树够不着这里。字段用结构体保证漏翻译编译期报错。
pub struct UpdatePromptText {
    /// 发现新版本：正文（{version} 新版本号，{current} 当前版本，{notes} 更新说明）
    pub available_body: &'static str,
    /// 发现新版本：标题
    pub available_title: &'static str,
    /// 「立即更新」按钮（点了直接下载安装）
    pub install: &'static str,
    /// 「稍后」按钮（关闭弹窗，更新仍留在面板卡片里）
    pub later: &'static str,
    /// 已是最新版本：正文（{current} 当前版本）
    pub up_to_date_body: &'static str,
    /// 已是最新版本：标题
    pub up_to_date_title: &'static str,
    /// 单个「好」按钮
    pub ok: &'static str,
    /// 检查失败：标题
    pub error_title: &'static str,
    /// 检查失败：正文（{err} 原始错误，本地化前后缀）
    pub error_body: &'static str,
    /// 更新说明过长时的截断提示
    pub notes_more: &'static str,
}

const PROMPT_ZH: UpdatePromptText = UpdatePromptText {
    available_body: "发现新版本 v{version}（当前 v{current}）。\n\n{notes}",
    available_title: "发现新版本",
    install: "立即更新",
    later: "稍后",
    up_to_date_body: "已是最新版本（v{current}），无需更新。",
    up_to_date_title: "检查更新",
    ok: "好",
    error_title: "检查更新失败",
    error_body: "检查更新失败：{err}",
    notes_more: "…（更多说明见 Release 页面）",
};

const PROMPT_EN: UpdatePromptText = UpdatePromptText {
    available_body: "Version {version} is available (current v{current}).\n\n{notes}",
    available_title: "Update Available",
    install: "Install now",
    later: "Later",
    up_to_date_body: "You're up to date (v{current}). No update needed.",
    up_to_date_title: "Check for Updates",
    ok: "OK",
    error_title: "Update Check Failed",
    error_body: "Update check failed: {err}",
    notes_more: "… (see the Release page for more)",
};

fn prompt_text(locale: Locale) -> &'static UpdatePromptText {
    match locale {
        Locale::Zh => &PROMPT_ZH,
        Locale::En => &PROMPT_EN,
    }
}

/// 截断更新说明：release notes 可能很长（多段 changelog），原生弹窗塞不下。
/// 按 char 边界截断，避免把多字节中文切断；超限追加本地化的省略提示。
fn truncate_notes(notes: &str, max_chars: usize, more: &str) -> String {
    let trimmed = notes.trim();
    let mut out = String::new();
    for (i, ch) in trimmed.chars().enumerate() {
        if i >= max_chars {
            out.push('\n');
            out.push_str(more);
            break;
        }
        out.push(ch);
    }
    out
}

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

/// 托盘「检查更新」入口：检查后弹原生对话框反馈结果（WorkBuddy 式交互）。
///
/// 为什么单独做这一步：托盘点击时面板窗口多半是隐藏的，而 `check_and_notify`
/// 的结果只通过事件发给前端卡片——用户点了菜单却看不到任何反馈。这里在 Rust
/// 侧直接弹系统原生对话框，保证「点了必有回应」；同时 `check_and_notify` 的
/// 事件照常发出，面板开着时卡片也同步更新，两套反馈互不干扰。
///
/// 三种结果：
/// - 发现新版本：弹「立即更新 / 稍后」。点「立即更新」直接下载安装并重启；
///   点「稍后」关闭弹窗，待更新仍留在 `UpdateState` + 面板卡片里。
/// - 已是最新：弹单行「好」提示。
/// - 出错：弹错误提示（不暴露过多细节，message 用本地化错误串）。
pub async fn check_and_prompt(app: &AppHandle) {
    let locale = crate::i18n::current(app);
    let text = prompt_text(locale);

    let result = check_and_notify(app, true).await;

    // 弹窗回调在独立线程执行（见 tauri-plugin-dialog desktop::show_message_dialog），
    // 不能在里面直接 await；构造好 builder 后交给 show 的闭包，闭包内再 spawn。
    match result {
        Ok(payload) if payload["status"] == "available" => {
            let version = payload["version"].as_str().unwrap_or("");
            let current = payload["currentVersion"].as_str().unwrap_or("");
            let notes = payload["notes"].as_str().unwrap_or("");
            let notes = if notes.trim().is_empty() {
                String::new()
            } else {
                format!("\n{}", truncate_notes(notes, 400, text.notes_more))
            };
            let body = fill(
                text.available_body,
                &[
                    ("version", version),
                    ("current", current),
                    ("notes", &notes),
                ],
            );

            let install_handle = app.clone();
            app.dialog()
                .message(body)
                .title(text.available_title)
                .kind(MessageDialogKind::Info)
                .buttons(MessageDialogButtons::OkCancelCustom(
                    text.install.to_string(),
                    text.later.to_string(),
                ))
                .show(move |confirmed| {
                    if confirmed {
                        let handle = install_handle.clone();
                        tauri::async_runtime::spawn(async move {
                            if let Err(e) = update_install(handle).await {
                                eprintln!("[updater] 弹窗「立即更新」安装失败: {e}");
                            }
                        });
                    }
                });
        }
        Ok(_) => {
            // up-to-date（executable version 从插件读，这里取包版本展示）
            let current = app.package_info().version.to_string();
            let body = fill(text.up_to_date_body, &[("current", &current)]);
            app.dialog()
                .message(body)
                .title(text.up_to_date_title)
                .kind(MessageDialogKind::Info)
                .buttons(MessageDialogButtons::OkCustom(text.ok.to_string()))
                .show(|_| {});
        }
        Err(e) => {
            let body = fill(text.error_body, &[("err", &e)]);
            app.dialog()
                .message(body)
                .title(text.error_title)
                .kind(MessageDialogKind::Error)
                .buttons(MessageDialogButtons::OkCustom(text.ok.to_string()))
                .show(|_| {});
        }
    }
}

/// 启动静默检查：延迟 5 s（避开启动期采样高峰），失败不打扰用户。
///
/// ┌─ ⚠️ D2 口径文件（roadmap 2027h1 §5.1，勿动）────────────────────────┐
/// │ 本函数的**调用时机**就是 D2（启动事件数）的指标定义本身：每次 GUI      │
/// │ 启动请求一次 `latest.json`，其下载计数 = D2 = 零遥测下唯一干净的装机   │
/// │ 代理（已穿过 Gatekeeper；CLI/MCP 用户不产生 D2）。                     │
/// │ 任何改动（时机、开关、节流、跳过）都等于**修改指标定义**：必须同步更新  │
/// │ roadmap §5.1 并重设 D2 基线，不接受静默变更。否则看板会表现为「启动数  │
/// │ 下滑」并被误读为流失，误触发门禁 B 证伪分支（RK-8）。                  │
/// └──────────────────────────────────────────────────────────────────┘
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
