//! 局域网设备 IPC（docs/m18-lan-devices-spec.md §10）。
//!
//! 扫描是**用户主动触发的短任务**（≤ 35 s：探测 ~5 s + mDNS 30s 窗口），
//! 用 `spawn_blocking` 包住，不堵 Tauri 的 async 运行时（对齐 `net_diag_run`）。
//!
//! 并发保护（spec §10）：`scanning` 原子门——已在扫时直接返回上次快照并附注
//! 「已有一轮扫描在进行」，**不叠加任务**（两次 30s 窗口叠加只会让用户更困惑）。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use prune_core::net::lan::{self, LanSnapshot};
use tauri::{AppHandle, Emitter, State};

/// 进度事件名（spec §10 契约）：`{ phase: "neighbors"|"probe"|"discover"|"merge", done, total }`
pub const PROGRESS_EVENT: &str = "lan://progress";

/// 最近一轮结果 + 扫描中标志
pub struct LanState {
    last: Mutex<Option<LanSnapshot>>,
    scanning: AtomicBool,
}

impl LanState {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            last: Mutex::new(None),
            scanning: AtomicBool::new(false),
        })
    }
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 暴露面：复用 ports::scan 的 `BindScope::Any` 口径（与 CLI `cmd_lan` 一致，spec §10）。
/// 端口扫描 ~1s，在 spawn_blocking 里跑，不额外占 IPC。
fn exposed_ports() -> Vec<u16> {
    let snap = prune_core::net::ports::scan(None);
    let mut ports: Vec<u16> = snap
        .entries
        .iter()
        .filter(|e| {
            e.exposure == prune_core::net::BindScope::Any && e.proto == prune_core::net::Proto::Tcp
        })
        .map(|e| e.port)
        .collect();
    ports.sort_unstable();
    ports.dedup();
    ports
}

/// 读最近一轮缓存快照，**不扫描**（进入分段时调，不会触发权限弹窗）。
/// `null` = 还没扫过。
#[tauri::command]
pub async fn lan_snapshot(state: State<'_, Arc<LanState>>) -> Result<Option<LanSnapshot>, String> {
    if let Some(snap) = state.last.lock().ok().and_then(|g| g.clone()) {
        return Ok(Some(snap));
    }
    // 进程重启后内存缓存为空：回落读盘（GUI 上次会话扫过的结果仍然可见）
    Ok(load_cache())
}

/// 把快照写入磁盘缓存（spec §12：MCP `lan_devices` 固定读这份文件）。
/// 写失败只影响 MCP 读数，不影响扫描返回。
fn persist_cache(snap: &LanSnapshot) {
    let Some(path) = prune_core::paths::lan_cache_path() else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(json) = serde_json::to_string_pretty(snap) {
        // 先写临时文件再改名：写一半的 JSON 会被 MCP 读到并当次失败
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, json).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
}

/// 读磁盘缓存。解析失败（写一半 / 手改坏）当作没缓存。
fn load_cache() -> Option<LanSnapshot> {
    let path = prune_core::paths::lan_cache_path()?;
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// 全段扫描一轮（探测 + 服务发现 + 合成），进度走 `lan://progress` 事件。
///
/// 门控语义：`scanning` 已置位时**不排队**，直接返回上次快照（可能为 null）
/// 并附一条说明——前端把它展示在 notes 里即可。
#[tauri::command]
pub async fn lan_scan(
    app: AppHandle,
    state: State<'_, Arc<LanState>>,
) -> Result<LanSnapshot, String> {
    if state.scanning.swap(true, Ordering::SeqCst) {
        let cached = state.last.lock().ok().and_then(|g| g.clone());
        let snap = cached.unwrap_or_else(|| LanSnapshot::unavailable("已有一轮扫描在进行"));
        return Ok(LanSnapshot {
            notes: {
                let mut n = snap.notes.clone();
                n.push("已有一轮扫描在进行，请等它完成。".to_string());
                n
            },
            ..snap
        });
    }

    let handle = app.clone();
    let res = tauri::async_runtime::spawn_blocking(move || {
        let exposed = exposed_ports();
        lan::snapshot_scan_cb(now_secs(), &exposed, &|phase, done, total| {
            // 单个事件发送失败（窗口已关）不影响扫描本身
            let _ = handle.emit(
                PROGRESS_EVENT,
                serde_json::json!({ "phase": phase, "done": done, "total": total }),
            );
        })
    })
    .await
    .map_err(|e| format!("扫描任务调度失败: {e}"));

    state.scanning.store(false, Ordering::SeqCst);

    let snap = res?;
    if let Ok(mut last) = state.last.lock() {
        *last = Some(snap.clone());
    }
    persist_cache(&snap);
    Ok(snap)
}

/// 平台能力（前端决定空态文案与按钮显隐）。不硬编码 navigator.platform，
/// 避免前端判断与后端注册面漂移（对齐 `net_diag_capability`）。
#[tauri::command]
pub fn lan_capability() -> serde_json::Value {
    serde_json::json!({ "scan": true })
}
