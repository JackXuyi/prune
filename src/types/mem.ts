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
  /** B5 泄漏嫌疑判定（M3）。note 非空 = 无法判定，不能当成「没有泄漏」 */
  leaks: LeakReport;
}

/** 一个命中进程的证据。只有事实字段，没有结论性形容词 */
export interface LeakItem {
  pid: number;
  startTime: number;
  family: string;
  name: string;
  /** 窗口末值（footprint 口径） */
  footprint: number;
  /** 窗口净增长 */
  growth: number;
  /** growth / 窗口起点值 */
  growthRatio: number;
  windowSecs: number;
  points: number;
  slopeMbPerHour: number;
  r2: number;
  /** 最大单点跃升 / 净增长；越低越像「持续爬升」 */
  stepRatio: number;
  gcRuntime: boolean;
}

export interface LeakReport {
  items: LeakItem[];
  /** 参与判定的进程数 */
  observed: number;
  /** 序列不够长（首次运行 / 刚启动的进程） */
  insufficient: number;
  /** 良性名单排除（浏览器 / VM / 系统进程 / 自身） */
  benign: number;
  /** 数值条件未达标 */
  below: number;
  windowSecs: number;
  updatedAt: number | null;
  /** 无法判定的原因；null = 判定正常运行 */
  note: string | null;
}

/** 某家族里命中的进程（家族行徽标用） */
export function leaksOfFamily(report: LeakReport | null | undefined, family: string): LeakItem[] {
  return (report?.items ?? []).filter((i) => i.family === family);
}

/** 一组命中进程的净增长合计 */
export function totalGrowth(items: LeakItem[]): number {
  return items.reduce((sum, i) => sum + i.growth, 0);
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
