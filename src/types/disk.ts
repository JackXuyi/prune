/** 磁盘可回收报告类型（镜像 prune_core::disk，serde camelCase）。 */
import { dynamicText, i18nFor } from "../i18n";
import type { Locale, NoParamKey } from "../i18n/types";

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

/** F9 项目体检结论：优先级 僵尸 > 偏胖 > 久未重装 > 健康 */
export type ProjectVerdict = "healthy" | "stale" | "heavy" | "zombie";

/** 一个项目的依赖账本（F9/M24）：只陈述事实，不给「可删」判决 */
export interface ProjectHealth {
  /** 展示名：相对 home 的项目路径 */
  name: string;
  /** 项目根绝对路径（点击定位目标） */
  root: string;
  /** node_modules 绝对路径 */
  depsPath: string;
  depsBytes: number;
  /** 依赖上次安装距今天数（node_modules mtime） */
  depsAgeDays: number;
  /** 项目最后活动距今天数；null = 没找到任何标记文件 */
  activityAgeDays: number | null;
  /** 同时存在的包管理器（npm/yarn/pnpm/bun） */
  lockfiles: string[];
  /** ≥2 个锁文件并存 */
  multiManager: boolean;
  verdict: ProjectVerdict;
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
  /** F9 项目体检（旧报告缺省为空数组） */
  projects?: ProjectHealth[];
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

/**
 * 「没进清单」的说明（镜像 `prune_core::reclaim::ReclaimNote`，内部 tag = code）。
 *
 * 后端只下发 code + 参数，**不带句子**：说明里的组名要按当前语言查
 * `disk.cache.<key>`，后端拼好的中文句子在英文界面里就是漏翻。
 */
export type ReclaimNote =
  | { code: "scanPending" }
  | { code: "missingGroup"; key: string }
  | { code: "highRisk"; key: string }
  | { code: "scattered"; key: string; n: number }
  | { code: "rosettaPending"; n: number }
  | { code: "nothingToDo" };

export interface ReclaimPreview {
  items: ReclaimItem[];
  totalBytes: number;
  /** L3 或超 20 GB 熔断时为 true */
  needsConfirm: boolean;
  notes: ReclaimNote[];
}

export interface ReclaimReport {
  moved: number;
  bytes: number;
  failed: { path: string; reason: string }[];
  /** 校验阶段发现已不存在的条目数（报告过期，自动跳过） */
  skipped: number;
}

export const LEVEL_META: Record<Level, { labelKey: NoParamKey; dot: string; tone: "accent" | "warn" | "danger" | "neutral" }> = {
  l1: { labelKey: "disk.level.l1", dot: "🟢", tone: "accent" },
  l2: { labelKey: "disk.level.l2", dot: "🟢", tone: "accent" },
  l3: { labelKey: "disk.level.l3", dot: "🟡", tone: "warn" },
  l4: { labelKey: "disk.level.l4", dot: "🔴", tone: "danger" },
};

/**
 * 缓存分组名的展示文案：按后端稳定 key 查 `disk.cache.<key>`，查不到回退后端 label。
 *
 * 为什么允许动态 key：缓存条目的集合由 `whitelist.rs` 决定，前端不可能靠
 * `as const` 提前枚举（枚举一次就多一处漏改点）。回退链保证「catalog 缺 key」
 * 不会显示成裸 key；`scripts/check-i18n.mjs` 会解析 Rust 源码逐条核对。
 */
export function cacheLabel(group: { key: string; label: string }, locale: Locale): string {
  return dynamicText(locale, `disk.cache.${group.key}`) ?? group.label;
}

/**
 * 把一条「没进清单」的说明渲染成当前语言的句子。
 *
 * 组名优先从本次报告的 groups 里取（走 `cacheLabel`，与分组列表共用同一套译名）；
 * 报告过期、组已不在表里时退回 `disk.cache.<key>`，再退回 key —— 与 Rust 侧
 * `ReclaimNote::text_zh` 是同一条回退链，只是那边的末位是报告里的 label。
 *
 * emoji 留在代码里（spec §3.7：不进 catalog）：它是标记不是文案。
 */
export function reclaimNoteText(
  note: ReclaimNote,
  locale: Locale,
  groups: CacheGroup[],
): string {
  const { t } = i18nFor(locale);
  const label = (key: string): string => {
    const g = groups.find((x) => x.key === key);
    return g ? cacheLabel(g, locale) : (dynamicText(locale, `disk.cache.${key}`) ?? key);
  };
  switch (note.code) {
    case "scanPending":
      return t("disk.preview.note.scanPending");
    case "missingGroup":
      return t("disk.preview.note.missingGroup", { group: label(note.key) });
    case "highRisk":
      return `🔴 ${t("disk.preview.note.highRisk", { group: label(note.key) })}`;
    case "scattered":
      return t("disk.preview.note.scattered", { group: label(note.key), n: note.n });
    case "rosettaPending":
      return t("disk.preview.note.rosettaPending", { n: note.n });
    case "nothingToDo":
      return t("disk.preview.note.nothingToDo");
  }
}
