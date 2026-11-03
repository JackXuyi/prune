/** 与 prune-core 的 serde 结构一一对应（camelCase）。改这里必须同步 Rust 侧。 */

/** 深探口径：macOS footprint / Windows Private Bytes / 降级 RSS */
export type MemSource = "footprint" | "private" | "rss";

/** 读数来源的展示文案：两个平台拿到的是不同指标，不能合并写成「footprint」 */
export function memSourceLabel(source: MemSource): string {
  switch (source) {
    case "footprint":
      return "/usr/bin/footprint --swapped";
    case "private":
      return "Private Bytes（不含换出）";
    default:
      return "sysinfo RSS";
  }
}
export type PressureLevel = "normal" | "warning" | "critical";
/** 压力读数口径：kernel = macOS 内核综合信号；occupancy = Windows 占用率百分比 */
export type PressureMetric = "kernel" | "occupancy";
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
  /** 组内 CPU 之和（单核口径）；null = 全部成员未知 */
  cpu?: number | null;
}

export interface Pressure {
  level: PressureLevel;
  /** 原始值：kernel 口径为 1/2/4，occupancy 口径为 0–100 */
  raw: number;
  metric: PressureMetric;
}

export interface Thrash {
  swapinsPerSec: number;
  swapoutsPerSec: number;
}

export interface SwapInfo {
  total: number;
  used: number;
  free: number;
  /** macOS 的 swapfile 个数；null = 该平台无此口径（Windows），UI 隐藏 */
  fileCount: number | null;
  /** macOS 今日新增 swapfile；null = 该平台无此口径（Windows），UI 隐藏 */
  newToday: number | null;
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
  /** 近 10 s 均值 CPU（单核口径，可 >100%）；null = 首见/采集失败（**不是 0**，UI 显示「—」） */
  cpu?: number | null;
  /** 累计 CPU 时间（CPU-ms，多线程下可大于墙钟时长） */
  cpuTimeMs?: number;
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
  /** 成员 CPU 之和（单核口径，可 >100%）；null = 全部成员未知；部分未知时是下界（见 cpuCovered） */
  cpu?: number | null;
  /** 有 CPU 读数的成员数；< processCount 时 cpu 为下界 */
  cpuCovered?: number;
}

/** 1/5/15 min 排队长度（macOS；Windows 无此口径） */
export interface CpuLoad {
  one: number;
  five: number;
  fifteen: number;
}

/**
 * 整机 CPU 读数（M5）。usage 是每核平均（0–100），与进程/家族的单核口径
 * 不同且不守恒——UI 绝不能把两者并排呈现让用户做减法。
 */
export interface CpuInfo {
  usage: number;
  cores: number;
  /** null = 平台不支持（Windows）；load 含 I/O，不能换算成使用率 */
  load: CpuLoad | null;
  /** 最近 60 点整机 CPU（10 min @10 s），sparkline 用 */
  history: number[];
  at: number;
}

export interface MemSnapshot {
  physicalTotal: number;
  /** 已用：macOS 排除可回收缓存 / Windows 为总量 − 可用；旧快照缺省 undefined，UI 按「未知」处理 */
  physicalUsedBytes?: number;
  /** 可用内存（macOS Pages free / Windows AvailPhys） */
  freeBytes: number;
  /** 压缩内存；null = 该平台无此口径（Windows），UI 隐藏该卡片 */
  compressorBytes: number | null;
  swap: SwapInfo;
  /** 内存压力；null = 读不到，UI 隐藏 */
  pressure: Pressure | null;
  /** 换页颠簸速率；null = 该平台没有换页计数，换出相关读数一并隐藏 */
  thrash: Thrash | null;
  families: FamilyMem[];
  topSwapped: ProcMem[];
  fastAt: number;
  deepUpdatedAt: number | null;
  deepAvailable: boolean;
  /** B5 泄漏嫌疑判定（M3）。note 非空 = 无法判定，不能当成「没有泄漏」 */
  leaks: LeakReport;
  /** 整机 CPU（M5）；null = 首轮/采集失败，旧快照缺省 undefined，UI 自动隐藏 */
  cpu?: CpuInfo | null;
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
