/** 与 prune-core 的 serde 结构一一对应（camelCase）。改这里必须同步 Rust 侧。 */

export type MemSource = "footprint" | "rss";
export type PressureLevel = "normal" | "warning" | "critical";

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
}

export interface FamilyMem {
  name: string;
  processCount: number;
  rss: number;
  footprint: number | null;
  swapped: number | null;
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
