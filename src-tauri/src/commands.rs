//! IPC 命令（T-207 / T-306）：只做读取与转发，业务逻辑都在 prune-core / sampler / scan。
//!
//! 契约见 docs/m2-memory-plan.md §6 与 docs/m1-disk-plan.md §8：
//! - `mem_snapshot`   取内存中最新快照（零 IO，前端打开即用）
//! - `mem_processes`  全量进程明细（家族聚合前）
//! - `mem_history`    某进程的序列（读 SQLite）
//! - `mem_refresh`    手动触发一轮快照 + 深探
//! - `disk_report`    取最新可回收报告
//! - `disk_scan`      触发一轮扫描（单飞，重复调用忽略）
//! - `reclaim_preview` dry-run：生成待清理清单
//! - `reclaim_execute` 执行清理（走废纸篓，护栏在 prune-core）

use std::sync::Arc;

use prune_core::db::Point;
use prune_core::disk::DiskReport;
use prune_core::mem::{MemSnapshot, ProcMem};
use prune_core::reclaim::{self, Limits, ReclaimItem, ReclaimPreview, ReclaimReport};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::sampler::{self, MemState};
use crate::scan::{self, DiskState};

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

/// 面板点击 → 打开主窗口并选中目标（可选指定 tab）。
/// 窗口显示逻辑统一走 tray::show_main（Accessory 下 show 不切 Space，需定位 + MoveToActiveSpace）。
#[tauri::command]
pub fn open_main(app: AppHandle, pid: Option<i32>, tab: Option<String>) {
    if let Some(panel) = app.get_webview_window("panel") {
        let _ = panel.hide();
    }
    crate::tray::show_main(&app);
    if let Some(pid) = pid {
        let _ = app.emit("mem://select", pid);
    }
    if let Some(tab) = tab {
        let _ = app.emit("ui://tab", tab);
    }
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------- 磁盘（M1）

#[tauri::command]
pub fn disk_report(state: State<'_, Arc<DiskState>>) -> Option<DiskReport> {
    state.report()
}

/// 手动触发扫描（A7 D-A7-2：手动 = 全量，用户盯着数字求证，准确性优先）。
/// 已有扫描在跑时直接返回 Ok（幂等），前端靠 `disk://done` 更新。
#[tauri::command]
pub async fn disk_scan(app: AppHandle) -> Result<(), String> {
    scan::scan_now(&app, true).await
}

/// dry-run：只生成清单，不动任何文件。
#[tauri::command]
pub fn reclaim_preview(state: State<'_, Arc<DiskState>>, keys: Vec<String>) -> ReclaimPreview {
    match state.report() {
        Some(report) => reclaim::preview(&keys, &report),
        None => ReclaimPreview {
            items: Vec::new(),
            total_bytes: 0,
            needs_confirm: false,
            notes: vec!["尚未完成扫描，请稍候或手动刷新".into()],
        },
    }
}

/// 执行清理：路径校验 / 级别闸门 / 熔断都在 prune-core，这里只转发。
/// `allowed` 从当前报告的明细表组装（A4：node_modules 逐项清理的报告派生白名单）。
/// 成功后自动增量重扫一轮，前端通过 `disk://done` 收到新报告。
#[tauri::command]
pub async fn reclaim_execute(
    app: AppHandle,
    state: State<'_, Arc<DiskState>>,
    items: Vec<ReclaimItem>,
    confirmed: bool,
) -> Result<ReclaimReport, String> {
    let home = scan::home_dir();
    // 报告派生白名单：只有当前报告里出现过的明细路径会被放行
    let allowed: Vec<String> = state
        .report()
        .map(|r| {
            r.groups
                .iter()
                .flat_map(|g| g.details.iter().map(|d| d.path.clone()))
                .collect()
        })
        .unwrap_or_default();

    let result = tauri::async_runtime::spawn_blocking(move || {
        reclaim::execute(&items, &home, confirmed, Limits::default(), &allowed)
    })
    .await
    .map_err(|e| format!("清理线程失败: {e}"))?
    .map_err(|e| e.to_string())?;

    eprintln!(
        "[m1-disk] 已移入废纸篓 {} 项 · {:.2} GB · 失败 {} 项",
        result.moved,
        result.bytes as f64 / 1e9,
        result.failed.len()
    );

    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = scan::scan_now(&handle, false).await {
            eprintln!("[m1-disk] 清理后增量重扫失败: {e}");
        }
    });

    Ok(result)
}
