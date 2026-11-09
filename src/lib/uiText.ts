/**
 * 全局三态文案常量库（UX v3 §5.3 / §7）。
 *
 * 「不显示 0」「尚未扫描 ≠ 空集」「不可读 + 原因」这些规则此前散落在各组件的
 * 字符串里；这里收敛为唯一来源。改动任何一条先读 docs/ux-interaction-redesign-plan.md §5.3。
 */
export const UI_TEXT = {
  /** 未扫描 / 未采样 / 未测量——绝不显示 0 或空表 */
  notScanned: "尚未扫描",
  notSampled: "尚未采样",
  notMeasured: "尚未测量",

  /** 加载中：动词 + … */
  scanning: "扫描中…",
  sampling: "采样中…",
  measuring: "测量中…",
  diagnosing: "诊断中…",
  speedTesting: "测速中…",

  /** 读不到：前缀 + 原因，绝不显示 0 */
  unreadablePrefix: "不可读：",

  /** 空结果（扫描过但为空），与「未扫描」区分 */
  emptyReclaim: "没有发现可回收项",
  emptySwapped: "当前没有明显被换出的进程。",
  emptyListenPorts: "最近一次扫描里没有该进程的监听端口。",
  notScannedListenPorts:
    "还没扫过端口。这里为空不代表该进程没有监听——打开「端口」页扫一轮再看。",

  /** 存储 */
  firstScanHint: "首次扫描在启动 30 秒后自动开始，也可以手动触发。",
  scanNow: "立即扫描",

  /** 导航 */
  backToOverview: "← 返回概况",
  settingsLabel: "设置",

  /** 总览页脚 */
  statsCadence: "内存 / CPU 每 10 s · 网速每 2 s · 端口仅进入端口页时扫描",
  statsPrinciple:
    "统计只呈现事实读数与口径。清理入口在「存储」，结束进程在「内存」与「端口」的实体行上——总览不放破坏性按钮。",
} as const;
