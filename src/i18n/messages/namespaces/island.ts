/**
 * `island` namespace：灵动岛（380×220）折叠条与展开卡。
 *
 * 三处语言相关的硬约束：
 * 1. 折叠态的**标签是颜色之外的第二识别通道**（「CPU」/「换出」/「新面孔」/
 *    「端口」），所以它们必须短——英文单词超长会把 229 pt 撑爆，宁可缩写。
 * 2. 数字与单位（`3.2G` / `85%` / `/s`）由代码拼，catalog 只放词语；
 *    `{size}` 这类占位符本身已含单位，翻译时不要重复写单位。
 * 3. 语气中性：「新面孔」不是「入侵」，`randomMac` 要解释这是常态。
 */
import type { MessageTree } from "../../types";

const zh = {
  /** 折叠条标签：越短越好，长度直接决定会不会溢出 */
  tag: {
    swap: "换出",
    newFace: "新面孔",
    conflict: "冲突",
    ports: "端口",
  },

  cpu: {
    subtitle: "CPU 持续偏高",
    stats: "{samples} 个采样里 {ratio}% 超过阈值 · 峰值 {peak}%",
    detailSoon: "明细稍后就绪",
  },

  mem: {
    subtitleBoth: "换出破阈值 · 内核压力：{level}",
    subtitlePressure: "内核内存压力：{level}",
    subtitleSwap: "换出破阈值",
    usage: "占用 {pct}%",
    swapRate: " · 换出速率 {rate}/s",
    compressor: " · 压缩器 {size}",
    procSwap: "换出 {size}",
    detailSoon: "换出明细稍后就绪",
  },

  level: {
    critical: "严重",
    warning: "警告",
    unknown: "未知",
  },

  ports: {
    conflict: "同端口多个持有者",
    ownedBy: "被 {comm} 占用",
    unknownProc: "未知进程",
    owners: "持有者 {n} 个 · ",
    exposed: "对外可达 · ",
    open: "点击打开端口页",
    project: "项目",
    noProject: "无项目线索（GUI 应用或系统进程）",
  },

  lan: {
    joined: "加入了局域网",
    more: "还有 {n} 台新设备 · ",
    open: "点击查看局域网设备",
    /** 设备类型查不到时的兜底词（类型词来自 `lan.kind.*`，见 types/lan.ts） */
    device: "设备",
    randomMac: "随机化 MAC，厂商不可识别（手机私密地址是常态）",
  },
} as const;

const en: MessageTree<typeof zh> = {
  tag: {
    swap: "Swap",
    newFace: "New",
    conflict: "Clash",
    ports: "Port",
  },

  cpu: {
    subtitle: "CPU staying high",
    stats: "{ratio}% of {samples} samples above the threshold · peak {peak}%",
    detailSoon: "Details in a moment",
  },

  mem: {
    subtitleBoth: "Swap over threshold · kernel pressure: {level}",
    subtitlePressure: "Kernel memory pressure: {level}",
    subtitleSwap: "Swap over threshold",
    usage: "Using {pct}%",
    swapRate: " · swap-out {rate}/s",
    compressor: " · compressor {size}",
    procSwap: "Swapped {size}",
    detailSoon: "Swap details in a moment",
  },

  level: {
    critical: "critical",
    warning: "warning",
    unknown: "unknown",
  },

  ports: {
    conflict: "Several owners on one port",
    ownedBy: "Held by {comm}",
    unknownProc: "an unknown process",
    owners: "{n} owners · ",
    exposed: "Externally reachable · ",
    open: "Click to open the Ports page",
    project: "Project",
    noProject: "No project hint (GUI app or system process)",
  },

  lan: {
    joined: "joined the network",
    more: "{n} more new devices · ",
    open: "Click to view LAN devices",
    device: "Device",
    randomMac: "Randomized MAC, vendor unknown (normal for phones' private addresses)",
  },
};

export const island = { zh, en } as const;
