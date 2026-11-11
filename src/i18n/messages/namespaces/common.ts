/**
 * `common` namespace：三态、空态、动作、单位、时间、脚注、错误前缀。
 *
 * 这一组是**跨页面复用**的最小词表。任何只在单个组件里出现的整句，
 * 一律留在该页面的 namespace（stats / disk / net …）里，不放这里 ——
 * 否则 `common` 会膨胀成第二个「什么都往里丢」的垃圾桶。
 */
import type { MessageTree } from "../../types";

const zh = {
  state: {
    notScanned: "尚未扫描",
    notScannedHint: "进入本页会自动扫一轮",
    notSampled: "尚未采样",
    notMeasured: "尚未测量",
    unavailable: "不可读",
    unavailableWith: "不可读（{why}）",
    unknown: "未知",
    samplingWarmup: "采样中…（首拍只建基线，约 2 s）",
    unreadableData: "数据不可读",
  },
  action: {
    scanNow: "立即扫描",
    rescan: "重新扫描",
    refresh: "刷新",
    refreshing: "刷新中",
    cancel: "取消",
    confirm: "确认",
    close: "关闭",
    copy: "复制",
    copied: "已复制",
    copyFailed: "复制失败",
    revealInFinder: "在 Finder 中打开",
    revealAndSelect: "在 Finder 中显示",
    executing: "执行中…",
    deepScan: "深探",
    viewDetails: "看明细",
    retry: "重试",
  },
  progress: {
    scanning: "扫描中…",
    sampling: "采样中…",
    measuring: "测量中…",
    diagnosing: "诊断中…",
    speedTesting: "测速中…",
  },
  pressure: {
    normal: "正常",
    warning: "警告",
    critical: "紧张",
  },
  unit: {
    coresApprox: "约 {n} 核",
    items: { one: "{n} 项", other: "{n} 项" },
    processes: { one: "{n} 个进程", other: "{n} 个进程" },
    daysAgo: "{n} 天前",
    monthsAgo: "{n} 个月前",
    yearsAgo: "{n} 年前",
    unchanged: "{n} 天未改动",
    unchangedMonths: "{n} 个月未改动",
    unchangedYears: "{n} 年未改动",
    seconds: " 秒",
    times: " 次",
  },
  time: {
    justNow: "刚刚",
    minutesAgo: "{n} 分钟前",
    hoursAgo: "{n} 小时前",
    notDeepScanned: "尚未深探",
  },
  empty: {
    reclaim: "没有发现可回收项",
    swapped: "当前没有明显被换出的进程。",
    listenPorts: "最近一次扫描里没有该进程的监听端口。",
    listenPortsUnscanned:
      "还没扫过端口。这里为空不代表该进程没有监听——打开「端口」页扫一轮再看。",
  },
  hint: {
    firstScan: "首次扫描在启动 30 秒后自动开始，也可以手动触发。",
    statsCadence: "内存 / CPU 每 10 s · 网速每 2 s · 端口仅进入端口页时扫描",
    statsPrinciple:
      "统计只呈现事实读数与口径。清理入口在「存储」，结束进程在「内存」与「端口」的实体行上——总览不放破坏性按钮。",
  },
  err: {
    prefix: "不可读：",
    saveFailed: "保存失败：{err}",
    needsDesktop: "需要在 Prune 桌面端内运行（pnpm tauri dev）",
  },
} as const;

const en: MessageTree<typeof zh> = {
  state: {
    notScanned: "Not scanned yet",
    notScannedHint: "Entering this page scans once automatically",
    notSampled: "Not sampled yet",
    notMeasured: "Not measured yet",
    unavailable: "Unavailable",
    unavailableWith: "Unavailable ({why})",
    unknown: "Unknown",
    samplingWarmup: "Sampling… (first sample only builds the baseline, ~2 s)",
    unreadableData: "Data unavailable",
  },
  action: {
    scanNow: "Scan now",
    rescan: "Rescan",
    refresh: "Refresh",
    refreshing: "Refreshing",
    cancel: "Cancel",
    confirm: "Confirm",
    close: "Close",
    copy: "Copy",
    copied: "Copied",
    copyFailed: "Copy failed",
    revealInFinder: "Reveal in Finder",
    revealAndSelect: "Show in Finder",
    executing: "Working…",
    deepScan: "Deep scan",
    viewDetails: "Details",
    retry: "Retry",
  },
  progress: {
    scanning: "Scanning…",
    sampling: "Sampling…",
    measuring: "Measuring…",
    diagnosing: "Diagnosing…",
    speedTesting: "Testing speed…",
  },
  pressure: {
    normal: "Normal",
    warning: "Warning",
    critical: "Critical",
  },
  unit: {
    coresApprox: "≈{n} cores",
    items: { one: "{n} item", other: "{n} items" },
    processes: { one: "{n} process", other: "{n} processes" },
    daysAgo: "{n} days ago",
    monthsAgo: "{n} months ago",
    yearsAgo: "{n} years ago",
    unchanged: "unchanged for {n} days",
    unchangedMonths: "unchanged for {n} months",
    unchangedYears: "unchanged for {n} years",
    seconds: " s",
    times: "×",
  },
  time: {
    justNow: "just now",
    minutesAgo: "{n} min ago",
    hoursAgo: "{n} h ago",
    notDeepScanned: "Not deep-scanned yet",
  },
  empty: {
    reclaim: "Nothing reclaimable found",
    swapped: "No process is being swapped out right now.",
    listenPorts: "No listening ports for this process in the last scan.",
    listenPortsUnscanned:
      "Ports haven't been scanned yet. Empty here doesn't mean nothing is listening — open the Ports tab and scan once.",
  },
  hint: {
    firstScan: "The first scan starts 30 s after launch — or trigger it manually.",
    statsCadence:
      "Memory / CPU every 10 s · network every 2 s · ports only when the Ports tab is open",
    statsPrinciple:
      "Stats show facts and their measurement basis only. Cleanup lives in Storage; killing processes happens on the rows in Memory and Ports — no destructive buttons on the overview.",
  },
  err: {
    prefix: "Unavailable: ",
    saveFailed: "Save failed: {err}",
    needsDesktop: "This runs inside the Prune desktop app (pnpm tauri dev)",
  },
};

export const common = { zh, en } as const;
