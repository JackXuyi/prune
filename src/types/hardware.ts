/** f26 硬件模块 IPC 类型（docs/f26-hardware-modules-plan.md §五）。 */

/** 电源源（G-05 内置电池 / G-06 蓝牙配件，同一 IOPS 数据源） */
export interface PowerSource {
  /** "battery" = 内置电池；"accessory" = 蓝牙等外接配件 */
  kind: "battery" | "accessory";
  name: string;
  /** 0–100；读不到的源为 null（不渲染该行） */
  percent: number | null;
  charging: boolean;
  /** 内置电池剩余分钟数；配件恒 null；未知为 null */
  timeToEmptyMin: number | null;
}

export interface PowerSnapshot {
  sources: PowerSource[];
  collectedAt: number;
}

/** CPU 温度（G-07，SMC 候选键中位数） */
export interface CpuTempSnapshot {
  celsius: number;
  collectedAt: number;
}

/** 磁盘 I/O（G-04，sysinfo 按 bsd 物理盘差分，真读写拆分） */
export interface DiskIoEntry {
  /** 物理盘节点（disk0） */
  name: string;
  readKbPerSec: number;
  writeKbPerSec: number;
}

export interface DiskIoSnapshot {
  disks: DiskIoEntry[];
  collectedAt: number;
}

/** SMART 状态灯（G-03，diskutil 解析，1 h 缓存） */
export interface DiskSmart {
  bsd: string;
  status: "verified" | "failing" | "unsupported";
}

/** 公网 IP（G-08，三级回退 + 10 min 缓存） */
export interface PubIp {
  ip: string;
  source: string;
  fetchedAt: number;
}

/** 硬件模块开关（D7，settings.json 布尔键，默认全开） */
export interface HwFlag {
  key: string;
  enabled: boolean;
}
