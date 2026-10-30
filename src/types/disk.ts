/** 磁盘可回收报告类型（镜像 prune_core::disk，serde camelCase）。 */

/** 安全分级：L1/L2 可直接清理，L3 需确认，L4 只给命令 */
export type Level = "l1" | "l2" | "l3" | "l4";

/** A1 判定结果 */
export type Verdict = "deadweight" | "needsConfirm" | "keep";

export interface DeadweightItem {
  name: string;
  path: string;
  bytes: number;
  platform: string | null;
  verdict: Verdict;
}

export interface PlatformStat {
  platform: string;
  entryCount: number;
  bytes: number;
}

export interface DeadweightStats {
  /** 🟢 本机永远加载不了 */
  bytes: number;
  /** 本机原生可加载（darwin-arm64）——浪费比分母 */
  nativeBytes: number;
  /** 其余条目（普通包 + 待确认的 darwin-x64） */
  otherBytes: number;
  entryCount: number;
  byPlatform: PlatformStat[];
  /** 按体积降序 */
  items: DeadweightItem[];
  ratio: number | null;
}

/** A4/A5 明细条目（node_modules 目录 / 模拟器设备 / 浏览器二进制） */
export interface DetailItem {
  /** 展示名（相对 home 的项目路径 / 设备名 / chromium-1155 等） */
  name: string;
  path: string;
  bytes: number;
  /** 距上次改动的天数（>180 天 UI 标灰） */
  ageDays: number;
  /** 附加信息（Simulator：机型 · 运行时） */
  note: string | null;
}

export interface CacheGroup {
  key: string;
  label: string;
  root: string;
  level: Level;
  totalBytes: number;
  entryCount: number;
  /** 聚合分组（root 是 home）时为 false：整项不提供一键清理（明细可逐项） */
  cleanable: boolean;
  /** A4/A5 明细（旧报告缺省为空数组） */
  details?: DetailItem[];
  deadweight: DeadweightStats | null;
}

/** A3：多包管理器并存 */
export interface DupSummary {
  /** [展示名, 体积] */
  managers: [string, number][];
  totalBytes: number;
  advice: string;
}

export interface Totals {
  deadweight: number;
  l2: number;
  l3: number;
  l4: number;
  /** 主数字 = L1 + L2 + L3 */
  reclaimable: number;
  cacheTotal: number;
}

/** 卷容量（M4/S0，statvfs；statvfs 失败时为 null，UI 隐藏容量读数） */
export interface VolumeInfo {
  totalBytes: number;
  /** 剩余（含 purgeable，偏乐观） */
  availableBytes: number;
  /** 用户视角剩余（UI 主读数） */
  userAvailableBytes: number;
}

export interface DiskReport {
  scannedAt: number;
  scanElapsedMs: number;
  /** A7：本轮是否增量扫描（false = 全量；旧报告缺省为 false） */
  incremental?: boolean;
  groups: CacheGroup[];
  dup: DupSummary | null;
  totals: Totals;
  /** home 所在卷容量（M4/S0；旧报告缺省为 null） */
  volume?: VolumeInfo | null;
}

export interface ScanProgress {
  phase: string;
  done: number;
  total: number;
  bytesSoFar: number;
  elapsedMs: number;
}

export interface ReclaimItem {
  path: string;
  label: string;
  bytes: number;
  level: Level;
}

export interface ReclaimChild {
  name: string;
  bytes: number;
  /** 是否目录（决定图标与右键语义） */
  isDir?: boolean;
}

export interface ReclaimPreview {
  items: ReclaimItem[];
  totalBytes: number;
  /** L3 或超 20 GB 熔断时为 true */
  needsConfirm: boolean;
  notes: string[];
}

export interface ReclaimReport {
  moved: number;
  bytes: number;
  failed: { path: string; reason: string }[];
  /** 校验阶段发现已不存在的条目数（报告过期，自动跳过） */
  skipped: number;
}

export const LEVEL_META: Record<Level, { label: string; tone: "accent" | "warn" | "danger" | "neutral" }> = {
  l1: { label: "🟢 零后悔", tone: "accent" },
  l2: { label: "🟢 可再生", tone: "accent" },
  l3: { label: "🟡 需判断", tone: "warn" },
  l4: { label: "🔴 只给命令", tone: "danger" },
};
