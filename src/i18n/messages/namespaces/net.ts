/**
 * `net` namespace：网络 tab（监测 / 诊断 / 测速）与「暴露面」词表。
 *
 * 三条纪律直接体现在文案里，翻译时**不许弱化**（docs/i18n-spec.md §4.2）：
 * - 不可读 / 探测失败 ≠ 0 —— 任何「0」都要能被读成「真的没有」；
 * - 阈值逐行展示，不搞黑盒；
 * - 不承诺：`captive`、`jitter` 这类探测结论一律带「疑似 / 阈值」而非断言。
 *
 * `↓` `↑` `▮` `⚠` `→` 这些符号留在代码里（与语言无关），catalog 只放词语。
 */
import type { MessageTree } from "../../types";

const zh = {
  title: "网络",
  subMonitor: "系统口径的下载 / 上传速率 · 只算物理网卡（虚拟接口单列，避免与 VPN 重复计数）",
  subDiag: "分层定位慢在哪：DNS → 路由器 → 出口 · 手动触发，不常驻",

  unavailable: "不可读：{why}。这里不显示 0 —— 读不到和「没有流量」是两件事。",
  unavailableFallback: "系统网卡计数器读不到",

  down: "下载",
  up: "上传",
  ownScale: "各自刻度",
  chartAria: "{label}近 5 分钟走势，每秒一个点",
  accumulating: "走势积累中（每秒一个点）",
  last5min: "近 5 min",
  noTraffic: "本拍无流量",

  agg: {
    peak: "本次会话峰值",
    peakTitle: "本次会话内单拍最大值，与走势窗口无关（否则「峰值」会随窗口缩水）",
    avg: "近 5 min 平均",
    avgSub: "时间加权",
    avgTitle: "按点间隔加权：采样抖动时简单平均会放大短间隔的尖峰",
    today: "今日",
    todayUnavailable: "不可用",
    todayNote: "存储打不开，实时速率与会话累计照常",
    todayTitle: "Prune 运行期间累计：未运行的时段无法回填（系统计数器只给「开机至今」）",
    notPersisted: "⚠ 未落库",
    session: "本次会话",
    sessionSub: "自启动起",
    sessionTitle: "自 Prune 启动以来：回答「这次开了多久用了多少」，不跨重启",
    noReading: "还没有读数",
  },

  days: {
    title: "近 7 天",
    hint: "下 / 上堆叠 · 缺失日灰显，不画成 0",
    todaySuffix: "（今天）",
    noRecord: "没有记录（Prune 未运行）",
    legendScale: "各日按 7 天内最大值",
    recordedLabel: "已记录",
    recordedSuffix: "/{total} 天",
  },

  iface: {
    title: "网卡",
    hint: "物理口合计 = 上面的数字",
    warmup: "采样中…（第一拍只建立基线，约 1–2 s）",
    empty: "没有可用的物理网卡读数。断网时这里是 0 B/s，而不是「不可读」。",
    totalAria: "{name} 合计 {speed}",
    virtualCount: "虚拟接口 {n} 个（{names}）",
    virtualNote: "不计入主数字，避免与物理口重复",
  },

  /** 网卡类型的展示词（物理口是默认，虚拟口必须自带「不计入」的解释） */
  kind: {
    physical: "物理",
    tunnel: "隧道",
    virtual: "虚拟",
    loopback: "回环",
  },

  apps: {
    title: "按应用",
    hint: "nettop · 仅非回环 · 按速率降序",
    detailTitle: "在「应用资源」里看这个应用的完整画像",
    totalAria: "{name} 合计 {speed}",
    empty: "本拍没有应用在跑流量。",
    collapse: "收起",
    more: "▸ 其余 {n} 个应用",
    degradedWarmup: "采样中…（按应用需要 nettop，进入本 tab 后 1 s 内出数）",
    degradedUnavailable: "按应用不可读：{why}（主数字与今日不受影响）",
    nettopMissing: "nettop 不可用",
    degradedOther: "按应用暂时不可读",
  },

  footnote:
    "口径：网卡计数器差分，排除回环与虚拟接口；「今日」为 Prune 运行期间累计（本地日期，每分钟落库），未运行时段无法回填。按应用来自 nettop，与网卡口径不必相等 ——一份按网卡、一份按进程，差几个百分点属正常。",

  subpage: {
    monitor: "监测",
    diag: "诊断",
    aria: "网络子页",
  },

  diag: {
    oneRound: "一轮约 5 s · 探测失败显示失败，不显示 0",
    start: "开始诊断",
    idle: "没跑过诊断。点「开始诊断」探测 DNS、路由器、出口三层，每层画一条横柱（中位数）并标出阈值刻度，最后给出一条主因结论。",
    failed: "无法诊断：{why}。这是探测机制本身失败，与「连不上路由器」是两件事。",
    unknownWhy: "未知原因",
    running: "探测中…（DNS → 网关 → 出口 → portal，全程串行不出中间结论）",
    trackAria: "分层探测轨道：DNS → 路由器 → 出口",
    dnsSub: "解析 www.apple.com · 系统解析器（含缓存），低值不代表无 DNS 问题",
    router: "路由器",
    routerWith: "路由器 {gateway}",
    lanSub: "网关 :80 连接耗时",
    wanName: "出口",
    wanSubJitter:
      "1.1.1.1:443（被拦自动回退 223.5.5.5:443）· 抖动 ±{jitter} ms（阈值 {threshold} ms）",
    wanSub: "1.1.1.1:443（被拦自动回退 223.5.5.5:443）连接耗时",
    captive: "检测到疑似登录页（酒店 / 机场 Wi-Fi）——打开浏览器完成认证后重新诊断。",
    verdict: "→ 主因：{label}",
    verdictRule: "{advice}（规则：{rule}）",
    scopeNote: "Wi-Fi 信号层 v2 提供，本版未含；阈值是经验值，全部标在柱图刻度上。",
    copyTitle: "生成 Markdown 报告并复制到系统剪贴板，可粘贴给 AI 分析",
    copy: "复制诊断报告",
    probeFailed: "探测失败",
    failNote: "诊断失败：{err}",
  },

  layer: {
    noReading: "本层没有可用读数，柱图省略——「没有读数」和「0 ms」是两件事。",
    thresholdTick: "阈值 {n} ms",
    sample: "样本 {index}：{n} ms",
    samplesMedian: "{count} 次样本 · 中位数 {median} ms",
    thresholdFoot: "阈值 {n} ms（竖刻度）",
  },

  speed: {
    title: "测速 & 负载响应",
    hint: "系统 networkQuality · 约 30 s",
    start: "开始测速",
    runningNote: "测速中…（期间网络会被占满，下载 / 上传 / 负载下延迟各测一段）",
    barAriaDown: "下载 {speed}，参考千兆带宽",
    barAriaUp: "上传 {speed}，参考千兆带宽",
    gigabit:
      "条形参考 = 千兆带宽的实际可用值（约 940 Mbps，已扣协议开销）；2.5G / 万兆网卡会顶满，此时以数字为准。",
    rpm: "RPM · 负载下响应度",
    baseRtt: "base_rtt {n} ms",
    rpmTitle:
      "RPM 是 Apple 的「负载下响应度」：满速下载/上传时连接的响应能力，直接对应视频会议会不会卡",
    rpmNote:
      "RPM 是 Apple 的「负载下响应度」：满速传输时还能保持多少响应，直接对应视频会议会不会卡。与 WhyFi 的 Cloudflare 口径不同，属 Apple 官方标准。",
    idle: "没跑过测速。点「开始测速」拿吞吐与 RPM（bufferbloat 官方口径），约 30 s。",
    /** Apple 原文分级 → 展示词（只做映射，不自造阈值） */
    level: {
      poor: "很差",
      low: "较差",
      medium: "尚可",
      good: "良好",
    },
  },

  /** 端口绑定范围（BindScope）——安全语义的唯一来源，不是显示细节 */
  exposure: {
    any: "全部网卡",
    interface: "指定网卡",
    loopback: "仅本机",
    unknown: "未知",
    hintAny: "绑定在 * / 0.0.0.0，同网段的其他设备可以连进来",
    hintInterface: "绑定在某个具体网卡地址，局域网内可达",
    hintLoopback: "127.0.0.1 / ::1，只有本机进程能连",
    hintUnknown: "地址解析失败，不对暴露面下结论",
  },
} as const;

const en: MessageTree<typeof zh> = {
  title: "Network",
  subMonitor:
    "System-level download / upload rates · physical interfaces only (virtual ones are listed separately so VPN traffic isn't counted twice)",
  subDiag:
    "Find the slow hop: DNS → router → internet · manual only, nothing runs in the background",

  unavailable:
    "Unavailable: {why}. We don't show 0 here — “can't read it” and “no traffic” are two different things.",
  unavailableFallback: "the system interface counters can't be read",

  down: "Download",
  up: "Upload",
  ownScale: "Own scale",
  chartAria: "{label} over the last 5 minutes, one point per second",
  accumulating: "Building the trend (one point per second)",
  last5min: "last 5 min",
  noTraffic: "No traffic this sample",

  agg: {
    peak: "Session peak",
    peakTitle:
      "Largest single sample this session, independent of the trend window (otherwise the “peak” shrinks with the window)",
    avg: "5 min average",
    avgSub: "time-weighted",
    avgTitle:
      "Weighted by sample spacing: with jittery sampling, a plain average inflates spikes from short gaps",
    today: "Today",
    todayUnavailable: "Unavailable",
    todayNote: "Storage won't open — live rates and session totals still work",
    todayTitle:
      "Counted while Prune is running: time when it wasn't can't be backfilled (system counters only cover “since boot”)",
    notPersisted: "⚠ not saved",
    session: "This session",
    sessionSub: "since launch",
    sessionTitle:
      "Since Prune started: answers “how much has this run used”, not across restarts",
    noReading: "No reading yet",
  },

  days: {
    title: "Last 7 days",
    hint: "Down / up stacked · missing days are greyed out, never drawn as 0",
    todaySuffix: " (today)",
    noRecord: "No record (Prune wasn't running)",
    legendScale: "each day scaled to the 7-day maximum",
    recordedLabel: "Recorded",
    recordedSuffix: "/{total} days",
  },

  iface: {
    title: "Interfaces",
    hint: "Physical total = the numbers above",
    warmup: "Sampling… (the first sample only builds the baseline, ~1–2 s)",
    empty:
      "No usable physical-interface reading. With no network this shows 0 B/s, not “unavailable”.",
    totalAria: "{name} total {speed}",
    virtualCount: "{n} virtual interfaces ({names})",
    virtualNote: "Not counted in the main number, to avoid double-counting with physical ones",
  },

  kind: {
    physical: "Physical",
    tunnel: "Tunnel",
    virtual: "Virtual",
    loopback: "Loopback",
  },

  apps: {
    title: "By app",
    hint: "nettop · non-loopback only · sorted by rate",
    detailTitle: "See this app's full profile in App resources",
    totalAria: "{name} total {speed}",
    empty: "No app is moving traffic this sample.",
    collapse: "Collapse",
    more: "▸ {n} more apps",
    degradedWarmup:
      "Sampling… (per-app data needs nettop; it shows up within 1 s of opening this tab)",
    degradedUnavailable: "Per-app data unavailable: {why} (the main numbers and today are unaffected)",
    nettopMissing: "nettop unavailable",
    degradedOther: "Per-app data is temporarily unavailable",
  },

  footnote:
    "Basis: interface counter deltas, excluding loopback and virtual interfaces. “Today” counts only while Prune is running (local date, saved once a minute); time when it wasn't running can't be backfilled. Per-app data comes from nettop and needn't match the interface totals — one is per interface, the other per process, so a few percent of difference is normal.",

  subpage: {
    monitor: "Monitor",
    diag: "Diagnose",
    aria: "Network subpages",
  },

  diag: {
    oneRound: "One round takes ~5 s · a failed probe shows as failed, never as 0",
    start: "Run diagnosis",
    idle: "No diagnosis yet. Hit “Run diagnosis” to probe DNS, the router and the internet; each layer gets a bar (median) with its threshold marked, and the result ends with a single root-cause conclusion.",
    failed:
      "Can't diagnose: {why}. The probing mechanism itself failed — that's not the same as “can't reach the router”.",
    unknownWhy: "unknown reason",
    running: "Probing… (DNS → gateway → internet → portal, strictly serial, no intermediate conclusions)",
    trackAria: "Layer probe track: DNS → router → internet",
    dnsSub:
      "resolving www.apple.com · system resolver (cached) — a low value doesn't mean there's no DNS problem",
    router: "Router",
    routerWith: "Router {gateway}",
    lanSub: "gateway :80 connect time",
    wanName: "Internet",
    wanSubJitter:
      "1.1.1.1:443 (falls back to 223.5.5.5:443 if blocked) · jitter ±{jitter} ms (threshold {threshold} ms)",
    wanSub: "1.1.1.1:443 (falls back to 223.5.5.5:443 if blocked) connect time",
    captive:
      "A sign-in page is suspected (hotel / airport Wi-Fi) — finish signing in with a browser, then diagnose again.",
    verdict: "→ Root cause: {label}",
    verdictRule: "{advice} (rule: {rule})",
    scopeNote:
      "The Wi-Fi signal layer arrives in v2 and isn't included here; thresholds are empirical and all marked on the bars.",
    copyTitle: "Generate a Markdown report and copy it to the system clipboard, ready to paste into an AI",
    copy: "Copy diagnosis report",
    probeFailed: "Probe failed",
    failNote: "Diagnosis failed: {err}",
  },

  layer: {
    noReading:
      "No usable reading for this layer, so the bar is omitted — “no reading” and “0 ms” are two different things.",
    thresholdTick: "threshold {n} ms",
    sample: "Sample {index}: {n} ms",
    samplesMedian: "{count} samples · median {median} ms",
    thresholdFoot: "threshold {n} ms (vertical mark)",
  },

  speed: {
    title: "Speed & loaded latency",
    hint: "System networkQuality · ~30 s",
    start: "Run speed test",
    runningNote:
      "Testing… (this saturates the link; download, upload and loaded latency are each measured in turn)",
    barAriaDown: "Download {speed}, referenced to gigabit bandwidth",
    barAriaUp: "Upload {speed}, referenced to gigabit bandwidth",
    gigabit:
      "The bar reference is gigabit bandwidth's practical ceiling (~940 Mbps, protocol overhead excluded); 2.5G / 10G interfaces will pin the bar — trust the number then.",
    rpm: "RPM · loaded responsiveness",
    baseRtt: "base_rtt {n} ms",
    rpmTitle:
      "RPM is Apple's “responsiveness under load”: how well connections respond while downloading/uploading at full speed — it maps directly to whether video calls stutter",
    rpmNote:
      "RPM is Apple's “responsiveness under load”: how much responsiveness survives a saturated link, which maps directly to whether video calls stutter. It's Apple's own standard, not the Cloudflare metric WhyFi uses.",
    idle: "No speed test yet. Hit “Run speed test” for throughput and RPM (Apple's official bufferbloat measure); about 30 s.",
    level: {
      poor: "Poor",
      low: "Low",
      medium: "Medium",
      good: "Good",
    },
  },

  exposure: {
    any: "All interfaces",
    interface: "Specific interface",
    loopback: "Local only",
    unknown: "Unknown",
    hintAny: "Bound to * / 0.0.0.0 — other devices on the same subnet can connect",
    hintInterface: "Bound to a specific interface address, reachable on the LAN",
    hintLoopback: "127.0.0.1 / ::1 — only local processes can connect",
    hintUnknown: "Address parsing failed, so no conclusion about exposure",
  },
};

export const net = { zh, en } as const;
