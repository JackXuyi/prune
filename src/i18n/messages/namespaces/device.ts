/**
 * `device` namespace：设备页 eul 式重设计（device-eul-mockup.html）。
 *
 * 结构：Ribbon 7 段（本机硬件 5 + 局域网 2）→ 设备点阵 Hero（lan.hero 复用）
 * → 磁贴 ×2（电源配件 / 磁盘 I/O·体检）→ 设备区（LanBoard 复用）。
 *
 * 纪律：读不到显示 — 或整段隐藏；台式机无内置电池只报配件（诚实口径）。
 */
import type { MessageTree } from "../../types";

const zh = {
  ribbon: {
    aria: "设备页读数带",
    tempTitle: "CPU 温度：SMC 候选键读数的中位数；读不到显示 —，不画占位",
    powerTitle:
      "电量取最低设备（MacBook 为内置电池，台式机只报蓝牙配件）；百分比才套环",
    diskReadTitle: "磁盘读速率：sysinfo 按物理盘差分（2.5 s 一轮快照）；速率不套环",
    diskWriteTitle: "磁盘写速率：真读写拆分；多块物理盘时各自独立、这里求和",
    smartTitle: "SMART：diskutil 解析（1 h 缓存）；三态灯 已验证 / 警告 / 不支持",
    lanTitle: "局域网在线：只读内核邻居表 + 被动收听；进入页面绝不自动扫描",
    exposedTitle:
      "对外暴露：本机绑定 0.0.0.0/:: 的 TCP 端口数（与端口页「对外」同口径）",
    notScanned: "未扫描",
  },
  tiles: {
    powerTitle: "电源与配件",
    powerTipDesktop:
      "蓝牙配件电量（IOPS 30 s 门控轮询）。台式机无内置电池——只报配件；配件也读不到时整块不渲染（隐藏是失败语义，不是 0 占位）",
    powerTipBattery: "内置电池与蓝牙配件电量（IOPS 30 s 门控轮询）",
    lowest: "最低设备 · {name}",
    diskTitle: "磁盘 I/O · 体检",
    diskTip: "读/写速率按物理盘差分（sysinfo bsd 级，2.5 s 一轮）；构成条只表达此刻读:写占比，不代表容量、也不是历史走势（当前无 I/O 时序，无数据不画线）",
    rwShare: "读 {r}% · 写 {w}%",
    idle: "空闲 · 无读写",
    snapshotNote: "2.5 s 一轮快照",
  },
} as const;

const en: MessageTree<typeof zh> = {
  ribbon: {
    aria: "Device page readouts",
    tempTitle: "CPU temp: median of readable SMC keys; shows — when unreadable, never a placeholder",
    powerTitle:
      "Level of the lowest device (internal battery on MacBooks, Bluetooth accessories only on desktops); rings are for percentages only",
    diskReadTitle: "Disk read rate: per-physical-disk differential via sysinfo (2.5 s snapshot); rates never get a ring",
    diskWriteTitle: "Disk write rate: true read/write split; multiple physical disks are measured separately and summed here",
    smartTitle: "SMART: parsed from diskutil (1 h cache); three states Verified / Failing / Unsupported",
    lanTitle: "LAN online: kernel neighbor table + passive listening only; never auto-scans on entry",
    exposedTitle:
      "Exposed: TCP ports bound to 0.0.0.0/:: (same scope as the Ports page)",
    notScanned: "not scanned",
  },
  tiles: {
    powerTitle: "Power & Accessories",
    powerTipDesktop:
      "Bluetooth accessory levels (IOPS-gated, 30 s polling). Desktops have no internal battery — accessories only; the tile disappears entirely when nothing is readable (hidden means unavailable, never a 0 placeholder)",
    powerTipBattery: "Internal battery and Bluetooth accessory levels (IOPS-gated, 30 s polling)",
    lowest: "Lowest device · {name}",
    diskTitle: "Disk I/O · Health",
    diskTip: "Read/write rates are per-physical-disk differentials (sysinfo bsd-level, 2.5 s snapshot); the bar only shows this instant's read:write share — not capacity, not history (no disk I/O time series exists, so no trend line without data)",
    rwShare: "Read {r}% · Write {w}%",
    idle: "Idle · no reads or writes",
    snapshotNote: "2.5 s snapshot",
  },
};

export const device = { zh, en } as const;
