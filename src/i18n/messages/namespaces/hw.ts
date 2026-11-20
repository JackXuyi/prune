/**
 * `hw` namespace：硬件读数文案（f26 电池 / 蓝牙配件 / CPU 温度）。
 *
 * 纪律：读不到不渲染对应行——「隐藏」是失败语义，不是占位。
 * strip 条本身已由设备页 eul 重设计替代（device.ribbon.*），
 * 这里只保留仍被 StatusRibbon / 设备磁贴消费的电量语义键。
 */
import type { MessageTree } from "../../types";

const zh = {
  strip: {
    timeLeft: "剩 {h} 小时 {m} 分",
    onAc: "接电",
    charging: "充电中",
    batteryAria: "电池电量 {pct}%",
  },
} as const;

const en: MessageTree<typeof zh> = {
  strip: {
    timeLeft: "{h} h {m} m left",
    onAc: "On AC power",
    charging: "Charging",
    batteryAria: "Battery level {pct}%",
  },
};

export const hw = { zh, en } as const;
