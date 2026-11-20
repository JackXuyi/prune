/**
 * `peripherals` namespace：外设页（m21，docs/m21-peripherals-plan.md）。
 *
 * 三个分区卡：蓝牙（AirPods 分件电量环）/ USB / 音频。
 * 纪律：读不到电量的部件不画环；无设备的分区整块隐藏（隐藏是失败语义，不是 0 占位）。
 */
import type { MessageTree } from "../../types";

const zh = {
  btTitle: "蓝牙设备",
  btTip: "蓝牙外设电量与连接状态（system_profiler，15 s 门控轮询）；读不到电量的部件不画环，分件电量只有 Apple 音频设备会上报",
  notConnected: "已配对 · 未连接",
  connectedLabel: "已连接",
  partLeft: "左",
  partRight: "右",
  partCase: "盒",
  partAria: "{name} {part} 电量 {pct}%",
  usbTitle: "USB 设备",
  usbTip: "USB 总线叶子设备（system_profiler，15 s 门控轮询）；hub 自身不上报，只列实际接入的设备",
  audioTitle: "音频设备",
  audioTip: "CoreAudio 输入 / 输出设备与默认设备标记（15 s 门控轮询）",
  output: "输出",
  input: "输入",
  defaultDevice: "默认",
  unavailable: "未采集",
  waiting: "等待首轮采样…",
  switchOff: "外设采样已在设置中关闭",
} as const;

const en: MessageTree<typeof zh> = {
  btTitle: "Bluetooth",
  btTip: "Bluetooth peripherals and battery levels (system_profiler, 15 s gated polling); unreadable parts get no ring, part-level levels are only reported by Apple audio devices",
  notConnected: "Paired · Not connected",
  connectedLabel: "Connected",
  partLeft: "L",
  partRight: "R",
  partCase: "Case",
  partAria: "{name} {part} battery {pct}%",
  usbTitle: "USB Devices",
  usbTip: "USB bus leaf devices (system_profiler, 15 s gated polling); hubs themselves are not listed — only actually attached devices",
  audioTitle: "Audio Devices",
  audioTip: "CoreAudio output / input devices with default-device badges (15 s gated polling)",
  output: "Output",
  input: "Input",
  defaultDevice: "Default",
  unavailable: "not collected",
  waiting: "Waiting for the first sample…",
  switchOff: "Peripheral sampling is disabled in Settings",
};

export const peripherals = { zh, en } as const;
