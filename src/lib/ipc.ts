/** 与 Rust 侧 IPC 命令的薄封装（契约见 docs/m2-memory-plan.md §6）。 */
import { invoke } from "@tauri-apps/api/core";

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

/** 打开主窗口；传 pid 时主窗口会自动选中该进程 */
export function openMain(pid?: number): Promise<void> {
  return invoke<void>("open_main", { pid: pid ?? null });
}
