//! GitHub 源自动升级（docs/updater-plan.md）。
//!
//! 设计要点：
//! - 更新逻辑全部落在 Rust 侧（`tauri-plugin-updater`），前端通过自定义命令
//!   调用即可——**自定义命令不走插件权限门**，capabilities 保持最小权限
//!   （仍只有 `core:default`），不需要为 updater JS API 放权。
//! - 托盘「检查更新」与面板入口复用同一套 `check_and_notify`，结果通过
//!   事件广播给所有窗口；面板隐藏时事件照收，重开面板时还有
//!   `update_pending` 兜底查询，状态不丢。
//! - 手动检查的「点了必有回应」由独立 `update` WebView 窗口承载（原生
//!   dialog 是纯文本，渲染不了 markdown）：Rust 把结果写进 `UpdateDialogState`
//!   后 show 窗口，窗口 mount 后经 `update_dialog_data` 拉取，配合 `update://dialog`
//!   事件刷新——不依赖事件重放，规避「窗口还没 listen 事件就发」的竞态。
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

/// 「检查更新」弹窗窗口拉取的结构化结果（serde 默认字段即 kebab-case 的
/// Rust 字段名，这里显式 rename 成 camelCase 对齐前端 TS 类型）。
///
/// `notes_md` 是 markdown 原文：发现新版时来自 latest.json 的 `notes`（CI 已
/// 改写为 CHANGELOG 本版段），已是最新时来自本地 CHANGELOG 摘要。前端用
/// 轻量 markdown 渲染器渲染——这正是换掉纯文本原生 dialog 的意义。
#[derive(Clone, serde::Serialize)]
#[serde(
    rename_all = "camelCase",        // 变体名 → status 值（upToDate / available ...）
    rename_all_fields = "camelCase", // struct variant 的字段 → data 内的 key
    tag = "status",
    content = "data"
)]
pub enum UpdateDialogPayload {
    /// 正在检查（窗口刚打开、网络检查进行中；点击后立即显示，不等结果）
    Checking,
    /// 发现新版本
    Available {
        version: String,
        current_version: String,
        notes_md: Option<String>,
        date: Option<String>,
    },
    /// 已是最新（附当前版本说明）
    UpToDate {
        current_version: String,
        notes_md: Option<String>,
    },
    /// 检查失败
    Error { message: String },
}

impl Default for UpdateDialogPayload {
    /// 窗口刚 mount、检查还没回来时的初始态：显示「正在检查」，而不是一个空错误框。
    fn default() -> Self {
        Self::Checking
    }
}

/// 最近一次手动检查的结果，供 `update` 窗口 mount 后主动拉取（curl 式 pull，
/// 不依赖事件重放）。窗口 show 之前写入，因此前端 mount 时一定能读到。
pub struct UpdateDialogState(Mutex<UpdateDialogPayload>);

impl UpdateDialogState {
    pub fn new() -> Self {
        Self(Mutex::new(UpdateDialogPayload::default()))
    }

    pub fn set(&self, payload: UpdateDialogPayload) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = payload;
    }

    pub fn get(&self) -> UpdateDialogPayload {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

impl Default for UpdateDialogState {
    fn default() -> Self {
        Self::new()
    }
}

pub const EVENT_AVAILABLE: &str = "update://available";
pub const EVENT_CHECK_RESULT: &str = "update://check-result";
pub const EVENT_PROGRESS: &str = "update://progress";
pub const EVENT_INSTALLED: &str = "update://installed";
/// 通知 `update` 窗口「有新结果了，来拉一次」（窗口 mount 后已 listen）。
pub const EVENT_DIALOG: &str = "update://dialog";

/// CHANGELOG.md 编译期嵌入：作为「已是最新版本」弹窗的更新说明数据源。
///
/// 为什么不用 `update.body`：插件在发现新版时才有 notes，且那来自 latest.json 的
/// `notes` 字段（历史上是静态下载表模板，见 docs/updater-plan.md）；「已是最新」
/// 时 `check()` 直接返回 `None`，元数据全被丢弃。本地 CHANGELOG 是当前版本说明
/// 唯一可靠、零网络、零新依赖的来源。编译期嵌入也避开了运行时 resource_dir 在
/// dev 模式下读不到的问题。
const CHANGELOG: &str = include_str!("../../CHANGELOG.md");

/// 提取 CHANGELOG 中指定版本的段落（从 `## {version}` 到下一个 `## `）。
fn changelog_section<'a>(changelog: &'a str, version: &str) -> Option<&'a str> {
    let marker = format!("## {version}");
    let start = changelog.find(&marker)?;
    let rest = &changelog[start + marker.len()..];
    let end = rest.find("\n## ").unwrap_or(rest.len());
    Some(rest[..end].trim())
}

/// 把 CHANGELOG 本版段转成 markdown 说明（交给前端渲染，保留完整层次）：
/// - `###` 小标题 → markdown 二级标题（`##`，在弹窗里比正文醒目一档）
/// - `-` bullet → 原样保留（含 `**加粗**` / `` `代码` `` / 链接），不再剥离标记
/// - 无冒号截断、无字符截断：窗口可滚动，长条目允许完整换行显示
///
/// 唯一的收窄是「只取本版段」：不把下一版本的内容带进来。返回 None 表示
/// CHANGELOG 里没有该版本段（调用方据此回退到无说明的文案）。
fn current_version_notes(changelog: &str, version: &str) -> Option<String> {
    let section = changelog_section(changelog, version)?;
    let mut lines: Vec<String> = Vec::new();
    for raw in section.lines() {
        let line = raw.trim_end();
        let trimmed = line.trim();
        if let Some(title) = trimmed.strip_prefix("### ") {
            // 小标题升一档：本版段在弹窗里自成一体，主题用 H2 更清晰
            lines.push(format!("## {title}"));
        } else if trimmed.starts_with("- ") {
            lines.push(trimmed.to_string());
        }
    }
    if lines.is_empty() {
        None
    } else {
        Some(lines.join("\n\n"))
    }
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

/// 托盘 / 设置页「检查更新」入口：点击即 show `update` 窗口，检查在后台跑。
///
/// 为什么换成 WebView 窗口而非原生 dialog：dialog 是纯文本，更新说明里的
/// markdown（标题层级、加粗、行内代码、链接）全部退化成难看的字符；用户要
/// 真正的 markdown 渲染 + 好看的样式。窗口在 tauri.conf.json 声明为
/// `visible:false`，随 App 启动创建，前端 mount 后一直 listen `update://dialog`。
///
/// 「点了就弹」的数据流（不等网络，先给反馈）：
/// 1. **同步**：写入 `Checking` 态 → show 窗口 → emit `EVENT_DIALOG`。前端
///    立即渲染「正在检查更新…」转圈，用户不必干等慢网络。
/// 2. **后台**：spawn 检查（结果已写入 `UpdateState` / 已广播既有事件），
///    组装 `UpdateDialogPayload` 写回 `UpdateDialogState`。
/// 3. 再次 emit `EVENT_DIALOG`，窗口重新 pull 并按结果渲染：发现新版 →
///    立即更新 / 稍后；已是最新 → 好；出错 → 好。
///
/// 窗口 mount 时会主动 pull 一次兜底，规避「窗口还没 listen 事件就发」的
/// 竞态；检查至少几百 ms，足以让窗口完成 mount + listen。
///
/// 三种结果都由前端窗口承载文案（i18n `update` namespace），Rust 只给结构化
/// 数据 + markdown 原文，不再维护一份 Rust 侧的文案树。
pub fn check_and_prompt(app: &AppHandle) {
    // 1. 立即反馈：checking 态 + show 窗口（全同步，不碰网络）
    if let Some(state) = app.try_state::<std::sync::Arc<UpdateDialogState>>() {
        state.set(UpdateDialogPayload::Checking);
    }
    show_update_window(app);

    // 2. 后台检查：结果出来后写回 state 并 emit 通知窗口刷新
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let result = check_and_notify(&handle, true).await;

        let payload = match result {
            Ok(p) if p["status"] == "available" => {
                let notes = p["notes"]
                    .as_str()
                    .filter(|s| !s.trim().is_empty())
                    .map(str::to_string);
                UpdateDialogPayload::Available {
                    version: p["version"].as_str().unwrap_or("").to_string(),
                    current_version: p["currentVersion"].as_str().unwrap_or("").to_string(),
                    notes_md: notes,
                    date: p["date"].as_str().map(str::to_string),
                }
            }
            Ok(_) => {
                // 已是最新：notes 只能本地取（插件 check() 返回 None 时元数据全丢）。
                // 版本用编译期常量，避免运行时 package_info 取不到导致显示成空 "v"。
                let current = env!("CARGO_PKG_VERSION").to_string();
                UpdateDialogPayload::UpToDate {
                    notes_md: current_version_notes(CHANGELOG, &current),
                    current_version: current,
                }
            }
            Err(e) => UpdateDialogPayload::Error { message: e },
        };

        if let Some(state) = handle.try_state::<std::sync::Arc<UpdateDialogState>>() {
            state.set(payload);
        }
        // 窗口此时已 mount 且在 listen，emit 触发它重新 pull 拿到结果
        let _ = handle.emit(EVENT_DIALOG, ());
    });
}

/// show 更新窗口 + 置前；失败只记日志（不影响检查本身的事件契约）。
fn show_update_window(app: &AppHandle) {
    let Some(win) = app.get_webview_window("update") else {
        eprintln!("[updater] 未找到 update 窗口（tauri.conf.json 应声明 label=update）");
        return;
    };
    if let Err(e) = win.show() {
        eprintln!("[updater] show update 窗口失败: {e}");
        return;
    }
    let _ = win.set_focus();
    // 通知已 mount 的窗口拉取新结果；窗口若恰好没监听到，mount 时的主动 pull 兜底
    let _ = app.emit(EVENT_DIALOG, ());
}

/// 更新窗口拉取最近一次检查结果（前端 mount 后调用）。
#[tauri::command]
pub fn update_dialog_data(app: AppHandle) -> UpdateDialogPayload {
    match app.try_state::<std::sync::Arc<UpdateDialogState>>() {
        Some(state) => state.get(),
        None => UpdateDialogPayload::default(),
    }
}

/// 设置页「检查更新」入口：与托盘完全复用同一套「立即弹窗 + 后台检查」逻辑。
/// 暴露成 command 供前端调用；弹窗由 `check_and_prompt` 负责，这里只转发。
/// 注意：`check_and_prompt` 是同步的（内部 spawn 后台任务），command 立即返回，
/// 前端 `await` 不会卡在网络检查上。
#[tauri::command]
pub fn check_update_dialog(app: AppHandle) {
    check_and_prompt(&app);
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 用真实 CHANGELOG 当前版本段验证：段落提取到下一个 `## ` 为止，
    /// 不把 0.13.0 的内容带进来。
    #[test]
    fn changelog_section_stops_at_next_version() {
        let section = changelog_section(CHANGELOG, "0.13.1").expect("应有 0.13.1 段");
        assert!(section.contains("修 0.13.0 的 Windows 构建失败"));
        assert!(!section.contains("Wave 1"), "不应跨越到下一个版本段");
        assert!(changelog_section(CHANGELOG, "0.13.0").is_some());
        assert!(changelog_section(CHANGELOG, "9.9.9").is_none());
    }

    /// markdown 摘要：小标题升为 H2、bullet 原样保留（含加粗/反引号），
    /// 不跨版本、不硬截断——交给前端 markdown 渲染器完整呈现。
    #[test]
    fn current_version_notes_is_markdown_and_bounded() {
        let notes = current_version_notes(CHANGELOG, "0.13.1").expect("应有摘要");
        // 小标题升为 H2
        assert!(notes.contains("## 修 0.13.0 的 Windows 构建失败"));
        // bullet 的 markdown 标记保留（不再剥离）
        assert!(notes.contains("- **"));
        assert!(notes.contains("`panel_fit_height`"));
        // 不跨到下一版本段
        assert!(!notes.contains("Wave 1"), "不应带下一版本内容");
        // 只有本版段：1 个 H2 + 3 个 bullet
        assert_eq!(notes.lines().filter(|l| l.starts_with("- ")).count(), 3);
        assert_eq!(notes.lines().filter(|l| l.starts_with("## ")).count(), 1);
    }

    /// CHANGELOG 缺段时返回 None（前端据此显示「暂无说明」兜底）。
    #[test]
    fn current_version_notes_missing_returns_none() {
        assert!(current_version_notes(CHANGELOG, "9.9.9").is_none());
    }

    /// checking 态（含 default）序列化为 {"status":"checking"}，前端据此渲染转圈。
    #[test]
    fn checking_payload_serializes_as_status_only() {
        let v = serde_json::to_value(UpdateDialogPayload::Checking).expect("should serialize");
        assert_eq!(v, serde_json::json!({ "status": "checking" }));
        let v = serde_json::to_value(UpdateDialogPayload::default()).expect("should serialize");
        assert_eq!(v, serde_json::json!({ "status": "checking" }));
    }

    /// 结果态仍按 tag/content 拆分，前端 discriminated union 依此收窄。
    #[test]
    fn result_payload_serializes_tag_and_data() {
        let v = serde_json::to_value(UpdateDialogPayload::UpToDate {
            current_version: "0.13.1".into(),
            notes_md: Some("## hi".into()),
        })
        .expect("should serialize");
        assert_eq!(v["status"], "upToDate");
        assert_eq!(v["data"]["currentVersion"], "0.13.1");
        assert_eq!(v["data"]["notesMd"], "## hi");
    }
}
