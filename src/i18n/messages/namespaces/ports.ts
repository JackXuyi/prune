/**
 * `ports` namespace：端口 tab（PortList）+ 端口仪表盘 / 折叠层 / 行内提示。
 *
 * 「Lsof 看不到别人的进程」这类**口径说明**是产品的一部分，不是错误信息：
 * 翻译时要保留「为什么看不到 + 为什么我们不提权」两层，不能压成一句
 * “permission denied” —— 那会让用户以为是自己操作错了。
 *
 * 命令（`sudo lsof -nP -iTCP -sTCP:LISTEN`）留在代码里，不进 catalog。
 */
import type { MessageTree } from "../../types";

const zh = {
  title: "端口",
  searchPlaceholder: "端口 / 进程 / 项目",

  /** 非 root 的 lsof 会静默跳过别的用户的进程 —— 必须自己说出来 */
  invisibleLead:
    "内核报告 {total} 个监听，其中 {invisible} 个无法归属到进程（没有权限时 lsof 会跳过别的用户的进程）。Prune 不会提权；需要时请手动执行",
  invisibleTail: "。",

  /** TIME_WAIT 会占端口但没有进程持有：不渲染成「被占用」，也不给结束入口 */
  timeWait:
    "另有 {n} 个 TIME_WAIT 连接{ports}。它们在内核保留期内仍会让新服务拿到 EADDRINUSE，但没有任何进程持有，约 2 分钟后自动消失——这不是漏查。",
  timeWaitPorts: "（涉及端口 {ports}）",

  scanning: "正在扫描监听端口…",
  emptyNeverScanned: "还没有扫描结果。点右上角「刷新」扫一轮——这不代表没有端口占用。",
  emptyCleanTitle: "很干净，没有监听端口 ✂️",
  emptyCleanNote:
    "当前用户可见范围内没有进程监听端口。扫描过了才是这个结论——不是没扫。",
  noMatch: "没有匹配「{query}」的端口。",
  noMatchTitle: "没有匹配「{query}」的端口",
  noMatchNote: "试试端口号、进程名或项目名的一部分，或清除搜索条件。",
  clearSearch: "清除搜索",
  rowLocate: "定位",
  /** W2/D-09：prune.toml 配置了关注但本轮空闲的端口——空闲是状态变化，不是没有 */
  configIdle: "项目配置的关注端口当前空闲：{ports}（prune.toml 还在，服务没起）",
  configIdleAria: "{n} 个配置关注端口空闲",

  /** eul 状态带：5 段全是本页任务内读数；归属盲区走提醒带，不上带子 */
  ribbon: {
    aria: "端口状态带",
    listenUnit: "监听",
    listenTitle:
      "监听 {n} 个：lsof 能归属到进程的 {attributed} 个 · 内核口径 {total} 个（盲区见下方提醒带）",
    exposedUnit: "暴露",
    exposedTitle: "对外暴露 {n} 个：绑定 0.0.0.0 / ::，局域网内其他设备可达",
    watchedUnit: "关注",
    watchedTitle: "关注 {n} 个：prune.toml 声明的端口（各项目并集）",
    connsUnit: "连接",
    connsTitle: "活跃连接 {n} 条：监听端口上的 ESTABLISHED 之和；{udp}",
    udpNote: "{n} 个 UDP 端口无连接口径，不计入",
    closeWaitUnit: "未关",
    closeWaitTitle:
      "CLOSE_WAIT {n} 条：本端未关闭的连接——泄漏信号，不是结论；0 为实测零，非缺数",
  },

  /** eul Hero：点阵主角（一端口一点）+ 扫描口径块（重扫从快捷键变显性按钮） */
  hero: {
    tip: "每个点 = 一个监听端口；hover 出「端口 · 进程」。点色：琥珀=对外暴露 / 绿=关注 / 灰=其他（含系统服务）",
    leadNote: "个监听端口 · 琥珀是对外暴露，绿是关注，灰是其他",
    dotAria: "端口点阵：对外暴露 {exposed} · 关注 {watched} · 其他 {other}",
    legendExposed: "对外暴露",
    legendWatched: "关注",
    legendOther: "其他（多为系统服务）",
    scanAt: "扫描完成",
    scanMs: "耗时",
    attribution: "归属口径",
    rescan: "重新扫描",
  },

  col: {
    port: "端口",
    exposure: "暴露面",
    process: "进程",
    project: "项目",
    conns: "连接",
  },

  row: {
    hintKillable: "双击查看该进程明细 · 右键结束进程",
    hintLocked: "该端口由系统进程或内核持有，不提供结束入口",
    kernelOwned: "内核持有",
    owners: "{n} 个持有者",
    system: "系统",
  },

  folded: "其他 {n} 个端口",
  foldedNote: "· 应用内部 IPC / 系统服务，通常不是你的服务",
  footer: "{ports} 个端口 · 关注 {watched} · 扫描 {ms} ms · {clock}",

  /** 服务类型：推断（L0）/ 探针确认（L1）两档，提示词必须分清不许混 */
  service: {
    web: "Web",
    devserver: "Dev",
    database: "数据库",
    mcp: "MCP",
    proxy: "代理",
    debug: "调试",
    other: "服务",
    hintInferred: "由端口与进程名推断，仅供参考。点「探测」可与端口握手确认",
    hintConfirmed: "与端口握手确认的服务类型",
    hintStdio: "stdio 型 MCP 不监听端口，这里只能识别网络型 MCP",
  },

  probe: {
    action: "探测",
    running: "探测中…",
    failed: "探测失败",
    /** serverInfo / Server 头的深展示行：detail = 名字 + 版本（有空格分隔） */
    confirmedLine: "已握手确认：{detail}",
  },

  /** F17 暴露面可达实测：只报观测事实不定因；口径 caveat 必须跟随结果 */
  reach: {
    action: "测一下",
    running: "实测中…",
    failed: "实测失败",
    title: "从本机网卡地址实测一次该端口是否可达（约 2 s 超时）",
    result: "实测 {addr}：{label}（{ms} ms）",
    inferredStill: "· 推断面仍为 {exposure}",
    connected: "可达（连接成功）",
    refused: "拒连：该地址上没有进程监听（假活嫌疑）",
    timeout: "超时：无响应（原因未知，可能是拦截或丢包）",
    noroute: "无路由",
    noifaceaddr: "无网卡地址可测（未联网或无默认路由）",
    failedDetail: "失败：{detail}",
    caveat: "同机经网卡地址回连实测，不等价于局域网其他设备可达",
  },

  why: {
    action: "为什么?",
    running: "解释中…",
    again: "再解释",
    failed: "解释失败",
    title: "这个端口是怎么回事（白话解释，零网络请求）",
  },

  /** F21 入站来源：计数是口径，明细 ≤5 条给人看 */
  inbound: {
    line: "入站 {total} 条：本机 {loopback} · 局域网 {lan} · 公网 {public}{unknownSuffix}",
    more: "… 还有 {n} 个对端",
    scope: {
      loopback: "本机",
      lan: "局域网",
      public: "公网",
      unknown: "未知",
    },
  },

  /** F22 基线漂移徽标：变宽是安全信号，新监听是信息信号 */
  drift: {
    new: "新",
    widened: "面变宽",
    newTitle: "基线里没有这个监听（24 小时后自动转为老住户）",
    widenedTitle: "暴露面比基线时刻变宽（如仅本机 → 全部网卡）；持续显示直到 CLI 重建基线",
  },
} as const;

const en: MessageTree<typeof zh> = {
  title: "Ports",
  searchPlaceholder: "port / process / project",

  invisibleLead:
    "The kernel reports {total} listeners, and {invisible} of them can't be attributed to a process (without root, lsof silently skips other users' processes). Prune never escalates; run this yourself when you need to:",
  invisibleTail: ".",

  timeWait:
    "Another {n} connections are in TIME_WAIT{ports}. While the kernel holds them a new service can still hit EADDRINUSE, but no process owns them and they clear in about 2 minutes — this isn't a missed scan.",
  timeWaitPorts: " (ports {ports})",

  scanning: "Scanning listening ports…",
  emptyNeverScanned:
    "Nothing scanned yet. Hit “Refresh” in the top-right to run a round — no result here doesn't mean nothing is listening.",
  emptyCleanTitle: "All clear — nothing is listening ✂️",
  emptyCleanNote:
    "No process is listening on a port within what the current user can see. That conclusion holds because a scan ran — not because none did.",
  noMatch: "No ports match “{query}”.",
  noMatchTitle: 'No ports match "{query}"',
  noMatchNote: "Try part of a port number, process name or project, or clear the search.",
  clearSearch: "Clear search",
  rowLocate: "Locate",
  /** W2/D-09: ports declared in prune.toml but idle this round — idle is a state change, not an absence */
  configIdle: "Configured watch ports currently idle: {ports} (prune.toml present, server not running)",
  configIdleAria: "{n} configured watch ports idle",

  /** eul status ribbon: all five segments are readings this page actually acts on */
  ribbon: {
    aria: "Port status ribbon",
    listenUnit: "listening",
    listenTitle:
      "{n} listening: {attributed} attributable to a process · kernel sees {total} (blind spots are called out below)",
    exposedUnit: "exposed",
    exposedTitle:
      "{exposed} exposed: bound to 0.0.0.0 / ::, reachable from other devices on the LAN",
    watchedUnit: "focused",
    watchedTitle: "{watched} focused: ports declared in prune.toml (union across projects)",
    connsUnit: "conns",
    connsTitle:
      "{n} active connections: sum of ESTABLISHED on listening ports; {udp}",
    udpNote: "UDP has no connection notion, so {n} UDP ports are excluded",
    closeWaitUnit: "unclosed",
    closeWaitTitle:
      "{n} CLOSE_WAIT: connections this side hasn't closed — a leak signal, not a conclusion; 0 is a measured zero, not missing data",
  },

  /** eul hero: the dot matrix is the lead (one dot per port) + scan provenance block */
  hero: {
    tip: "Each dot is a listening port; hover for “port · process”. Amber = externally exposed / green = focused / grey = other (mostly system services)",
    leadNote: "listening ports · amber is externally exposed, green is focused, grey is other",
    dotAria: "Port dot matrix: {exposed} externally exposed · {watched} focused · {other} other",
    legendExposed: "Externally exposed",
    legendWatched: "Focused",
    legendOther: "Other (mostly system services)",
    scanAt: "Scanned",
    scanMs: "Took",
    attribution: "Attributed",
    rescan: "Rescan",
  },

  col: {
    port: "Port",
    exposure: "Exposure",
    process: "Process",
    project: "Project",
    conns: "Conns",
  },

  row: {
    hintKillable: "Double-click for process details · right-click to end the process",
    hintLocked:
      "This port is held by a system process or the kernel — no kill action is offered",
    kernelOwned: "Kernel-owned",
    owners: "{n} owners",
    system: "System",
  },

  folded: "{n} more ports",
  foldedNote: "· app-internal IPC / system services, usually not your own servers",
  footer: "{ports} ports · focus {watched} · scanned in {ms} ms · {clock}",

  service: {
    web: "Web",
    devserver: "Dev",
    database: "Database",
    mcp: "MCP",
    proxy: "Proxy",
    debug: "Debugger",
    other: "Service",
    hintInferred:
      "Guessed from the port and process name — hit “Probe” to confirm by handshake",
    hintConfirmed: "Service type confirmed by protocol handshake",
    hintStdio:
      "stdio MCP servers don't listen on ports — only network MCP can be detected here",
  },

  probe: {
    action: "Probe",
    running: "Probing…",
    failed: "Probe failed",
    confirmedLine: "Confirmed by handshake: {detail}",
  },

  reach: {
    action: "Reach test",
    running: "Testing…",
    failed: "Reach test failed",
    title: "One TCP connect from this machine's interface address (~2 s timeout)",
    result: "Tested {addr}: {label} ({ms} ms)",
    inferredStill: "· inferred exposure still {exposure}",
    connected: "reachable (connected)",
    refused: "refused: nothing listening on that address (stale-binding suspect)",
    timeout: "timed out: no response (cause unknown — could be blocked or dropped)",
    noroute: "no route",
    noifaceaddr: "no interface address to test (offline or no default route)",
    failedDetail: "failed: {detail}",
    caveat:
      "Same-machine connect via the interface address — not equivalent to reachability from other LAN devices",
  },

  why: {
    action: "Why?",
    running: "Explaining…",
    again: "Explain again",
    failed: "Explain failed",
    title: "What is this port (plain-language verdict, zero network)",
  },

  /** F21 inbound peers: counts are the truth, detail capped at 5 */
  inbound: {
    line: "{total} inbound: loopback {loopback} · LAN {lan} · public {public}{unknownSuffix}",
    more: "… {n} more peers",
    scope: {
      loopback: "loopback",
      lan: "LAN",
      public: "public",
      unknown: "unknown",
    },
  },

  /** F22 baseline drift badge: widened is a safety signal, new is informational */
  drift: {
    new: "New",
    widened: "Widened",
    newTitle: "Not in the baseline yet (auto-settles after 24 hours)",
    widenedTitle: "Exposure widened since baseline (e.g. loopback → all interfaces); persists until CLI rebase",
  },
};

export const ports = { zh, en } as const;
