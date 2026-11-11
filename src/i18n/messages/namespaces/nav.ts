/**
 * `nav` namespace：左侧图标栏、主窗口 tab、⌘ 快捷键提示、返回按钮。
 *
 * `label` / `hint` 在 RailNav 里存的是 **key**，渲染时才 `t()` —— 因为
 * ITEMS 是模块级常量，写死文案的话切语言不会跟着变。
 */
import type { MessageTree } from "../../types";

const zh = {
  stats: "总览",
  resources: "应用",
  memory: "内存",
  disk: "存储",
  net: "网络",
  ports: "端口",
  settings: "设置",
  statsHint: "可回收 + 内存 / CPU / 硬盘 / 端口 / 网速：本机资源总览",
  resourcesHint: "每个应用一张卡：进程 / CPU / 内存 / 磁盘 / 端口 / 网速",
  memoryHint: "内存概况、swap 与换出受害者 · 进程家族与明细",
  diskHint: "卷容量、可回收空间与清理",
  netHint: "本机下载 / 上传速率、走势、网卡分布与今日流量",
  portsHint: "谁占着 3000：按端口反查持有进程",
  settingsHint: "设置 · 灵动岛开关与阈值",
  mainAria: "主导航",
  tabAria: "主标签",
  cmdHint: "⌘{n} 切换",
  backTo: "← 返回{label}",
  backToOverview: "← 返回概况",
  expand: "展开",
  collapse: "收起",
} as const;

const en: MessageTree<typeof zh> = {
  stats: "Overview",
  resources: "Apps",
  memory: "Memory",
  disk: "Storage",
  net: "Network",
  ports: "Ports",
  settings: "Settings",
  statsHint: "Reclaimable + memory / CPU / disk / ports / speed: this machine at a glance",
  resourcesHint: "One card per app: processes / CPU / memory / disk / ports / speed",
  memoryHint: "Memory overview, swap and victims · process families and detail",
  diskHint: "Volume capacity, reclaimable space and cleanup",
  netHint: "Download / upload rate, trend, interface split and today's traffic",
  portsHint: "Who owns 3000: map ports back to processes",
  settingsHint: "Settings · Dynamic Island switches and thresholds",
  mainAria: "Main navigation",
  tabAria: "Main tabs",
  cmdHint: "⌘{n} to switch",
  backTo: "← Back to {label}",
  backToOverview: "← Back to overview",
  expand: "Expand",
  collapse: "Collapse",
};

export const nav = { zh, en } as const;
