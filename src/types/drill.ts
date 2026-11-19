/**
 * 总览页下钻目标（M9 §4）：总览各段/各卡只导航，不做任何清理动作。
 *
 * `device` 后加入（eul 式首页重设计）：设备磁贴 / 状态带电池段直达设备页。
 * 独立成文件是因为 StatsBoard 与 StatusRibbon 双向引用——类型放组件里会成环。
 */
export type DrillTab = "stats" | "resources" | "memory" | "disk" | "ports" | "net" | "device";
