/** 与 prune-core::net 的 serde 结构一一对应（camelCase）。改这里必须同步 Rust 侧。 */

export type Proto = "tcp" | "udp";

/** 绑定范围——安全语义的唯一来源，不是显示细节 */
export type BindScope = "loopback" | "interface" | "any" | "unknown";

/** 一条「进程 × 端口」绑定。一个端口可以有多个（双栈 / SO_REUSEPORT）。 */
export interface PortBinding {
  port: number;
  proto: Proto;
  scope: BindScope;
  /** 原始绑定地址（`*` / `127.0.0.1` / `::1` / `192.168.0.115`） */
  addr: string;
  /** 该 fd 是 IPv6 socket。双栈合并后靠它显示 `IPv4+IPv6` */
  v6: boolean;
  /** null = 内核持有，不可结束 */
  pid: number | null;
  /** 进程命令名 */
  comm: string;
  startTime: number;
  /** cwd 归属的项目目录（绝对路径）；null = 无项目线索 */
  project: string | null;
  projectLabel: string | null;
}

/** 按 `(port, proto)` 聚合后的一行 */
export interface PortEntry {
  port: number;
  proto: Proto;
  listeners: PortBinding[];
  /** ESTABLISHED 连接数；null = 本轮未统计（UDP / 采集失败） */
  conns: number | null;
  /** 聚合后的暴露面（listeners 中最宽者） */
  exposure: BindScope;
  /** false → 折进「其他 N 个」 */
  watched: boolean;
  /** 内置解释（目前只有 AirPlay 5000/7000） */
  note: string | null;
}

/** 可见性说明：没有它就只能猜「端口是不是空闲」 */
export interface Coverage {
  /** 内核口径的 LISTEN 条数。0 = 未采集到 */
  listenTotal: number;
  /** 其中能归属到进程的条数 */
  attributed: number;
  /** 非空 = 覆盖面受限，UI 必须显示这句话 */
  note: string | null;
}

/** 单个进程的 TCP 连接计数（M8 应用维度）。只有「有连接的进程」才出现。 */
export interface ConnStats {
  /** 活跃连接（ESTABLISHED） */
  established: number;
  /** 本端未关（CLOSE_WAIT）——泄漏信号，不是结论 */
  closeWait: number;
}

export interface PortSnapshot {
  /** 按 port 升序 */
  entries: PortEntry[];
  coverage: Coverage;
  /** 内核 TIME_WAIT 条数；null = 未采集 */
  timeWait: number | null;
  /** TIME_WAIT 涉及的本地端口（去重升序，上限 128） */
  timeWaitPorts: number[];
  /** pid → 连接计数。「没有记录 = 0 条」，不是「不知道」 */
  connsByPid: Record<string, ConnStats>;
  at: number;
  scanMs: number;
}

// ---------------------------------------------------------------- 派生（与 Rust 方法同义）

/** 双栈：同一端口在 v4 与 v6 各占一个 fd，合并后据此标 `IPv4+IPv6` */
export function dualStack(e: PortEntry): boolean {
  return e.listeners.some((b) => b.v6) && e.listeners.some((b) => !b.v6);
}

/** 去重后的持有进程数（SO_REUSEPORT 时会 > 1） */
export function ownerCount(e: PortEntry): number {
  return new Set(e.listeners.filter((b) => b.pid != null).map((b) => b.pid)).size;
}

/** 主展示进程：第一个有 pid 的 binding */
export function primaryOf(e: PortEntry): PortBinding | null {
  return e.listeners.find((b) => b.pid != null) ?? null;
}

/** 该端口的全部持有进程（去重，保留首个 binding 的项目信息） */
export function ownersOf(e: PortEntry): PortBinding[] {
  const seen = new Set<number>();
  const out: PortBinding[] = [];
  for (const b of e.listeners) {
    if (b.pid == null || seen.has(b.pid)) continue;
    seen.add(b.pid);
    out.push(b);
  }
  return out;
}

/** 内核有、但本进程看不到的监听数 */
export function invisibleCount(c: Coverage): number {
  return Math.max(0, c.listenTotal - c.attributed);
}

/** 暴露面的展示文案与色调。不猜：unknown 不给结论 */
export function exposureMeta(scope: BindScope): {
  label: string;
  tone: "warn" | "neutral" | "info";
  hint: string;
} {
  switch (scope) {
    case "any":
      return {
        label: "全部网卡",
        tone: "warn",
        hint: "绑定在 * / 0.0.0.0，同网段的其他设备可以连进来",
      };
    case "interface":
      return {
        label: "指定网卡",
        tone: "info",
        hint: "绑定在某个具体网卡地址，局域网内可达",
      };
    case "loopback":
      return { label: "仅本机", tone: "neutral", hint: "127.0.0.1 / ::1，只有本机进程能连" };
    default:
      return { label: "未知", tone: "neutral", hint: "地址解析失败，不对暴露面下结论" };
  }
}

/** 关注层（watched）与折叠层的切分 */
export function splitLayers(entries: PortEntry[]): {
  watched: PortEntry[];
  folded: PortEntry[];
} {
  return {
    watched: entries.filter((e) => e.watched),
    folded: entries.filter((e) => !e.watched),
  };
}

/** 某进程持有的端口（应用列表徽标 / 进程详情用） */
export function portsOfPid(entries: PortEntry[], pid: number): PortEntry[] {
  return entries.filter((e) => e.listeners.some((b) => b.pid === pid));
}

/**
 * 一组 pid 的连接汇总。后端语义是「没有记录 = 0 条」，
 * 所以这里缺失的 pid 直接跳过，不返回 null——与端口不同，连接没有
 * 「采集失败」这个状态可区分（lsof 拿不到就是拿不到，也不会骗人）。
 */
export function connsOfPids(
  connsByPid: Record<string, ConnStats> | undefined,
  pids: number[],
): ConnStats {
  const out: ConnStats = { established: 0, closeWait: 0 };
  if (!connsByPid) return out;
  for (const pid of pids) {
    const c = connsByPid[String(pid)];
    if (!c) continue;
    out.established += c.established;
    out.closeWait += c.closeWait;
  }
  return out;
}

/**
 * 连接徽标的展示阈值：< 5 不显示（沉默即正常）。
 * 实测分布 42 / 21 / 14 / 9 / 8，阈值 5 能突出真异常而不让每行都长徽标。
 */
export const CONN_BADGE_MIN = 5;
export const CONN_BADGE_WARN = 20;

/**
 * 系统关键进程（与 Rust `SYSTEM_COMMS` 同源；那边管「不占关注层 + 禁杀」）。
 * UI 用它决定**不给结束入口**——端口列表里结束 ControlCenter 只会让 Wi-Fi 掉一下，
 * 对用户零收益。
 */
const SYSTEM_COMMS = new Set(
  [
    "ControlCenter",
    "rapportd",
    "sharingd",
    "mDNSResponder",
    "launchd",
    "configd",
    "netbiosd",
    "sshd",
    "cupsd",
    "rpcbind",
    "SystemUIServer",
    "WindowServer",
    "identityservicesd",
    "remoted",
    "AirPlayUIAgent",
    "svchost.exe",
    "System",
    "lsass.exe",
  ].map((s) => s.toLowerCase()),
);

export function isSystemProcess(comm: string): boolean {
  return SYSTEM_COMMS.has(comm.toLowerCase());
}

/** 该行是否允许出现「结束进程」入口：系统进程 / 内核持有（pid null）/ pid ≤ 1 一律不给 */
export function canKillOwner(b: PortBinding): boolean {
  return b.pid != null && b.pid > 1 && !isSystemProcess(b.comm);
}

/** 浏览器打开用的地址：恒定 loopback，不用 `0.0.0.0` / 局域网 IP */
export function localUrl(port: number): string {
  return `http://127.0.0.1:${port}`;
}
