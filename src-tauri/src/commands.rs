//! IPC 命令（T-207）：只做读取与转发，业务逻辑都在 prune-core / sampler。
//!
//! 契约见 docs/m2-memory-plan.md §6：
//! - `mem_snapshot`   取内存中最新快照（零 IO，前端打开即用）
//! - `mem_processes`  全量进程明细（家族聚合前）
//! - `mem_history`    某进程的序列（读 SQLite）
//! - `mem_refresh`    手动触发一轮快照 + 深探

use std::sync::Arc;

use prune_core::db::Point;
use prune_core::mem::{MemSnapshot, ProcMem};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::sampler::{self, MemState};

#[tauri::command]
pub fn mem_snapshot(state: State<'_, Arc<MemState>>) -> Option<MemSnapshot> {
    state
        .snapshot
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

#[tauri::command]
pub fn mem_processes(state: State<'_, Arc<MemState>>) -> Vec<ProcMem> {
    state
        .processes
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

#[tauri::command]
pub fn mem_history(
    state: State<'_, Arc<MemState>>,
    pid: i32,
    minutes: u32,
) -> Vec<Point> {
    let since = now_secs() - (minutes.max(1) as i64) * 60;
    state.history(pid, since, 2000)
}

/// 手动刷新：先出一轮快照，再尝试深探。
/// 深探自带单飞闸门——若定时器轮正在跑，这里的 `deep_round` 会直接跳过并返回，
/// 不会出现两轮 footprint 并行；本轮结果仍会由定时器轮的 `mem://deep` 事件送达。
#[tauri::command]
pub async fn mem_refresh(app: AppHandle) -> Result<(), String> {
    sampler::fast_round(&app).await?;
    sampler::deep_round(&app).await?;
    Ok(())
}

/// 面板点击受害者 → 打开主窗口并选中该进程（附带收起面板）
#[tauri::command]
pub fn open_main(app: AppHandle, pid: Option<i32>) {
    if let Some(panel) = app.get_webview_window("panel") {
        let _ = panel.hide();
    }
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.show();
        let _ = main.set_focus();
    }
    if let Some(pid) = pid {
        let _ = app.emit("mem://select", pid);
    }
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
