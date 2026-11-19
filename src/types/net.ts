/** 与 prune-core::net 的 serde 结构一一对应（camelCase）。改这里必须同步 Rust 侧。 */
import { i18nFor } from "../i18n";
import { currentLocale } from "../i18n/store";
import type { Locale } from "../i18n/types";

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
  /** 服务类型（L0 静态推断）；null = 推断不出，**不是「不是服务」** */
  service: ServiceKind | null;
  /** F21 入站来源摘要；null = 本端口没有 ESTABLISHED 连接或本轮未统计 */
  inbound?: PortPeers | null;
  /** F22 基线漂移徽标；null = 无漂移 / 基线未建。判定在 Rust 侧算好随轮下发 */
  drift?: DriftKind | null;
}

/** F22 基线漂移类别（Rust `baseline::DriftKind`，serde lowercase）。
 *  信息量排序：widened（暴露面变宽）> new（新监听）> 消失（不进 UI） */
export type DriftKind = "new" | "widened";

/** F21 连接对端来源段（Rust `PeerScope`，serde lowercase）。只分类不定因 */
export type PeerScope = "loopback" | "lan" | "public" | "unknown";

/** F21 单条入站来源明细（数字字面量，无反解析） */
export interface PeerEntry {
  addr: string;
  scope: PeerScope;
}

/** F21 一个监听端口的入站来源摘要（Rust `PortPeers`，camelCase）。
 *  计数是口径（含明细截断掉的部分），明细封顶 32 条 */
export interface PortPeers {
  loopback: number;
  lan: number;
  public: number;
  unknown: number;
  detail: PeerEntry[];
}

/** 端口提供的服务类型（与 Rust `ServiceKind` 同源，serde lowercase） */
export type ServiceKind = "web" | "devserver" | "database" | "mcp" | "proxy" | "debug" | "other";

/** L1 探针结果（Rust `probe::ProbeOutcome`）。「与端口握过手」的确认级结论 */
export interface ProbeOutcome {
  kind: ServiceKind;
  /** HTTP Server 头 / MCP serverInfo.name / SSH 版本串；null = 协议无自述 */
  detail: string | null;
  /** 自述版本（MCP serverInfo.version）；null = 协议未提供 */
  version: string | null;
}

/** F17 暴露面可达实测的四态结论（Rust `reach::ReachOutcome`，serde internally-tagged） */
export type ReachOutcome =
  | { outcome: "connected" }
  | { outcome: "refused" }
  | { outcome: "timeout" }
  | { outcome: "noroute" }
  | { outcome: "noifaceaddr" }
  | { outcome: "failed"; detail: string };

/** F17 一次实测的完整产出（Rust `reach::ReachResult`，camelCase） */
export interface ReachResult {
  port: number;
  /** 实际回连的本机网卡地址；null = 无可用非 loopback 地址 */
  triedAddr: string | null;
  outcome: ReachOutcome;
  elapsedMs: number;
  /** 固定口径说明：同机回连 ≠ 其他设备可达 */
  caveat: string;
}

/** F16 白话解释的结论类别（Rust `why::WhyKind`，serde lowercase）。
 *  文案呈现按类别区分语气：system/conflict → warn，dev/know → accent，free → 中性 */
export type WhyKind =
  | "systemservice"
  | "devserver"
  | "conflict"
  | "knownservice"
  | "timewait"
  | "free"
  | "unknown";

/** F16 一次白话解释的完整产出（Rust `why::WhyVerdict`，camelCase）。
 *  headline / evidence / actionHint 是核心层产出的中文单语文案
 *  （CLI 决策 W1-DP-6），与 CLI / MCP 三个消费者同一契约 */
export interface WhyVerdict {
  port: number;
  kind: WhyKind;
  /** 一句话结论 */
  headline: string;
  /** 证据行：进程 / pid / 绑定 / 项目 / 连接 / Coverage 口径 */
  evidence: string[];
  /** 指路（只指路不执行）：关闭路径、项目目录、自查命令 */
  actionHint: string | null;
}

/** F19 自耗读数（Rust `self_footprint::SelfFootprint`，camelCase）。
 *  null = 该项暂无数据（显示「暂无」而不是 0） */
export interface SelfFootprint {
  pid: number;
  /** phys_footprint 口径，与内存页同源 */
  footprint: number | null;
  threads: number | null;
  /** 自身 CPU 多轮均值（%）；null = 采样不足 */
  cpuPctAvg: number | null;
  cpuSamples: number;
  /** 序列库（prune.db 含 -wal / -shm）字节数 */
  dbBytes: number | null;
  /** 采样时刻（Unix 秒） */
  sampleAt: number;
}

/**
 * 服务类型的展示文案。标签走静态 key（MessageKey 是严格类型，动态拼接过不了编译）；
 * 提示词区分「推断」与「确认」两档——推断不准充「确认」就是撒谎。
 */
export function serviceLabel(kind: ServiceKind, locale: Locale = currentLocale()): string {
  const { t } = i18nFor(locale);
  switch (kind) {
    case "web":
      return t("ports.service.web");
    case "devserver":
      return t("ports.service.devserver");
    case "database":
      return t("ports.service.database");
    case "mcp":
      return t("ports.service.mcp");
    case "proxy":
      return t("ports.service.proxy");
    case "debug":
      return t("ports.service.debug");
    default:
      return t("ports.service.other");
  }
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
  /**
   * 项目 `prune.toml` 声明的关注端口并集（W2/D-09；旧快照缺省为空）。
   * UI 用它显示「配置了关注但当前空闲」的端口——空闲是状态变化，不是没有
   */
  configuredWatchPorts?: number[];
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

/**
 * 暴露面的展示文案与色调。不猜：unknown 不给结论。
 *
 * `locale` 必须显式传（门禁禁止纯函数读全局）：切语言时组件重渲染会重新调用，
 * 不会留下旧语言的字符串。
 */
export function exposureMeta(
  scope: BindScope,
  locale: Locale = currentLocale(),
): {
  label: string;
  tone: "warn" | "neutral" | "info";
  hint: string;
} {
  const { t } = i18nFor(locale);
  switch (scope) {
    case "any":
      return {
        label: t("net.exposure.any"),
        tone: "warn",
        hint: t("net.exposure.hintAny"),
      };
    case "interface":
      return {
        label: t("net.exposure.interface"),
        tone: "info",
        hint: t("net.exposure.hintInterface"),
      };
    case "loopback":
      return {
        label: t("net.exposure.loopback"),
        tone: "neutral",
        hint: t("net.exposure.hintLoopback"),
      };
    default:
      return {
        label: t("net.exposure.unknown"),
        tone: "neutral",
        hint: t("net.exposure.hintUnknown"),
      };
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

// ---------------------------------------------------------------- 网速（M10）

/** 单个进程的即时速率（字节/秒） */
export interface Speed {
  downBps: number;
  upBps: number;
}

/**
 * 采样状态。三种都必须能被 UI 区分：
 * - `ok` 正常出数
 * - `warmup` 只有第一轮累计值，还没基线 → 显示「采样中…」
 * - `unavailable` nettop 缺失/失败 → 显示「不可读 + 原因」，**绝不显示 0**
 */
export type NetSpeedStatus = "ok" | "warmup" | "unavailable";

export interface NetSpeedSample {
  at: number;
  status: NetSpeedStatus;
  note: string | null;
  /** pid → 速率。只含本样本里出现的进程 */
  perPid: Record<string, Speed>;
  totalDownBps: number;
  totalUpBps: number;
  /** 与上一轮的实际间隔（毫秒）。首轮 warmup 时是标称间隔 */
  intervalMs: number;
}

/**
 * 一组 pid 的速率之和（家族口径），与 CPU / 内存一致。
 *
 * 缺失的 pid 按 0 处理：nettop 只列出「有流量的进程」，
 * 没有记录就是这一秒没有流量，而不是「不知道」。
 */
export function netSpeedOfPids(
  sample: NetSpeedSample | null | undefined,
  pids: number[],
): Speed {
  const out: Speed = { downBps: 0, upBps: 0 };
  if (!sample || sample.status !== "ok") return out;
  for (const pid of pids) {
    const s = sample.perPid[String(pid)];
    if (!s) continue;
    out.downBps += s.downBps;
    out.upBps += s.upBps;
  }
  return out;
}

/** 速率展示：B/s → 自动单位；0 显示「0」，null 由调用方负责「—」 */
export function formatSpeed(bps: number): string {
  if (!Number.isFinite(bps) || bps < 1) return "0 B/s";
  const units = ["B/s", "KB/s", "MB/s", "GB/s"];
  let v = bps;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i += 1;
  }
  return `${v >= 100 ? Math.round(v) : v.toFixed(1)} ${units[i]}`;
}

// ---------------------------------------------------------------- 网络总览（M14）
//
// 与 prune-core::net::traffic 一一对应（camelCase）。改这里必须同步 Rust 侧。

/** 网卡类型：只有 physical 计入主数字（虚拟口与物理口统计同一批字节） */
export type IfaceKind = "physical" | "tunnel" | "virtual" | "loopback";

/** 采样状态三态，UI 必须区分（warmup / unavailable 都不能显示 0） */
export type NetTotalStatus = "ok" | "warmup" | "unavailable";

export interface IfaceRate {
  name: string;
  kind: IfaceKind;
  downBps: number;
  upBps: number;
  /** 本次会话该网卡的正增量累计 */
  sessionDownBytes: number;
  sessionUpBytes: number;
}

/** 一天的流量（本地日期，仅物理口，仅 Prune 运行期间） */
export interface DayTraffic {
  /** `YYYY-MM-DD` */
  day: string;
  downBytes: number;
  upBytes: number;
}

export interface NetTotalPoint {
  at: number;
  downBps: number;
  upBps: number;
}

export interface NetTotalSample {
  at: number;
  status: NetTotalStatus;
  /** 非空 = 必须显示给用户的原因（不可读 / 今日流量未落库） */
  note: string | null;
  /** 物理口合计 */
  downBps: number;
  upBps: number;
  /** 上一拍的真实间隔（毫秒） */
  intervalMs: number;
  /** 物理口在前、虚拟口在后，各自按速率降序 */
  ifaces: IfaceRate[];
  sessionDownBytes: number;
  sessionUpBytes: number;
  /** 峰值固定为「本次会话最大值」，与走势窗口解耦 */
  peakDownBps: number;
  peakUpBps: number;
  peakAt: number;
  /** 今日。null = 未落库 / 库不可用 —— **不是 0** */
  today: DayTraffic | null;
  /** 上次成功落库时刻（0 = 还没落过） */
  todayPersistedAt: number;
}

/** 网卡类型的展示文案（虚拟口那一行必须自带「不计入」的解释） */
export function ifaceKindLabel(kind: IfaceKind, locale: Locale = currentLocale()): string {
  const { t } = i18nFor(locale);
  switch (kind) {
    case "tunnel":
      return t("net.kind.tunnel");
    case "virtual":
      return t("net.kind.virtual");
    case "loopback":
      return t("net.kind.loopback");
    default:
      return t("net.kind.physical");
  }
}

/**
 * 拆分网卡列表：物理口一行一行显示，虚拟口折叠。
 *
 * 折叠不是为了省地方，是为了不抢注意力 —— 一排 `awdl0 0 B/s` 会让人
 * 以为网卡列表坏了。信息不能丢，所以折叠区仍然显示当前速率。
 */
export function splitIfaces(ifaces: IfaceRate[]): {
  physical: IfaceRate[];
  virtual: IfaceRate[];
} {
  return {
    physical: ifaces.filter((i) => i.kind === "physical"),
    virtual: ifaces.filter((i) => i.kind !== "physical"),
  };
}

/** 单张网卡的合计速率（排序与占比条用，与 Rust `IfaceRate::total_bps` 同义） */
export function ifaceTotalBps(i: IfaceRate): number {
  return i.downBps + i.upBps;
}

/**
 * 窗口内的**时间加权**平均速率。
 *
 * 用点间隔做权重而不是简单平均：采样抖动（漏拍、2 s 节拍）时简单平均
 * 会让短间隔的尖峰被过度放大。
 */
export function weightedAverage(points: NetTotalPoint[]): { downBps: number; upBps: number } {
  if (points.length < 2) return { downBps: 0, upBps: 0 };
  let down = 0;
  let up = 0;
  let span = 0;
  for (let i = 1; i < points.length; i += 1) {
    const dt = Math.max(0, points[i].at - points[i - 1].at);
    if (dt === 0) continue;
    down += points[i].downBps * dt;
    up += points[i].upBps * dt;
    span += dt;
  }
  if (span === 0) return { downBps: 0, upBps: 0 };
  return { downBps: down / span, upBps: up / span };
}

// ---------------------------------------------------------------- 网络诊断（net-diag-plan）
//
// 与 prune_core::net::diag 一一对应（camelCase）。改这里必须同步 Rust 侧。

/** 诊断状态机。failed = 探测机制本身失败；「连不上路由器」是 done + 结论，两者严格区分 */
export type DiagStatus = "idle" | "running" | "done" | "failed";

/** captive portal：none = 正常公网；suspected = 疑似登录页；unknown = 探测失败不下结论 */
export type CaptiveState = "none" | "suspected" | "unknown";

/** 单层探测。medianMs null = 探测失败（**不是 0**） */
export interface LayerProbe {
  medianMs: number | null;
  /** 原始样本，UI 可展开 */
  samples: number[];
  note: string | null;
}

/** 分层结论。规则与阈值一并给 UI，不搞黑盒 */
export interface NetVerdict {
  label: string;
  advice: string;
  rule: string;
}

export interface NetDiag {
  at: number;
  status: DiagStatus;
  /** 非空 = 必须显示的原因（failed 时必填） */
  note: string | null;
  dns: LayerProbe | null;
  lan: LayerProbe | null;
  wan: LayerProbe | null;
  /** 出口抖动 = WAN 样本平均绝对偏差（MAD） */
  jitterMs: number | null;
  gateway: string | null;
  captive: CaptiveState | null;
  verdict: NetVerdict | null;
  /** 报告用。读不到就 null，不下假结论 */
  ssid: string | null;
}

/** 手动测速（macOS networkQuality）。Windows 上整卡隐藏（capability 门控） */
export interface SpeedResult {
  at: number;
  /** bps，前端格式化 */
  downBps: number;
  upBps: number;
  /** Apple「responsiveness」：负载下响应度，bufferbloat 的官方口径 */
  rpm: number;
  /** Apple 原文分级，前端映射中文 */
  rpmLevel: string;
  baseRttMs: number;
}

export interface NetDiagCapability {
  speedTest: boolean;
}

/** 判定阈值（§4.3）。与 Rust LAN_SLOW_MS 等常量同步，UI 逐行展示——阈值不搞黑盒 */
export const DIAG_THRESHOLDS = {
  lanMs: 50,
  wanLanGapMs: 100,
  jitterMs: 30,
  dnsMs: 300,
} as const;

/** Apple RPM 分级 → 展示词（自造公式不做，只做文案映射） */
export function rpmLevelLabel(level: string, locale: Locale = currentLocale()): string {
  const { t } = i18nFor(locale);
  switch (level) {
    case "Poor":
      return t("net.speed.level.poor");
    case "Low":
      return t("net.speed.level.low");
    case "Medium":
      return t("net.speed.level.medium");
    case "Good":
      return t("net.speed.level.good");
    default:
      // 未知分级原样透传（Apple 改了分级也不该显示成空白）
      return level || "—";
  }
}

// ---------------------------------------------------------------- 站点检测（F15，docs/net-sites-plan.md）
//
// 与 prune_core::net::sites 一一对应（camelCase）。改这里必须同步 Rust 侧。
//
// 三条纪律来自方案 §4：
// - 五个阶段各自可为 null（没走到这一步），**永不渲染成 0 ms**；
// - `refused`（4xx/5xx）不是失败，链路是通的；
// - 速率样本不足时 `transferBps` 为 null，UI 只报样本量，不报数字。

/** 站表里的一站。`id` 跨保存稳定（进度事件与单站重测按它定位） */
export interface Site {
  id: string;
  name: string;
  url: string;
}

/** ok = 正常；slow = 有阶段超阈值；stalled = 握手过了但首字节不到；
 *  refused = 站点返回 4xx/5xx（**不是连不上**）；unreachable = DNS/TCP/TLS 没打通 */
export type SiteVerdict = "ok" | "slow" | "stalled" | "refused" | "unreachable" | "error";

/** 断在哪一步，只用于文案与着色 */
export type FailStage = "dns" | "tcp" | "tls" | "ttfb" | "transfer" | "http";

/** 单站一次探测。五个时刻都是**累计值**（ms），派生增量在 `siteDelta` 里做 */
export interface SiteProbe {
  id: string;
  /** 探测当时的名称 / URL 快照：站表随后可能被改，报告要记录实际打的地址 */
  name: string;
  url: string;
  at: number;
  dnsMs: number | null;
  tcpMs: number | null;
  tlsMs: number | null;
  ttfbMs: number | null;
  totalMs: number | null;
  bytes: number;
  httpCode: number | null;
  remoteIp: string | null;
  /** bytes / (total − ttfb)；样本不足为 null */
  transferBps: number | null;
  verdict: SiteVerdict;
  stage: FailStage | null;
  /** curl 原文（技术细节，展开行里展示；用户看到的结论来自 verdict + stage） */
  note: string | null;
  curlExit: number | null;
}

export interface SiteRun {
  at: number;
  results: SiteProbe[];
  cancelled: boolean;
  elapsedMs: number;
}

export interface SiteList {
  sites: Site[];
  customized: boolean;
  /** true = 存下的站表读不出来，已回落默认（必须提示，不能静默替换） */
  recovered: boolean;
}

export interface SiteSummary {
  ok: number;
  slow: number;
  stalled: number;
  refused: number;
  unreachable: number;
  error: number;
  total: number;
}

/** curl 能力：false 时整页换成说明，不渲染带 0 的空表 */
export interface SiteCapability {
  curl: boolean;
  version: string | null;
}

/** 判慢阈值（§4.4）。与 Rust 侧同名常量同步，UI 页脚逐行展示——阈值不搞黑盒 */
export const SITE_THRESHOLDS = {
  tcpMs: 300,
  tlsMs: 400,
  ttfbMs: 1000,
  slowBps: 100_000,
  minSpeedBytes: 200_000,
  minSpeedMs: 300,
  maxTimeSec: 8,
  maxSites: 20,
} as const;

/** 阶段增量：`later − earlier`，任一缺失返回 null（不用 0 兜底） */
export function siteDelta(later: number | null, earlier: number | null): number | null {
  if (later == null || earlier == null) return null;
  const d = later - earlier;
  return d > 0 ? d : null;
}

/** 速率展示：KB/s 起步，≥ 1 MB/s 换单位。null = 样本不足，由调用方决定文案 */
export function formatRate(bps: number | null): string {
  if (bps == null) return "—";
  return bps >= 1_000_000 ? `${(bps / 1_000_000).toFixed(1)} MB/s` : `${Math.round(bps / 1000)} KB/s`;
}

/** 状态 → 语义色（沿用既有三色板，不新造色） */
export function siteTone(v: SiteVerdict): "accent" | "warn" | "danger" | "neutral" {
  switch (v) {
    case "ok":
      return "accent";
    case "slow":
    case "refused":
      return "warn";
    case "stalled":
    case "unreachable":
      return "danger";
    default:
      return "neutral";
  }
}

/** 汇总计数（顺序即 UI 展示顺序） */
export function summarizeSites(probes: SiteProbe[]): SiteSummary {
  const s: SiteSummary = {
    ok: 0,
    slow: 0,
    stalled: 0,
    refused: 0,
    unreachable: 0,
    error: 0,
    total: probes.length,
  };
  for (const p of probes) s[p.verdict] += 1;
  return s;
}

// ---------------------------------------------------------------- 冲突（W2/D-07）

/**
 * 当前冲突端口列表。与 `prune_core::net::ports::conflicts` 同一口径：
 * **TCP** 且同端口有 **≥2 个不同 pid**（双栈同 pid 多 fd 不算）。
 * 总览卡 / 菜单栏 / 端口页三处都从同一份快照派生，不各算一遍。
 */
export interface PortConflict {
  port: number;
  /** 去重后的持有 pid 数 */
  owners: number;
  /** 持有者（按 pid 升序） */
  holders: PortBinding[];
}

export function portConflicts(entries: PortEntry[]): PortConflict[] {
  const out: PortConflict[] = [];
  for (const e of entries) {
    if (e.proto !== "tcp") continue;
    const owners: PortBinding[] = [];
    const seen = new Set<number>();
    for (const b of e.listeners) {
      if (b.pid == null || seen.has(b.pid)) continue;
      seen.add(b.pid);
      owners.push(b);
    }
    if (owners.length > 1) {
      owners.sort((a, b) => (a.pid ?? 0) - (b.pid ?? 0));
      out.push({ port: e.port, owners: owners.length, holders: owners });
    }
  }
  return out.sort((a, b) => a.port - b.port);
}
