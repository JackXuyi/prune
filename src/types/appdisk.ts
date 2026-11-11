/** 与 prune-core::appdisk 一一对应（camelCase）。改这里必须同步 Rust 侧。 */

/** 一层磁盘读数 */
export interface DiskLeaf {
  path: string;
  /** 已分配字节数（稀疏文件按实占，不按逻辑长度） */
  bytes: number;
  /** 超出时间预算，只量了一部分——UI 必须显示「≥」 */
  partial: boolean;
}

/**
 * 一个应用的磁盘三层：
 * - `bundle` 本体（.app，只读）
 * - `cache`  可再生缓存（清理入口归「存储」域）
 * - `data`   用户数据（**永不给清理入口**）
 *
 * 三层各自独立为 null：一层测不出不影响其余。全 null 时看 `note`。
 */
export interface AppDisk {
  family: string;
  bundle: DiskLeaf | null;
  cache: DiskLeaf | null;
  data: DiskLeaf | null;
  /** 测量时刻（unix 秒） */
  at: number;
  elapsedMs: number;
  /** 为什么没有数据（给用户一句人话，别让他猜） */
  note: string | null;
}

/** 三层合计（展示用；**不等于可释放空间**——含 APFS 克隆共享部分） */
export function diskTotal(d: AppDisk): number {
  return (d.bundle?.bytes ?? 0) + (d.cache?.bytes ?? 0) + (d.data?.bytes ?? 0);
}

/** 三层是否全是空的（此时 UI 显示 note 或「—」） */
export function diskEmpty(d: AppDisk): boolean {
  return !d.bundle && !d.cache && !d.data;
}

/** 缓存层可清理提示：只有这一层归存储域管，本体与数据都不提供删除入口。
 *  文案在 `resources.mesh.diskHint` 与 `resources.tile.diskHint`（i18n 化后不再有中文常量）。 */

// ---------------------------------------------------------------- M10 批量本体

/** 批量本体查询（一个应用 → 它的某个 exe；后端据此找 .app） */
export interface BundleQuery {
  family: string;
  exe: string | null;
}

/** 批量本体读数。**没有条目 = 没有 .app**（CLI 工具 / 系统进程），要显示「—」而不是 0 */
export interface BundleSize {
  bytes: number;
  /** 超预算只量了一部分 → 显示「≥」 */
  partial: boolean;
}

/** 家族 → 本体体积 */
export type BundleMap = Record<string, BundleSize>;
