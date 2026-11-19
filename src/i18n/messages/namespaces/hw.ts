/**
 * `hw` namespace：概览页硬件读数条（f26 电池 / 蓝牙配件 / CPU 温度）。
 *
 * 纪律：读不到不渲染对应行——「隐藏」是失败语义，不是占位。
 */
import type { MessageTree } from "../../types";

const zh = {
  strip: {
    title: "硬件读数",
    hint: "本页可见时低频采样 · 切走即停",
    cpuTemp: "CPU 温度",
    cpuTempTitle: "SMC 候选键读数的中位数",
    timeLeft: "剩 {h} 小时 {m} 分",
    onAc: "接电",
    charging: "充电中",
    batteryAria: "电池电量 {pct}%",
  },
} as const;

const en: MessageTree<typeof zh> = {
  strip: {
    title: "Hardware",
    hint: "low-frequency sampling while visible · stops when you leave",
    cpuTemp: "CPU temp",
    cpuTempTitle: "Median of readable SMC keys",
    timeLeft: "{h} h {m} m left",
    onAc: "On AC power",
    charging: "Charging",
    batteryAria: "Battery level {pct}%",
  },
};

export const hw = { zh, en } as const;
