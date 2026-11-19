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
  subTest: "手动发起的测试：链路诊断 · 带宽测速 · 站点检测",

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

  pubip: {
    label: "公网 IP",
    title: "出口 IP（来源 {source}）；点击刷新",
    refreshTitle: "立即刷新（绕过 10 分钟缓存）",
    unavailable: "不可用",
  },

  footnote:
    "口径：网卡计数器差分，排除回环与虚拟接口；「今日」为 Prune 运行期间累计（本地日期，每分钟落库），未运行时段无法回填。按应用来自 nettop，与网卡口径不必相等 ——一份按网卡、一份按进程，差几个百分点属正常。",

  subpage: {
    monitor: "监测",
    test: "测试",
    aria: "网络子页",
  },

  diag: {
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
    scopeNote: "Wi-Fi 信号层 v2 提供，本版未含；阈值是经验值，逐层展示不搞黑盒。",
    copyTitle: "生成 Markdown 报告并复制到系统剪贴板，可粘贴给 AI 分析",
    copy: "复制诊断报告",
    probeFailed: "探测失败",
    failNote: "诊断失败：{err}",
  },

  /**
   * 测试二级页（诊断 + 测速 + 站点合并，2026-11-19 重设计）：
   * 速读条 tooltip / 诊断轨道卡 / 测速 Hero 的专用词。
   */
  test: {
    ribbonAria: "测试页速读条",
    verdictTip:
      "链路结论（verdict）：出口 − 网关 > 100 ms 判偏慢；判定规则与建议见下方诊断卡右侧结论区",
    verdictRunning: "诊断中…",
    verdictUnavailable: "不可用",
    rpmTip: "响应度 RPM（Apple networkQuality · bufferbloat 官方口径）· 基础 RTT {rtt} ms",
    rpmSub: "负载下响应度（bufferbloat 官方口径）",
    sitesTip: "站点检测汇总：{n} 站中 {ok} 站可达 · 上次全量 {time}；未测时显示 —",
    sitesUnit: "/ {n} 站",
    gatewayTip: "网关地址（LAN 层探测目标 · ICMP 中位阈值 50 ms）",
    diagTitle: "链路诊断",
    diagNote: "DNS → 网关 → 出口 · 中位数 · 手动触发 · 一轮约 5 s",
    diagRunTip: "重新运行分层探测：DNS 解析、网关 ICMP、公网出口；运行中不可重复发起",
    diagRunningBtn: "诊断中…",
    diagRerun: "重新诊断",
    diagRunningHead: "诊断中…",
    diagFailedHead: "诊断失败",
    diagIdleHead: "尚未诊断",
    diagIdleBody:
      "点右上角「重新诊断」：逐层探测 DNS / 网关 / 出口，完成后给出主因结论与判定规则",
    captiveHead: "疑似 captive portal（登录页）",
    trackTip: "节点读数为样本中位数；超阈值的层是当前瓶颈；原始样本与口径悬停各节点可见",
    heroRunning: "测速中…（约 1–2 分钟，期间可继续使用其它页面）",
    heroRunningBtn: "测速中…",
    heroRunTip: "运行 macOS networkQuality（约 1–2 分钟）；运行中不可重复发起，可离开本页",
    heroIdle: "尚未测速 · 点右侧「开始测速」",
    heroIdleLegend: "完成后给出 ↓ / ↑ 与响应度 RPM",
    heroSource: "系统 networkQuality · 手动触发 · 上次 {time}",
    heroNoIntermediate: "进行中不显示中间值 · 完成后一次性给出 ↓ / ↑ / RPM",
    heroRef: "参考 1 Gbps 实测可用（扣协议开销）· 条满 = 不低于参考",
    stackDownTip: "下行 {speed} · 相对千兆实测可用参考",
    stackUpTip: "上行 {speed} · 相对千兆实测可用参考",
    sitesTitle: "站点检测",
    runAllTip: "按站表顺序逐站检测（单站上限 8 s）；检测中可点「取消」中途停止",
    runStopTip: "停止本轮检测（已完成的结果保留）",
    editTip: "编辑站表（可增删至 20 个站点）",
  },

  speed: {
    start: "开始测速",
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

  /**
   * 站点检测（F15）。措辞纪律（方案 §12）：
   * - 只写观测事实（「TCP 无响应」），不写「被墙 / 封锁」；
   * - `refused` 用「站点拒绝」而不是「失败」；
   * - 速率样本不足时说「样本不足」，不出现 0 KB/s。
   */
  sites: {
    headCount: "{n} 个站点 · 直连（不走系统代理）",
    lastRun: "上次 {time}",
    run: "全部检测",
    running: "检测中 {done}/{total}",
    cancel: "取消",
    cancelledNote: "本轮已取消",
    elapsed: "本轮耗时 {s} s",
    edit: "编辑",
    save: "保存",
    cancelEdit: "取消",
    add: "添加站点",
    remove: "删除",
    reset: "恢复默认（10 个常用站）",
    empty: "站表是空的。点「编辑」加一个，或「恢复默认」拿回 10 个常用站。",
    recovered: "存储的站表读不出来，已回落默认 10 站；原文件没有被覆盖。",
    listError: "操作失败：{why}",
    unavailable: "本机未找到 curl，站点检测不可用。",
    unavailableHint:
      "检测依赖系统 curl（macOS 10.15+ / Windows 10 1803+ 自带）。装好 curl 后重新进入这一页即可。",
    pending: "待测",
    probing: "检测中…",
    rerun: "重测",
    detailNote: "curl 原文",
    cardAria: "查看 {name} 的检测详情",
    detailTitle: "{name} 检测详情",
    noProbe: "这一站还没测过。点「重测」跑一次。",
    probedAt: "检测于 {time}",
    unreachableChip: "不可达：{stage}",
    waterfall: "耗时分布",
    lgHandshake: "握手（DNS / TCP / TLS）",
    lgTtfb: "等服务端首字节",
    lgTransfer: "收数据",
    lgWait: "无响应等待",
    waitSegment: "{stage} 无响应，等了 {ms} ms",
    colStage: "阶段",
    colSeg: "本段",
    colCum: "累计",
    rateLabel: "传输速率",
    rateBasis: "（{bytes} / {ms} ms）",
    truncated: "本轮被 8 s 上限截断（curl 退出码 28）：速率按已收字节算，不是站点真实上限。",
    metaHttp: "HTTP",
    metaBytes: "取样",
    metaIp: "出口",
    metaExit: "curl 退出码",
    rateSampleShort: "样本不足（{bytes} / {ms} ms）",
    thresholds:
      "判慢阈值：TCP >{tcp} ms · TLS >{tls} ms · 首字节 >{ttfb} ms · 传输 <{rate} KB/s（样本 <200 KB 或 <300 ms 不下结论）",
    scope:
      "口径：直连不走代理 · 只测首页文档（≤2 MiB，不含图片/脚本等子资源）· 不跟随跳转 · 速率只用于同站前后对比，跨站比速率是比内容。",
    copy: "复制结果",
    copyTitle: "生成 Markdown 结果并复制到系统剪贴板，可粘贴给 AI 或同事",
    namePlaceholder: "名称",
    urlPlaceholder: "https://example.com/",
    validateHint:
      "保存时校验：只支持 http / https，不能带凭据（user:pass@），最多 20 个站点；同一地址只会留一条。",
    slowChip: "偏慢：{stage}",
    refusedChip: "{code} · 站点拒绝",
    stage: {
      dns: "DNS",
      tcp: "TCP",
      tls: "TLS",
      ttfb: "首字节",
      transfer: "传输",
      http: "HTTP",
    },
    summary: {
      ok: "可达",
      slow: "偏慢",
      stalled: "无响应",
      refused: "站点拒绝",
      unreachable: "不可达",
      error: "出错",
    },
    row: {
      stalled: "连上了，但迟迟没有响应（{sec} s 无首字节）",
      refused: "站点返回 {code}（可能拒绝非浏览器请求，链路本身是通的）",
      errorNote: "探测本身出错：{why}",
      dnsFail: "域名解析失败",
      tcpFail: "无法建立连接：TCP 无响应（{sec} s 超时）",
      tlsFail: "TLS 握手无响应（TCP {tcp} ms 已通）",
      tlsFailNoTcp: "TLS 握手无响应",
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
  subTest: "Tests you run yourself: link diagnosis · bandwidth speed test · site checks",

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

  pubip: {
    label: "Public IP",
    title: "Egress IP (source: {source}); click to refresh",
    refreshTitle: "Refresh now (bypass the 10-minute cache)",
    unavailable: "Unavailable",
  },

  footnote:
    "Basis: interface counter deltas, excluding loopback and virtual interfaces. “Today” counts only while Prune is running (local date, saved once a minute); time when it wasn't running can't be backfilled. Per-app data comes from nettop and needn't match the interface totals — one is per interface, the other per process, so a few percent of difference is normal.",

  subpage: {
    monitor: "Monitor",
    test: "Test",
    aria: "Network subpages",
  },

  diag: {
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
      "The Wi-Fi signal layer arrives in v2 and isn't included here; thresholds are empirical, shown layer by layer — no black boxes.",
    copyTitle: "Generate a Markdown report and copy it to the system clipboard, ready to paste into an AI",
    copy: "Copy diagnosis report",
    probeFailed: "Probe failed",
    failNote: "Diagnosis failed: {err}",
  },

  test: {
    ribbonAria: "Test page ribbon",
    verdictTip:
      "Link verdict: the internet layer is considered slow when gateway-to-internet exceeds 100 ms; the rule and advice live in the diagnosis card below",
    verdictRunning: "Diagnosing…",
    verdictUnavailable: "Unavailable",
    rpmTip: "Responsiveness RPM (Apple networkQuality · the official bufferbloat measure) · base RTT {rtt} ms",
    rpmSub: "Responsiveness under load (the official bufferbloat measure)",
    sitesTip: "Site checks: {ok} of {n} sites reachable · last full run {time}; shows — before the first run",
    sitesUnit: "/ {n} sites",
    gatewayTip: "Gateway address (LAN probe target · ICMP median threshold 50 ms)",
    diagTitle: "Link diagnosis",
    diagNote: "DNS → gateway → internet · medians · manual trigger · one round ≈ 5 s",
    diagRunTip: "Re-run the layered probes: DNS, gateway ICMP, internet egress; disabled while running",
    diagRunningBtn: "Diagnosing…",
    diagRerun: "Diagnose again",
    diagRunningHead: "Diagnosing…",
    diagFailedHead: "Diagnosis failed",
    diagIdleHead: "Not diagnosed yet",
    diagIdleBody:
      "Hit “Diagnose again” above: DNS / gateway / internet are probed layer by layer, ending with a root cause and its rule",
    captiveHead: "Captive portal suspected (sign-in page)",
    trackTip: "Node readings are sample medians; the over-threshold layer is the current bottleneck; hover a node for raw samples",
    heroRunning: "Testing… (~1–2 min, you can keep using other pages meanwhile)",
    heroRunningBtn: "Testing…",
    heroRunTip: "Runs macOS networkQuality (~1–2 min); can't be restarted while running, and you can leave this page",
    heroIdle: "No speed test yet · hit “Run speed test” on the right",
    heroIdleLegend: "Down / up and responsiveness RPM appear once it finishes",
    heroSource: "System networkQuality · manual trigger · last run {time}",
    heroNoIntermediate: "No intermediate values while running · down / up / RPM arrive together at the end",
    heroRef: "Reference: 1 Gbps practical ceiling (overhead excluded) · a full bar means “at least this”",
    stackDownTip: "Down {speed} · against the gigabit practical reference",
    stackUpTip: "Up {speed} · against the gigabit practical reference",
    sitesTitle: "Site checks",
    runAllTip: "Checks each site in list order (8 s cap per site); press “Cancel” to stop mid-run",
    runStopTip: "Stop this round (completed results are kept)",
    editTip: "Edit the site list (up to 20 sites)",
  },

  speed: {
    start: "Run speed test",
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

  /**
   * Site checks (F15). Same wording discipline as above: describe the observed
   * stage only (never "blocked / censored"), `refused` is not "failed", and a
   * too-small sample never turns into "0 KB/s".
   */
  sites: {
    headCount: "{n} sites · direct connection (system proxy bypassed)",
    lastRun: "last run {time}",
    run: "Check all",
    running: "Checking {done}/{total}",
    cancel: "Cancel",
    cancelledNote: "This round was cancelled",
    elapsed: "Took {s} s",
    edit: "Edit",
    save: "Save",
    cancelEdit: "Cancel",
    add: "Add site",
    remove: "Remove",
    reset: "Restore defaults (10 common sites)",
    empty: "The list is empty. Use “Edit” to add one, or “Restore defaults” to get the 10 common sites back.",
    recovered: "The stored site list could not be read, so the default 10 are shown. The original file was not overwritten.",
    listError: "That didn't work: {why}",
    unavailable: "curl was not found on this machine, so site checks are unavailable.",
    unavailableHint:
      "The check relies on the system curl (bundled with macOS 10.15+ / Windows 10 1803+). Install curl and reopen this page.",
    pending: "Not checked",
    probing: "Checking…",
    rerun: "Recheck",
    detailNote: "curl output",
    cardAria: "Open the check details for {name}",
    detailTitle: "{name} check details",
    noProbe: "This site has not been checked yet. Hit “Recheck” to run it.",
    probedAt: "checked {time}",
    unreachableChip: "Unreachable: {stage}",
    waterfall: "Where the time went",
    lgHandshake: "handshake (DNS / TCP / TLS)",
    lgTtfb: "waiting for the first byte",
    lgTransfer: "receiving data",
    lgWait: "no response",
    waitSegment: "{stage} never answered — waited {ms} ms",
    colStage: "Stage",
    colSeg: "This stage",
    colCum: "Cumulative",
    rateLabel: "Transfer rate",
    rateBasis: "({bytes} / {ms} ms)",
    truncated:
      "Cut off by the 8 s cap (curl exit 28): the rate is computed on bytes received, not the site's real ceiling.",
    metaHttp: "HTTP",
    metaBytes: "sample",
    metaIp: "remote",
    metaExit: "curl exit",
    rateSampleShort: "sample too small ({bytes} / {ms} ms)",
    thresholds:
      "Slow thresholds: TCP >{tcp} ms · TLS >{tls} ms · first byte >{ttfb} ms · transfer <{rate} KB/s (no verdict under 200 KB or 300 ms of sample)",
    scope:
      "Scope: direct connection (no proxy) · home document only (≤ 2 MiB, no images/scripts) · redirects not followed · rates compare the same site over time, not one site against another.",
    copy: "Copy results",
    copyTitle: "Render a Markdown summary and copy it to the system clipboard",
    namePlaceholder: "Name",
    urlPlaceholder: "https://example.com/",
    validateHint:
      "Validated on save: http / https only, no credentials (user:pass@), 20 sites max; duplicate URLs collapse into one.",
    slowChip: "Slow: {stage}",
    refusedChip: "{code} · refused",
    stage: {
      dns: "DNS",
      tcp: "TCP",
      tls: "TLS",
      ttfb: "first byte",
      transfer: "transfer",
      http: "HTTP",
    },
    summary: {
      ok: "Reachable",
      slow: "Slow",
      stalled: "No response",
      refused: "Refused",
      unreachable: "Unreachable",
      error: "Error",
    },
    row: {
      stalled: "Connected, but nothing came back ({sec} s without a first byte)",
      refused: "The site returned {code} (likely refusing non-browser requests — the path itself is open)",
      errorNote: "The probe itself failed: {why}",
      dnsFail: "DNS resolution failed",
      tcpFail: "Could not connect: TCP gave no response ({sec} s timeout)",
      tlsFail: "TLS handshake gave no response (TCP was up at {tcp} ms)",
      tlsFailNoTcp: "TLS handshake gave no response",
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
