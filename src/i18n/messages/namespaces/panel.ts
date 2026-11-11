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
  skeleton: {
    sampling: "Sampling…",
    none: "No data",
  },
};

export const panel = { zh, en } as const;
