/** 与 Rust 侧 IPC 命令的薄封装（契约见 docs/m2-memory-plan.md §6 / docs/m1-disk-plan.md §8）。 */
import { invoke } from "@tauri-apps/api/core";

import type { DiskReport, ReclaimChild, ReclaimItem, ReclaimPreview, ReclaimReport, VolumeInfo } from "../types/disk";
import type { AppDisk, BundleMap, BundleQuery } from "../types/appdisk";
import type { MemSnapshot, Point, ProcMem } from "../types/mem";
import type { NetSpeedSample, PortSnapshot } from "../types/net";

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
export function openMain(
  pid?: number,
  tab?: "stats" | "resources" | "apps" | "overview" | "disk" | "ports",
): Promise<void> {
  return invoke<void>("open_main", { pid: pid ?? null, tab: tab ?? null });
}

/** 结束进程：默认 SIGTERM（可优雅退出），force=true 时 SIGKILL */
export function killProcess(pid: number, force: boolean): Promise<void> {
  return invoke<void>("kill_process", { pid, force });
}

/** 在 Finder 中打开目录；传文件路径时定位并选中它 */
export function revealPath(path: string): Promise<void> {
  return invoke<void>("reveal_path", { path });
}

// ---------------------------------------------------------------- 磁盘（M1）

export function diskReport(): Promise<DiskReport | null> {
  return invoke<DiskReport | null>("disk_report");
}

/** 触发一轮扫描（单飞：已有扫描在跑时直接返回） */
export function diskScan(): Promise<void> {
  return invoke<void>("disk_scan");
}

/**
 * M9 统计 tab：home 所在卷容量（statvfs 直读，不依赖扫描）。
 * null = statvfs 失败（罕见），UI 显示「容量不可读」而不是 0。
 */
export function diskVolume(): Promise<VolumeInfo | null> {
  return invoke<VolumeInfo | null>("disk_volume");
}

/** dry-run：只生成清单，不动任何文件 */
export function reclaimPreview(keys: string[]): Promise<ReclaimPreview> {
  return invoke<ReclaimPreview>("reclaim_preview", { keys });
}

/** 一级子项体积明细（du 较慢，独立按需加载；预览弹层先出、内容后到） */
export function reclaimChildren(path: string): Promise<ReclaimChild[]> {
  return invoke<ReclaimChild[]>("reclaim_children", { path });
}

/** 执行清理（走废纸篓）；成功后 Rust 侧会自动重扫并发 disk://done */
export function reclaimExecute(items: ReclaimItem[], confirmed: boolean): Promise<ReclaimReport> {
  return invoke<ReclaimReport>("reclaim_execute", { items, confirmed });
}

// ---------------------------------------------------------------- 端口（M7）

/** 最新一轮端口快照；null = 还没扫过（**不是「没有端口」**） */
export function portSnapshot(): Promise<PortSnapshot | null> {
  return invoke<PortSnapshot | null>("port_snapshot");
}

/** 手动刷新（用户点了就该扫，绕过 30 s 定时；单飞闸门仍在） */
export function portRefresh(): Promise<void> {
  return invoke<void>("port_refresh");
}

/** 可见性门控：false 时定时器完全不采样，常驻开销 0 */
export function portSetActive(active: boolean): Promise<void> {
  return invoke<void>("port_set_active", { active });
}

// ---------------------------------------------------------------- 应用磁盘（M8）

/**
 * 测量一个应用的磁盘三层（本体 / 缓存 / 数据）。
 *
 * 这是**按需**调用：Rust 侧有 30 min TTL 缓存，`force` 才会重测。
 * 实测耗时 20 ms（Docker）到 4 s（Chrome profile 撞预算上限），
 * 所以调用方必须有「测量中」占位，不能假定它即时返回。
 */
export function appDisk(
  family: string,
  exe: string | null,
  force = false,
): Promise<AppDisk> {
  return invoke<AppDisk>("app_disk", { family, exe, force });
}

/**
 * M10：批量测一批应用的**本体**体积（卡片首屏用）。
 * 8 路并行 + 4 s/应用预算 + 30 min 缓存；超预算条目 `partial = true`。
 */
export function appBundles(items: BundleQuery[], force = false): Promise<BundleMap> {
  return invoke<BundleMap>("app_bundles", { items, force });
}

// ---------------------------------------------------------------- 网速（M10）

/** 最新一轮网速样本；null = 还没采过（**不是「没有流量」**） */
export function netSpeedSnapshot(): Promise<NetSpeedSample | null> {
  return invoke<NetSpeedSample | null>("net_speed_snapshot");
}

/** 可见性门控：false 时 1 s 采样循环连 nettop 都不 spawn，常驻开销 0 */
export function netSpeedSetActive(active: boolean): Promise<void> {
  return invoke<void>("net_speed_set_active", { active });
}
