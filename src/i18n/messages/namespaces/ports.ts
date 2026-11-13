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
  subPorts: "按端口反查持有进程 · 关注层是开发服务与数据库端口 · 内部 IPC 默认折叠",
  subLan: "同网段设备与暴露面 · 手动触发，进入分段不扫描（首次扫描 macOS 会请求本地网络授权）",
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

  board: {
    title: "端口仪表盘",
    summary: "关注 {watched} · 对外 {exposed} · 其他 {other}（已折叠）",
    dotAria: "端口点阵：关注层 {watched}，对外暴露 {exposed}，其他 {other}",
    barAria: "端口分组：关注层 {watched}，对外暴露 {exposed}，其他 {other}",
    segWatched: "关注层",
    segExposed: "对外暴露",
    segOther: "其他",
    /** 点阵里单点的 title：内核持有的端口没有 comm */
    kernelOwned: "内核持有",
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
  },

  subpage: {
    ports: "端口",
    lan: "局域网",
    aria: "端口子页",
  },
} as const;

const en: MessageTree<typeof zh> = {
  title: "Ports",
  subPorts:
    "Look up the owning process by port · the focus layer is dev servers and database ports · internal IPC is folded by default",
  subLan:
    "Devices on your subnet and what you expose · manual only; switching subpages never scans (the first scan triggers macOS's local-network prompt)",
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

  board: {
    title: "Port dashboard",
    summary: "Focus {watched} · exposed {exposed} · other {other} (folded)",
    dotAria: "Port dot matrix: focus layer {watched}, externally exposed {exposed}, other {other}",
    barAria: "Port groups: focus layer {watched}, externally exposed {exposed}, other {other}",
    segWatched: "Focus layer",
    segExposed: "Externally exposed",
    segOther: "Other",
    kernelOwned: "Kernel-owned",
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
  },

  subpage: {
    ports: "Ports",
    lan: "LAN",
    aria: "Port subpages",
  },
};

export const ports = { zh, en } as const;
