/** 与 prune-core 的 serde 结构一一对应（camelCase）。改这里必须同步 Rust 侧。 */

export type MemSource = "footprint" | "rss";
export type PressureLevel = "normal" | "warning" | "critical";
/** 工作区归属来源，置信度从高到低 */
export type WorkspaceSource = "handle" | "label" | "inherited";

export interface WorkspaceRef {
  path: string;
  label: string;
  source: WorkspaceSource;
}

export interface WorkspaceMem {
  path: string;
  label: string;
  source: WorkspaceSource;
  processCount: number;
  rss: number;
  footprint: number | null;
}

export interface Pressure {
  level: PressureLevel;
  /** 原始 kern.memorystatus_vm_pressure_level（1/2/4） */
  raw: number;
}

export interface Thrash {
  swapinsPerSec: number;
  swapoutsPerSec: number;
}

export interface SwapInfo {
  total: number;
  used: number;
  free: number;
  fileCount: number;
  newToday: number;
}

export interface ProcMem {
  pid: number;
  ppid: number;
  name: string;
  exe: string | null;
  family: string;
  rss: number;
  footprint: number | null;
  swapped: number | null;
  source: MemSource;
  startTime: number;
  /** 工作区归属（M2.5）；null = 未归属，UI 必须如实显示 */
  workspace: WorkspaceRef | null;
}

export interface FamilyMem {
  name: string;
  processCount: number;
  /** 已完成 footprint 深探的子进程数；< processCount 表示总量是下界估算 */
  deepCovered: number;
  rss: number;
  footprint: number | null;
  swapped: number | null;
  /** 按工作区分组（仅编辑器类家族非空），按总量降序 */
  workspaces: WorkspaceMem[];
  /** 未能归属到任何工作区的子进程数 */
  wsUnattributedCount: number;
  /** 未归属部分的总量（同 footprint 口径） */
  wsUnattributedFootprint: number | null;
}

export interface MemSnapshot {
  physicalTotal: number;
  compressorBytes: number;
  pagesFreeBytes: number;
  swap: SwapInfo;
  pressure: Pressure;
  thrash: Thrash;
  families: FamilyMem[];
  topSwapped: ProcMem[];
  fastAt: number;
  deepUpdatedAt: number | null;
  deepAvailable: boolean;
}

export interface Point {
  ts: number;
  rss: number;
  footprint: number | null;
  swapped: number | null;
}

/** 总量：有 footprint 用 footprint，否则退回 RSS（与 Rust 侧 total() 一致） */
export function totalOf(p: { footprint: number | null; rss: number }): number {
  return p.footprint ?? p.rss;
}

/** 换出占比 0~1 */
export function swappedRatio(p: {
  footprint: number | null;
  swapped: number | null;
}): number | null {
  if (p.footprint == null || p.swapped == null || p.footprint <= 0) return null;
  return p.swapped / p.footprint;
}

/** 全部子进程都已深探 → 绝对内存是精确值；否则只是下界估算 */
export function isExact(f: FamilyMem): boolean {
  return f.footprint != null && f.deepCovered === f.processCount;
}
