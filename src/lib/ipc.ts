/** 与 Rust 侧 IPC 命令的薄封装（契约见 docs/m2-memory-plan.md §6 / docs/m1-disk-plan.md §8）。 */
import { invoke } from "@tauri-apps/api/core";

import type { DiskReport, ReclaimItem, ReclaimPreview, ReclaimReport } from "../types/disk";
import type { MemSnapshot, Point, ProcMem } from "../types/mem";

/** 浏览器里跑 vite 时没有任何 Tauri 注入，需要显式兜底而不是静默失败 */
export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export function memSnapshot(): Promise<MemSnapshot | null> {
  return invoke<MemSnapshot | null>("mem_snapshot");
}

export function memProcesses(): Promise<ProcMem[]> {
  return invoke<ProcMem[]>("mem_processes");
}

export function memHistory(pid: number, minutes: number): Promise<Point[]> {
  return invoke<Point[]>("mem_history", { pid, minutes });
}

export function memRefresh(): Promise<void> {
  return invoke<void>("mem_refresh");
}

/** 打开主窗口；传 pid 时主窗口会自动选中该进程，传 tab 时直接切到对应 tab */
export function openMain(pid?: number, tab?: "apps" | "overview" | "disk"): Promise<void> {
  return invoke<void>("open_main", { pid: pid ?? null, tab: tab ?? null });
}

// ---------------------------------------------------------------- 磁盘（M1）

export function diskReport(): Promise<DiskReport | null> {
  return invoke<DiskReport | null>("disk_report");
}

/** 触发一轮扫描（单飞：已有扫描在跑时直接返回） */
export function diskScan(): Promise<void> {
  return invoke<void>("disk_scan");
}

/** dry-run：只生成清单，不动任何文件 */
export function reclaimPreview(keys: string[]): Promise<ReclaimPreview> {
  return invoke<ReclaimPreview>("reclaim_preview", { keys });
}

/** 执行清理（走废纸篓）；成功后 Rust 侧会自动重扫并发 disk://done */
export function reclaimExecute(items: ReclaimItem[], confirmed: boolean): Promise<ReclaimReport> {
  return invoke<ReclaimReport>("reclaim_execute", { items, confirmed });
}
