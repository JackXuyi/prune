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

export interface CacheGroup {
  key: string;
  label: string;
  root: string;
  level: Level;
  totalBytes: number;
  entryCount: number;
  /** 聚合分组（root 是 home）时为 false：只统计，不提供一键清理 */
  cleanable: boolean;
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

export interface DiskReport {
  scannedAt: number;
  scanElapsedMs: number;
  groups: CacheGroup[];
  dup: DupSummary | null;
  totals: Totals;
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
}

export const LEVEL_META: Record<Level, { label: string; tone: "accent" | "warn" | "danger" | "neutral" }> = {
  l1: { label: "🟢 零后悔", tone: "accent" },
  l2: { label: "🟢 可再生", tone: "accent" },
  l3: { label: "🟡 需判断", tone: "warn" },
  l4: { label: "🔴 只给命令", tone: "danger" },
};
