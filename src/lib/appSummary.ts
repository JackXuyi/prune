/**
 * D7：应用诊断摘要（Markdown）。
 *
 * 三条纪律（与 UI 红线一致）：
 * 1. 拿不到的项写「不可读（原因）」，**不写 0**；
 * 2. 泄漏未命中写「无（未发现持续增长）」，不省略该节；
 * 3. 不贴 pid 全表（45 行太长），只给进程数 + Top 3 进程名。
 *
 * i18n：报告是**生成方拼的整句**，所以每句在 `resources.summary.*` 里存一个完整 key，
 * 不在运行时拼片段 —— 片段拼接出来的英文语序会崩。`locale` 必须显式传入（不在函数内部
 * 读全局），否则「复制过一次摘要后切语言，再复制还是旧语言」。
 */
import type { Locale } from "../i18n/types";
import { i18nFor } from "../i18n";
import { formatBytes, formatPercent } from "./format";
import { aggregateDays, lastDays } from "./useAppHistory";
import { diskEmpty, diskTotal, type AppDisk } from "../types/appdisk";
import type { HistoryBucket } from "../types/appHistory";
import type { LeakItem } from "../types/mem";
import type { Speed } from "../types/net";

function stamp(d = new Date()): string {
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`;
}

export interface SummaryInput {
  /** 报告语言：调用方从 `useI18n()` 取，函数内不读全局 */
  locale: Locale;
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
    locale,
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
  const { t } = i18nFor(locale);
  const na = (why: string) => t("resources.summary.na", { why });

  const cpuLine =
    cpu == null || !Number.isFinite(cpu)
      ? na(t("resources.summary.cpuNaWhy"))
      : t("resources.summary.cpu", { pct: Math.round(cpu) });
  const memLine = `${exact ? "" : "≥ "}${formatBytes(mem)} · ${
    deepAvailable ? t("resources.summary.memFootprint") : t("resources.summary.memRss")
  }${
    swap > 0
      ? t("resources.summary.memSwap", {
          size: formatBytes(swap),
          pct: formatPercent(swap / Math.max(1, mem)),
        })
      : ""
  }`;
  const diskLine = !disk
    ? na(diskBusy ? t("resources.summary.diskBusy") : t("resources.summary.diskNotMeasured"))
    : diskEmpty(disk)
      ? na(disk.note ?? t("resources.summary.diskNoApp"))
      : t("resources.summary.diskLine", {
          total: formatBytes(diskTotal(disk)),
          bundle: disk.bundle ? formatBytes(disk.bundle.bytes) : "—",
          cache: disk.cache ? formatBytes(disk.cache.bytes) : "—",
          data: disk.data ? formatBytes(disk.data.bytes) : "—",
        });
  const netLine =
    netStatus == null
      ? na(t("resources.summary.netNotSampled"))
      : netStatus === "warmup"
        ? na(t("resources.summary.netWarmup"))
        : netStatus === "unavailable"
          ? na(netNote ?? t("resources.summary.netNoNettop"))
          : net
            ? t("resources.summary.netLine", {
                down: formatBytes(net.downBps),
                up: formatBytes(net.upBps),
              })
            : na(t("resources.summary.netNoTraffic"));

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
      ? [t("resources.summary.historyEmpty")]
      : [
          t("resources.summary.cpuPeakLine", {
            peak: cpuPeak == null ? "—" : `${Math.round(cpuPeak)}%`,
            avg: cpuAvg == null ? "—" : `${Math.round(cpuAvg)}%`,
          }),
          t("resources.summary.memPeakLine", {
            peak: rssPeak == null ? "—" : formatBytes(rssPeak),
          }),
          t("resources.summary.trafficLine", {
            down: formatBytes(netDown),
            up: formatBytes(netUp),
            pct: Math.round(coverage * 100),
          }),
          t("resources.summary.dailyLine", {
            days: lastDays(7)
              .map((d) => {
                const a = days.get(d);
                return `${d.slice(5)} ${a?.rssPeak ? formatBytes(a.rssPeak) : "—"}`;
              })
              .join(" · "),
          }),
        ];

  const leakLines = leak
    ? [
        t("resources.summary.leakLine", {
          growth: formatBytes(leak.growth),
          minutes: Math.round(leak.windowSecs / 60),
          r2: leak.r2.toFixed(2),
          slope: Math.round(leak.slopeMbPerHour),
        }),
      ]
    : [t("resources.summary.leakNone")];

  return [
    t("resources.summary.title"),
    "",
    t("resources.summary.headline", {
      family,
      procs: processCount,
      conns:
        established == null
          ? t("resources.summary.connsUnscanned")
          : t("resources.summary.conns", { n: established }),
    }),
    t("resources.summary.path", { path: bundlePath ?? t("resources.summary.pathNone") }),
    t("resources.summary.collectedAt", { time: stamp() }),
    "",
    t("resources.summary.current"),
    t("resources.summary.tableHead"),
    t("resources.summary.tableSep"),
    t("resources.summary.rowCpu", { value: cpuLine }),
    t("resources.summary.rowMem", { value: memLine, basis: deepAvailable ? "footprint" : "RSS" }),
    t("resources.summary.rowDisk", { value: diskLine }),
    t("resources.summary.rowNet", { value: netLine }),
    t("resources.summary.rowPorts", {
      value:
        ports.length === 0
          ? t("resources.summary.portsNone")
          : ports
              .map((p) => t("resources.summary.portItem", { port: p.port, label: p.label }))
              .join(t("resources.summary.portsJoin")),
    }),
    "",
    t("resources.summary.historySection"),
    ...historyLines,
    "",
    t("resources.summary.leakSection"),
    ...leakLines,
    "",
    t("resources.summary.topSection"),
    ...(topProcs.length === 0
      ? [t("resources.summary.topNone")]
      : topProcs.slice(0, 3).map((p) => `- ${p.name} · ${formatBytes(p.bytes)}`)),
    "",
    t("resources.summary.envSection"),
    t("resources.summary.envSampling"),
    t("resources.summary.envUa", {
      ua: typeof navigator === "undefined" ? "—" : navigator.userAgent,
    }),
    "",
  ].join("\n");
}
