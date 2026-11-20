/**
 * `panel` namespace：菜单栏面板（380×432 单页统计）。
 *
 * 面板是「瞄一眼」场景，文案密度极高——每一句都要能一行读完，翻译时
 * 优先短词而不是解释句（`看进度` / `去扫描` 这类动词短语尤其要短）。
 *
 * 主 CTA 的三态受「不造假事实」红线约束：没扫过是「尚未深探」，扫出 0 是
 * 「暂无可回收」，绝不把 null 写成 0 GB —— 英文侧同样不许出现 "0 B"。
 */
import type { MessageTree } from "../../types";

const zh = {
  badge: {
    footprintTitle: "footprint 口径：含被换出到磁盘的部分",
    rss: "RSS 口径",
    rssTitle: "footprint 不可用，当前全部为 RSS 口径，不含换出量",
  },
  action: {
    refresh: "立即刷新",
    openMain: "打开主窗口",
    settings: "设置",
  },
  footer: {
    lastDeep: "上次深探 {when}",
    cadence: "内存 10 s · 网速 2 s",
  },
  /** 主 CTA：读数 → 动作，只给一个去处（存储页的清理流） */
  cta: {
    scanning: "正在深探…",
    viewProgress: "看进度",
    notScanned: "尚未深探",
    goScan: "去扫描",
    nothing: "暂无可回收",
    details: "看明细",
    reclaim: "可回收 {size}",
    goClean: "去清理",
    title: "{label} · 打开存储页",
  },
  apps: {
    title: "应用资源：按应用看进程 / CPU / 内存 / 磁盘 / 端口 / 网速",
    label: "应用 {n}",
  },
  /** W2/D-07：本地服务段——只在有冲突或关注端口被占时出现，菜单栏空间稀缺 */
  ports: {
    title: "本地服务",
    /** 端口号由行首 .pt 渲染，这里只补冲突倍数，不重复端口 */
    conflict: "{comm} ×{n}",
    conflictBadge: "冲突",
    more: "其余 {n} 个",
  },
  /** 端口三段芯片带（域色 domain-port）：暴露 0 该段不渲染，未扫描整块退化成一行 */
  portsBand: {
    listening: "监听",
    watched: "关注",
    exposed: "暴露",
    scanHint: "去端口页扫描",
    title: "监听 {listening} 个 · 关注 {watched} · 对外暴露 {exposed} · 活跃连接 {conns} · 冲突 {conflicts}",
  },
  skeleton: {
    sampling: "正在采样…",
    none: "无数据",
  },
} as const;

const en: MessageTree<typeof zh> = {
  badge: {
    footprintTitle: "footprint basis: includes what has been swapped to disk",
    rss: "RSS basis",
    rssTitle: "footprint unavailable — everything below is RSS, excluding swap",
  },
  action: {
    refresh: "Refresh now",
    openMain: "Open the main window",
    settings: "Settings",
  },
  footer: {
    lastDeep: "Last deep scan {when}",
    cadence: "Memory 10 s · network 2 s",
  },
  cta: {
    scanning: "Deep scan running…",
    viewProgress: "progress",
    notScanned: "Not deep-scanned",
    goScan: "scan now",
    nothing: "Nothing reclaimable",
    details: "details",
    reclaim: "Reclaimable {size}",
    goClean: "clean up",
    title: "{label} · open the Storage page",
  },
  apps: {
    title: "App resources: processes / CPU / memory / disk / ports / speed per app",
    label: "Apps {n}",
  },
  /** W2/D-07: local services section — only when a conflict exists or watched ports are occupied */
  ports: {
    title: "Local services",
    /** Port number is rendered by the row-leading badge; only the conflict multiplier lives here */
    conflict: "{comm} ×{n}",
    conflictBadge: "conflict",
    more: "{n} more",
  },
  /** Port three-segment band: exposed segment is hidden at 0; unscanned collapses to one line */
  portsBand: {
    listening: "listening",
    watched: "watched",
    exposed: "exposed",
    scanHint: "scan in Ports",
    title: "{listening} listening · {watched} watched · {exposed} exposed · {conns} active · {conflicts} conflicts",
  },
  skeleton: {
    sampling: "Sampling…",
    none: "No data",
  },
};

export const panel = { zh, en } as const;
