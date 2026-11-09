/** D1/D3：应用图标与小时历史的前端契约。 */

export interface IconQuery {
  family: string;
  /** 主进程 exe（拿不到就 null → 前端直接落字母徽标） */
  exe: string | null;
}

/** 图标：family → `data:image/png;base64,...`。缺失的家族不在 map 里 */
export type IconMap = Record<string, string>;

/**
 * 应用小时历史的一行。
 *
 * **只含有记录的小时**：缺失由前端画灰柱，不能用 0 补
 * （补 0 等于把「Prune 没运行」说成「那一小时 CPU 为 0」）。
 */
export interface HistoryBucket {
  /** 本地时间 `YYYY-MM-DDTHH`，字典序即时间序 */
  bucket: string;
  rssAvg: number | null;
  rssPeak: number | null;
  cpuAvg: number | null;
  cpuPeak: number | null;
  netDown: number;
  netUp: number;
  /** RSS 样本数：判「这一小时 Prune 在不在」 */
  samples: number;
}
