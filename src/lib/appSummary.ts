/**
 * D7：应用诊断摘要（Markdown）。
 *
 * 三条纪律（与 UI 红线一致）：
 * 1. 拿不到的项写「不可读（原因）」，**不写 0**；
 * 2. 泄漏未命中写「无（未发现持续增长）」，不省略该节；
 * 3. 不贴 pid 全表（45 行太长），只给进程数 + Top 3 进程名。
 */
import { formatBytes, formatPercent } from "./format";
import { aggregateDays, lastDays } from "./useAppHistory";
import { diskEmpty, diskTotal, type AppDisk } from "../types/appdisk";
import type { HistoryBucket } from "../types/appHistory";
import type { LeakItem } from "../types/mem";
import type { Speed } from "../types/net";

const NA = (why: string) => `不可读（${why}）`;

function stamp(d = new Date()): string {
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`;
}

export interface SummaryInput {
  family: string;
  bundlePath: string | null;
  processCount: number;
  established: number | null;
  cpu: number | null;
  mem: number;
  swap: number;
  exact: boolean;
  deepAvailable: boolean;
  disk: AppDisk | null;
  diskBusy: boolean;
  net: Speed | null;
  netStatus: "ok" | "warmup" | "unavailable" | null;
  netNote?: string | null;
  ports: { port: number; label: string }[];
  history: HistoryBucket[];
  leak: LeakItem | null;
  topProcs: { name: string; bytes: number }[];
}

export function buildAppSummary(input: SummaryInput): string {
  const {
    family,
    bundlePath,
    processCount,
    established,
    cpu,
    mem,
    swap,
    exact,
    deepAvailable,
    disk,
    diskBusy,
    net,
    netStatus,
    netNote,
    ports,
    history,
    leak,
    topProcs,
  } = input;

  const cpuLine =
    cpu == null || !Number.isFinite(cpu)
      ? NA("首见或采集失败")
      : `${Math.round(cpu)}%（单核口径）`;
  const memLine = `${exact ? "" : "≥ "}${formatBytes(mem)} · ${deepAvailable ? "footprint 含换出" : "RSS 口径不含换出"}${
    swap > 0 ? `，其中换出 ${formatBytes(swap)}（${formatPercent(swap / Math.max(1, mem))}）` : ""
  }`;
  const diskLine = !disk
    ? NA(diskBusy ? "正在测量" : "尚未测量")
    : diskEmpty(disk)
      ? NA(disk.note ?? "没有 .app / CLI 工具")
      : `${formatBytes(diskTotal(disk))}（本体 ${disk.bundle ? formatBytes(disk.bundle.bytes) : "—"} / 缓存 ${
          disk.cache ? formatBytes(disk.cache.bytes) : "—"
        } / 数据 ${disk.data ? formatBytes(disk.data.bytes) : "—"}）`;
  const netLine =
    netStatus == null
      ? NA("网速尚未采样")
      : netStatus === "warmup"
        ? NA("采样预热中")
        : netStatus === "unavailable"
          ? NA(netNote ?? "nettop 不可读")
          : net
            ? `↓ ${formatBytes(net.downBps)}/s ↑ ${formatBytes(net.upBps)}/s（1 s 差分，仅非回环）`
            : NA("该应用这一拍没有流量记录");

  const buckets = history.filter((h) => h.samples > 0);
  const cpuPeaks = buckets.map((h) => h.cpuPeak).filter((v): v is number => v != null);
  const cpuAvgs = buckets.map((h) => h.cpuAvg).filter((v): v is number => v != null);
  const rssPeaks = buckets.map((h) => h.rssPeak).filter((v): v is number => v != null);
  const cpuPeak = cpuPeaks.length ? Math.max(...cpuPeaks) : null;
  const cpuAvg = cpuAvgs.length ? cpuAvgs.reduce((a, b) => a + b, 0) / cpuAvgs.length : null;
  const rssPeak = rssPeaks.length ? Math.max(...rssPeaks) : null;
  const netDown = buckets.reduce((s, h) => s + h.netDown, 0);
  const netUp = buckets.reduce((s, h) => s + h.netUp, 0);
  const coverage = buckets.length > 0 ? Math.min(1, buckets.length / (7 * 24)) : 0;
  const days = aggregateDays(history);

  const historyLines =
    buckets.length === 0
      ? ["- 还没有历史（Prune 运行满一小时后出现）"]
      : [
          `- CPU 峰值 ${cpuPeak == null ? "—" : `${Math.round(cpuPeak)}%`} · 均值 ${
            cpuAvg == null ? "—" : `${Math.round(cpuAvg)}%`
          }`,
          `- 内存峰值 ${rssPeak == null ? "—" : formatBytes(rssPeak)}（RSS 口径）`,
          `- 流量 ↓ ${formatBytes(netDown)} / ↑ ${formatBytes(netUp)} · 覆盖 ${Math.round(coverage * 100)}%`,
          `- 逐日内存峰值：${lastDays(7)
            .map((d) => {
              const a = days.get(d);
              return `${d.slice(5)} ${a?.rssPeak ? formatBytes(a.rssPeak) : "—"}`;
            })
            .join(" · ")}`,
        ];

  const leakLines = leak
    ? [
        `- +${formatBytes(leak.growth)} / ${Math.round(leak.windowSecs / 60)} min · R² ${leak.r2.toFixed(2)} · 斜率 ${Math.round(leak.slopeMbPerHour)} MB/h`,
      ]
    : ["- 无（未发现持续增长）"];

  return [
    "## Prune · 应用诊断摘要",
    "",
    `**${family}** · ${processCount} 进程 · ${established == null ? "连接数未扫" : `${established} 连接`}`,
    `- 路径：${bundlePath ?? "无 .app（系统进程 / CLI）"}`,
    `- 采集时间：${stamp()}`,
    "",
    "### 当前读数",
    "| 指标 | 值 | 口径 |",
    "|---|---|---|",
    `| CPU | ${cpuLine} | 成员 CPU 之和，可 >100% |`,
    `| 内存 | ${memLine} | ${deepAvailable ? "footprint" : "RSS"} |`,
    `| 磁盘 | ${diskLine} | du 名义大小，APFS 克隆会重复计入 |`,
    `| 网速 | ${netLine} | nettop 差分 |`,
    `| 端口 | ${
      ports.length === 0 ? "无监听端口" : ports.map((p) => `${p.port}（${p.label}）`).join("、")
    } | lsof |`,
    "",
    "### 近 7 天（小时聚合，RSS 口径）",
    ...historyLines,
    "",
    "### 泄漏证据",
    ...leakLines,
    "",
    "### 进程 Top 3（按内存）",
    ...(topProcs.length === 0
      ? ["- 无进程数据"]
      : topProcs.slice(0, 3).map((p) => `- ${p.name} · ${formatBytes(p.bytes)}`)),
    "",
    "### 环境",
    `- 采样：fast 10 s / deep 5 min / 历史保留 8 天`,
    `- UA：${typeof navigator === "undefined" ? "—" : navigator.userAgent}`,
    "",
  ].join("\n");
}
