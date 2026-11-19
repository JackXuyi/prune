/**
 * 应用列表页的排序维度。抽到 types/ 是为了让摘要带与列表页共用同一套
 * key（同一个 key 在「排序 chip」「摘要带段」「卡内指标」里必须指同一指标）。
 */
export type SortKey = "mem" | "cpu" | "procs" | "disk" | "ports" | "down";
