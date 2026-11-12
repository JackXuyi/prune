import { useEffect, useMemo, useState } from "react";

import type { DayTraffic, IfaceRate, LayerProbe, NetTotalSample, NetTotalStatus } from "../types/net";
import { formatBytes, formatClock, recentDays } from "../lib/format";
import { i18nFor } from "../i18n";
import { useI18n } from "../i18n/store";
import type { Locale } from "../i18n/types";
import { useMemStore } from "../stores/memStore";
import { useNetDiagStore } from "../stores/netDiagStore";
import { useNetSpeedStore } from "../stores/netStore";
import { TRAFFIC_DAYS, useNetTotalStore } from "../stores/netTotalStore";
import {
  DIAG_THRESHOLDS,
  formatMbps,
  formatSpeed,
  ifaceKindLabel,
  ifaceTotalBps,
  netSpeedOfPids,
  probeTone,
  rpmLevelLabel,
  splitIfaces,
  weightedAverage,
} from "../types/net";
import { Badge } from "./primitives";
import { MeterBar } from "./charts/MeterBar";
import { AreaChart } from "./charts/AreaChart";
import { TrackProgress, type TrackNode } from "./charts/TrackProgress";

/**
 * 网络 tab（M14 / M17）：本机下载 / 上传速率、走势、网卡分布、今日流量、按应用排名；
 * M17 起拆成「监测 / 诊断」两个二级页，诊断页图表化展示分层探测结果。
 *
 * 三条纪律（方案 §3 / §7）：
 * - 主数字只算**物理网卡**：开 VPN 时 `utun3` 与 `en0` 是同一批字节，相加等于双计
 * - `warmup` 显示「采样中…」、`unavailable` 显示「不可读 + 原因」，**都不显示 0**
 * - 「今日」是 **Prune 运行期间累计**：系统计数器只给「开机至今」，未运行时段无法回填
 *
 * 组件内不订阅、不采样：数据全部来自 store，保持「谁可见谁负责」的既有分工。
 */
type NetSubPage = "monitor" | "diag";

export function NetBoard({ onOpenResources }: { onOpenResources: () => void }) {
  const { t } = useI18n();
  const sample = useNetTotalStore((s) => s.sample);
  const history = useNetTotalStore((s) => s.history);
  const days = useNetTotalStore((s) => s.days);

  const [page, setPage] = useState<NetSubPage>("monitor");

  const status: NetTotalStatus = sample?.status ?? "warmup";
  const { physical, virtual } = splitIfaces(sample?.ifaces ?? []);
  const avg = useMemo(() => weightedAverage(history), [history]);
  const downSeries = useMemo(() => history.map((p) => p.downBps), [history]);
  const upSeries = useMemo(() => history.map((p) => p.upBps), [history]);

  return (
    <div className="min-h-0 flex-1 overflow-y-auto px-5 py-4">
      <div className="flex items-center justify-between">
        {/* 页名在顶栏（spec §1.3）；副标区分「监控 / 诊断」两个子页，保留 */}
        <p className="text-[11px] text-ink-faint">
          {page === "monitor" ? t("net.subMonitor") : t("net.subDiag")}
        </p>
        <Segmented page={page} onChange={setPage} />
      </div>

      {page === "monitor" ? (
        <>
      {status === "unavailable" ? (
        <p className="mt-3 rounded-lg bg-warn/[0.08] px-3 py-2 text-[11px] leading-relaxed text-warn">
          {t("net.unavailable", { why: sample?.note ?? t("net.unavailableFallback") })}
        </p>
      ) : null}

      <div className="mt-4 grid grid-cols-2 gap-8 border-b border-hairline-soft pb-4">
        <DirectionReading
          arrow="↓"
          label={t("net.down")}
          tone="accent"
          chartColor="var(--color-domain-net)"
          status={status}
          bps={sample?.downBps ?? 0}
          values={downSeries}
        />
        <DirectionReading
          arrow="↑"
          label={t("net.up")}
          tone="info"
          chartColor="var(--color-domain-cpu)"
          status={status}
          bps={sample?.upBps ?? 0}
          values={upSeries}
        />
      </div>

      <Aggregates sample={sample} avgDown={avg.downBps} avgUp={avg.upBps} />

      <section className="mt-4 border-b border-hairline-soft pb-4">
        <SectionHead title={t("net.days.title")} hint={t("net.days.hint")} />
        <DayBars days={days} count={TRAFFIC_DAYS} />
      </section>

      <section className="mt-4 border-b border-hairline-soft pb-4">
        <SectionHead title={t("net.iface.title")} hint={t("net.iface.hint")} />
        {status === "warmup" ? (
          <p className="mt-2 text-[11px] text-ink-faint">{t("net.iface.warmup")}</p>
        ) : physical.length === 0 ? (
          <p className="mt-2 text-[11px] text-ink-faint">{t("net.iface.empty")}</p>
        ) : (
          <ul className="mt-2 space-y-1.5">
            {physical.map((i) => (
              <IfaceRow key={i.name} iface={i} maxBps={maxRate(physical)} />
            ))}
          </ul>
        )}
        <VirtualIfaces ifaces={virtual} maxBps={maxRate(physical)} />
      </section>

      <AppRanking netStatus={status} onOpenResources={onOpenResources} />

      <p className="mt-3 text-[10px] leading-relaxed text-ink-faint">{t("net.footnote")}</p>
        </>
      ) : (
        <DiagPage />
      )}
    </div>
  );
}

// ---------------------------------------------------------------- 主读数

/** 大字读数 + 独立走势（各自归一化：上下行量级常差 10 倍，共享纵轴会把上传压平） */
function DirectionReading({
  arrow,
  label,
  tone,
  chartColor,
  status,
  bps,
  values,
}: {
  arrow: string;
  label: string;
  tone: "accent" | "info";
  /** UI v4：走势面积图用域色（↓ 青 / ↑ 紫），与主读数文字色解耦 */
  chartColor: string;
  status: NetTotalStatus;
  bps: number;
  values: number[];
}) {
  const { t } = useI18n();
  const { value, unit } = splitSpeed(bps);
  const valueTone = tone === "accent" ? "text-domain-net" : "text-domain-cpu";
  const ready = status !== "warmup";

  return (
    <div>
      <div className="flex items-baseline justify-between">
        <span className="text-[11px] tracking-wide text-ink-dim">
          <span className={tone === "accent" ? "text-accent" : "text-info"}>{arrow}</span> {label}
        </span>
        <span className="text-[10px] text-ink-faint">{t("net.ownScale")}</span>
      </div>
      <div className="mt-1 flex items-baseline gap-1.5">
        {status === "unavailable" ? (
          <span className="font-mono text-[26px] leading-none text-ink-faint">—</span>
        ) : ready ? (
          <>
            <span className={`font-mono text-[26px] leading-none tabular-nums ${valueTone}`}>
              {value}
            </span>
            <span className="text-[11px] text-ink-faint">{unit}</span>
          </>
        ) : (
          <span className="text-[13px] leading-none text-ink-faint">
            {t("common.progress.sampling")}
          </span>
        )}
      </div>
      <div className="mt-2">
        {values.length >= 2 ? (
          <AreaChart
            values={values}
            color={chartColor}
            height={52}
            ariaLabel={t("net.chartAria", { label })}
          />
        ) : (
          <div className="flex h-[52px] items-center justify-center rounded-lg border border-dashed border-hairline-soft text-[10px] text-ink-faint">
            {t("net.accumulating")}
          </div>
        )}
      </div>
      <div className="mt-1 flex items-center justify-between text-[9.5px] text-ink-faint">
        <span>{t("net.last5min")}</span>
        <span>
          {ready && bps === 0
            ? t("net.noTraffic")
            : ready
              ? formatSpeed(bps)
              : "—"}
        </span>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------- 聚合指标

function Aggregates({
  sample,
  avgDown,
  avgUp,
}: {
  sample: NetTotalSample | null;
  avgDown: number;
  avgUp: number;
}) {
  const { t } = useI18n();
  const ready = sample?.status === "ok";
  const today = sample?.today ?? null;
  // note 只承载「必须让用户知道」的原因（连续落库失败才有值）。这里不再去匹配
  // 后端中文串 —— 那是语言耦合，英文环境下会静默失效。
  const missingPersist = (sample?.note ?? "") !== "";

  return (
    <div className="grid grid-cols-2 gap-x-8 gap-y-3 border-b border-hairline-soft py-4">
      <MiniStat
        label={t("net.agg.peak")}
        main={ready ? `↓ ${formatSpeed(sample.peakDownBps)}` : "—"}
        sub={
          ready
            ? `↑ ${formatSpeed(sample.peakUpBps)}${sample.peakAt ? ` · ${formatClock(sample.peakAt)}` : ""}`
            : t("net.agg.noReading")
        }
        title={t("net.agg.peakTitle")}
      />
      <MiniStat
        label={t("net.agg.avg")}
        main={`↓ ${formatSpeed(avgDown)}`}
        sub={`↑ ${formatSpeed(avgUp)} · ${t("net.agg.avgSub")}`}
        title={t("net.agg.avgTitle")}
      />
      <MiniStat
        label={t("net.agg.today")}
        main={today ? `↓ ${formatBytes(today.downBytes)}` : t("net.agg.todayUnavailable")}
        sub={today ? `↑ ${formatBytes(today.upBytes)}` : t("net.agg.todayNote")}
        title={t("net.agg.todayTitle")}
        warn={missingPersist}
        warnText={t("net.agg.notPersisted")}
      />
      <MiniStat
        label={t("net.agg.session")}
        main={sample ? `↓ ${formatBytes(sample.sessionDownBytes)}` : "—"}
        sub={
          sample
            ? `↑ ${formatBytes(sample.sessionUpBytes)} · ${t("net.agg.sessionSub")}`
            : t("net.agg.noReading")
        }
        title={t("net.agg.sessionTitle")}
      />
    </div>
  );
}

function MiniStat({
  label,
  main,
  sub,
  title,
  warn,
  warnText,
}: {
  label: string;
  main: string;
  sub: string;
  title?: string;
  warn?: boolean;
  warnText?: string;
}) {
  return (
    <div title={title}>
      <div className="flex items-center gap-1.5">
        <span className="text-[10px] tracking-wide text-ink-faint">{label}</span>
        {warn && warnText ? <Badge tone="warn">{warnText}</Badge> : null}
      </div>
      <div className="mt-1 font-mono text-[14px] leading-none tabular-nums text-ink">{main}</div>
      <div className="mt-1 text-[10px] leading-snug text-ink-faint">{sub}</div>
    </div>
  );
}

// ---------------------------------------------------------------- 7 日柱

/**
 * 近 7 天柱（纯 SVG 不引图表库）。
 *
 * 缺失日画 **1 px 灰条**而不是 0 柱：0 柱的语义是「那天确实没有流量」，
 * 而 Prune 没运行的日子是「不知道」。底部同时标「已记录 N/7 天」。
 */
function DayBars({ days, count }: { days: DayTraffic[]; count: number }) {
  const { t } = useI18n();
  const labels = useMemo(() => recentDays(count), [count]);
  const map = useMemo(() => new Map(days.map((d) => [d.day, d])), [days]);
  const max = Math.max(
    1,
    ...labels.map((k) => {
      const day = map.get(k);
      return day ? day.downBytes + day.upBytes : 0;
    }),
  );
  const recorded = labels.filter((k) => map.has(k)).length;
  // UI v4（§6.5）：今天（最后一个标签日）的柱发光——「现在进行时」一眼可辨
  const todayKey = labels[labels.length - 1];

  return (
    <div className="mt-2">
      <div className="flex h-16 items-end gap-2">
        {labels.map((k) => {
          const day = map.get(k);
          const pct = day ? ((day.downBytes + day.upBytes) / max) * 100 : 0;
          const upShare =
            day && day.downBytes + day.upBytes > 0
              ? day.upBytes / (day.downBytes + day.upBytes)
              : 0;
          const isToday = k === todayKey && day != null;
          return (
            <div
              key={k}
              className="flex h-full min-w-0 flex-1 flex-col justify-end"
              title={
                day
                  ? `${k}${isToday ? t("net.days.todaySuffix") : ""} · ↓ ${formatBytes(day.downBytes)} / ↑ ${formatBytes(day.upBytes)}`
                  : `${k} · ${t("net.days.noRecord")}`
              }
            >
              {day ? (
                <div
                  className="flex w-full flex-col overflow-hidden rounded-[2px]"
                  style={{
                    height: `${Math.max(4, pct)}%`,
                    ...(isToday
                      ? { boxShadow: "0 0 10px color-mix(in srgb, var(--color-domain-net) 55%, transparent)" }
                      : {}),
                  }}
                >
                  <div className={isToday ? "bg-domain-cpu" : "bg-info/70"} style={{ height: `${upShare * 100}%` }} />
                  <div className={`flex-1 ${isToday ? "bg-domain-net" : "bg-accent/80"}`} />
                </div>
              ) : (
                <div className="h-px w-full bg-white/[0.09]" />
              )}
            </div>
          );
        })}
      </div>
      <div className="mt-1 flex gap-2">
        {labels.map((k) => (
          <span
            key={k}
            className={`min-w-0 flex-1 text-center text-[9px] ${
              k === todayKey ? "font-medium text-domain-net" : map.has(k) ? "text-ink-faint" : "text-ink-faint/50"
            }`}
          >
            {k.slice(5)}
          </span>
        ))}
      </div>
      <div className="mt-1 flex items-center justify-between text-[10px] text-ink-faint">
        <span>
          <span className="text-accent">▮</span> {t("net.down")}{" "}
          <span className="text-info">▮</span> {t("net.up")} · {t("net.days.legendScale")}
        </span>
        <span>
          {t("net.days.recordedLabel")}{" "}
          <span className="font-mono tabular-nums text-ink-dim">{recorded}</span>
          {t("net.days.recordedSuffix", { total: count })}
        </span>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------- 网卡

function maxRate(list: IfaceRate[]): number {
  return Math.max(1, ...list.map((i) => ifaceTotalBps(i)));
}

function IfaceRow({ iface, maxBps }: { iface: IfaceRate; maxBps: number }) {
  const { t } = useI18n();
  return (
    <li className="flex items-center gap-3">
      <span className="w-[64px] shrink-0 truncate font-mono text-[11px] text-ink" title={iface.name}>
        {iface.name}
      </span>
      <span className="w-[86px] shrink-0 font-mono text-[11px] tabular-nums text-accent">
        ↓ {formatSpeed(iface.downBps)}
      </span>
      <span className="w-[86px] shrink-0 font-mono text-[11px] tabular-nums text-info">
        ↑ {formatSpeed(iface.upBps)}
      </span>
      <span className="min-w-0 flex-1">
        <MeterBar
          value={ifaceTotalBps(iface)}
          max={maxBps}
          tone={ifaceTotalBps(iface) > 0 ? "accent" : "neutral"}
          height={4}
          label={t("net.iface.totalAria", {
            name: iface.name,
            speed: formatSpeed(ifaceTotalBps(iface)),
          })}
        />
      </span>
      <span className="w-[74px] shrink-0 text-right font-mono text-[10px] tabular-nums text-ink-faint">
        {formatBytes(iface.sessionDownBytes + iface.sessionUpBytes)}
      </span>
    </li>
  );
}

/** 虚拟接口默认折叠：一排 `awdl0 0 B/s` 会抢走注意力，但信息不能丢 */
function VirtualIfaces({ ifaces, maxBps }: { ifaces: IfaceRate[]; maxBps: number }) {
  const { t, locale } = useI18n();
  const [open, setOpen] = useState(false);
  if (ifaces.length === 0) return null;
  const names = ifaces.map((i) => i.name).join(" · ");

  return (
    <div className="mt-2">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        aria-expanded={open}
        className="flex w-full items-center gap-1.5 rounded-md px-1 py-1 text-left text-[10px] text-ink-faint transition-colors hover:bg-white/[0.04] hover:text-ink-dim"
      >
        <span className={`transition-transform ${open ? "rotate-90" : ""}`}>▸</span>
        <span>
          {t("net.iface.virtualCount", { n: ifaces.length, names })}
          <span className="ml-1.5 text-ink-faint/80">{t("net.iface.virtualNote")}</span>
        </span>
      </button>
      {open ? (
        <ul className="mt-1 space-y-1.5 pl-3">
          {ifaces.map((i) => (
            <li key={i.name} className="flex items-center gap-3">
              <span className="w-[64px] shrink-0 truncate font-mono text-[10.5px] text-ink-dim" title={i.name}>
                {i.name}
              </span>
              <span className="w-[86px] shrink-0 font-mono text-[10.5px] tabular-nums text-ink-dim">
                ↓ {formatSpeed(i.downBps)}
              </span>
              <span className="w-[86px] shrink-0 font-mono text-[10.5px] tabular-nums text-ink-dim">
                ↑ {formatSpeed(i.upBps)}
              </span>
              <span className="min-w-0 flex-1">
                <MeterBar
                  value={ifaceTotalBps(i)}
                  max={maxBps}
                  tone="neutral"
                  height={3}
                  label={t("net.iface.totalAria", {
                    name: i.name,
                    speed: formatSpeed(ifaceTotalBps(i)),
                  })}
                />
              </span>
              <span className="w-[52px] shrink-0 text-right text-[9.5px] text-ink-faint">
                {ifaceKindLabel(i.kind, locale)}
              </span>
            </li>
          ))}
        </ul>
      ) : null}
    </div>
  );
}

// ---------------------------------------------------------------- 按应用

const TOP_APPS = 8;

function AppRanking({
  netStatus,
  onOpenResources,
}: {
  netStatus: NetTotalStatus;
  onOpenResources: () => void;
}) {
  const { t } = useI18n();
  const processes = useMemStore((s) => s.processes);
  const netSample = useNetSpeedStore((s) => s.sample);
  const [expanded, setExpanded] = useState(false);

  const rows = useMemo(() => {
    if (!netSample || netSample.status !== "ok") return [];
    const pidsByFamily = new Map<string, number[]>();
    for (const p of processes) {
      const arr = pidsByFamily.get(p.family);
      if (arr) arr.push(p.pid);
      else pidsByFamily.set(p.family, [p.pid]);
    }
    return [...pidsByFamily.entries()]
      .map(([family, pids]) => ({ family, speed: netSpeedOfPids(netSample, pids) }))
      .filter((r) => r.speed.downBps + r.speed.upBps > 0)
      .sort(
        (a, b) =>
          b.speed.downBps + b.speed.upBps - (a.speed.downBps + a.speed.upBps),
      );
  }, [processes, netSample]);

  const shown = expanded ? rows : rows.slice(0, TOP_APPS);
  const restCount = rows.length - shown.length;
  // 条形基准 = 本拍第一名（§7.6「多实体比大小」）：速率是瞬时值，基准跟着每拍走
  const topBps = shown.length > 0
    ? Math.max(
        ...shown.map((r) => r.speed.downBps + r.speed.upBps),
        1,
      )
    : 1;
  const degraded =
    netSample == null
      ? t("net.apps.degradedWarmup")
      : netSample.status === "unavailable"
        ? t("net.apps.degradedUnavailable", {
            why: netSample.note ?? t("net.apps.nettopMissing"),
          })
        : netStatus === "unavailable"
          ? t("net.apps.degradedOther")
          : null;

  return (
    <section className="mt-4">
      <SectionHead title={t("net.apps.title")} hint={t("net.apps.hint")} />
      {degraded ? (
        <p className="mt-2 text-[11px] text-ink-faint">{degraded}</p>
      ) : rows.length === 0 ? (
        <p className="mt-2 text-[11px] text-ink-faint">{t("net.apps.empty")}</p>
      ) : (
        <>
          <ul className="mt-2 space-y-0.5">
            {shown.map((r) => (
              <li key={r.family}>
                <button
                  type="button"
                  onClick={onOpenResources}
                  title={t("net.apps.detailTitle")}
                  className="flex w-full items-center gap-3 rounded-md px-1 py-1 text-left transition-colors hover:bg-white/[0.04]"
                >
                  <span className="w-[76px] shrink-0 truncate text-[11.5px] text-ink" title={r.family}>
                    {r.family}
                  </span>
                  <span className="min-w-0 flex-1">
                    <MeterBar
                      value={r.speed.downBps + r.speed.upBps}
                      max={topBps}
                      tone="accent"
                      height={4}
                      label={t("net.apps.totalAria", {
                        name: r.family,
                        speed: formatSpeed(r.speed.downBps + r.speed.upBps),
                      })}
                    />
                  </span>
                  <span className="w-[92px] shrink-0 text-right font-mono text-[11px] tabular-nums text-accent">
                    ↓ {formatSpeed(r.speed.downBps)}
                  </span>
                  <span className="w-[92px] shrink-0 text-right font-mono text-[11px] tabular-nums text-info">
                    ↑ {formatSpeed(r.speed.upBps)}
                  </span>
                </button>
              </li>
            ))}
          </ul>
          {restCount > 0 || expanded ? (
            <button
              type="button"
              onClick={() => setExpanded((v) => !v)}
              className="mt-1 text-[10px] text-ink-faint transition-colors hover:text-ink-dim"
            >
              {expanded ? t("net.apps.collapse") : t("net.apps.more", { n: restCount })}
            </button>
          ) : null}
        </>
      )}
    </section>
  );
}

// ---------------------------------------------------------------- 二级页切换

/** 网络 tab 的二级页：监测（速率/流量）与 诊断（分层探测） */
function Segmented({
  page,
  onChange,
}: {
  page: NetSubPage;
  onChange: (p: NetSubPage) => void;
}) {
  const { t } = useI18n();
  const items: { key: NetSubPage; label: string }[] = [
    { key: "monitor", label: t("net.subpage.monitor") },
    { key: "diag", label: t("net.subpage.diag") },
  ];
  return (
    <div
      className="flex shrink-0 rounded-full bg-white/[0.05] p-0.5"
      role="tablist"
      aria-label={t("net.subpage.aria")}
    >
      {items.map((it) => {
        const active = page === it.key;
        return (
          <button
            key={it.key}
            type="button"
            role="tab"
            aria-selected={active}
            onClick={() => onChange(it.key)}
            className={`rounded-full px-3 py-1 text-[11px] transition-colors ${
              active
                ? "bg-white/[0.08] font-medium text-ink"
                : "text-ink-faint hover:text-ink-dim"
            }`}
          >
            {it.label}
          </button>
        );
      })}
    </div>
  );
}

// ---------------------------------------------------------------- 诊断页（net-diag-plan §5，M17 图表化）

/**
 * 诊断二级页：分层横条图（DNS / 网关 / 出口）+ 一条主因结论 + 测速卡。
 *
 * 纪律（§7）：阈值逐行展示（不搞黑盒）；探测失败显示「探测失败」不是 0；
 * 手动触发，running 态禁用重复点击；failed 与「连不上路由器」严格区分。
 */
function DiagPage() {
  const { t, locale } = useI18n();
  const diag = useNetDiagStore((s) => s.diag);
  const running = useNetDiagStore((s) => s.running);
  const copied = useNetDiagStore((s) => s.copied);
  const copyFailed = useNetDiagStore((s) => s.copyFailed);
  const run = useNetDiagStore((s) => s.run);
  const copyReport = useNetDiagStore((s) => s.copyReport);
  const resetCopied = useNetDiagStore((s) => s.resetCopied);

  useEffect(() => {
    if (!copied && !copyFailed) return;
    const timer = setTimeout(resetCopied, 1500);
    return () => clearTimeout(timer);
  }, [copied, copyFailed, resetCopied]);

  const done = diag?.status === "done" && diag;

  return (
    <>
      <div className="mt-4 flex items-center justify-between">
        <span className="text-[10px] text-ink-faint">{t("net.diag.oneRound")}</span>
        <button
          type="button"
          onClick={() => void run()}
          disabled={running}
          className="shrink-0 rounded-md border border-hairline-soft px-2.5 py-1 text-[10.5px] text-ink-dim transition-colors hover:bg-white/[0.05] disabled:opacity-50"
        >
          {running ? t("common.progress.diagnosing") : t("net.diag.start")}
        </button>
      </div>

      {!diag || diag.status === "idle" ? (
        <p className="mt-3 text-[11px] leading-relaxed text-ink-faint">{t("net.diag.idle")}</p>
      ) : diag.status === "failed" ? (
        <p className="mt-3 rounded-lg bg-warn/[0.08] px-3 py-2 text-[11px] leading-relaxed text-warn">
          {t("net.diag.failed", { why: diag.note ?? t("net.diag.unknownWhy") })}
        </p>
      ) : running ? (
        <p className="mt-3 text-[11px] text-ink-faint">{t("net.diag.running")}</p>
      ) : done ? (
        <>
          {/* UI v4（§6.5）诊断轨道：DNS → 网关 → 出口逐段点亮，先看「断在哪」再看细节 */}
          <section className="mt-3 rounded-xl border border-hairline-soft bg-white/[0.02] px-4 py-3.5">
            <TrackProgress ariaLabel={t("net.diag.trackAria")} nodes={diagTrackNodes(done, locale)} />
          </section>
          <section className="mt-4 space-y-5 border-b border-hairline-soft pb-4">
            <LayerChart
              name="DNS"
              sub={t("net.diag.dnsSub")}
              probe={done.dns}
              threshold={DIAG_THRESHOLDS.dnsMs}
            />
            <LayerChart
              name={
                done.gateway
                  ? t("net.diag.routerWith", { gateway: done.gateway })
                  : t("net.diag.router")
              }
              sub={t("net.diag.lanSub")}
              probe={done.lan}
              threshold={DIAG_THRESHOLDS.lanMs}
            />
            <LayerChart
              name={t("net.diag.wanName")}
              sub={
                done.jitterMs != null
                  ? t("net.diag.wanSubJitter", {
                      jitter: done.jitterMs.toFixed(1),
                      threshold: DIAG_THRESHOLDS.jitterMs,
                    })
                  : t("net.diag.wanSub")
              }
              probe={done.wan}
              threshold={DIAG_THRESHOLDS.wanLanGapMs}
            />
          </section>

          {done.captive === "suspected" ? (
            <p className="mt-3 text-[11px] text-warn">{t("net.diag.captive")}</p>
          ) : null}

          {done.verdict ? (
            <div className="mt-3 rounded-lg bg-white/[0.04] px-3 py-2.5">
              <div className="text-[12px] font-medium text-ink">
                {t("net.diag.verdict", { label: done.verdict.label })}
              </div>
              <div className="mt-0.5 text-[10.5px] leading-relaxed text-ink-faint">
                {t("net.diag.verdictRule", {
                  advice: done.verdict.advice,
                  rule: done.verdict.rule,
                })}
              </div>
            </div>
          ) : null}

          <p className="mt-2 text-[10px] text-ink-faint">{t("net.diag.scopeNote")}</p>

          <button
            type="button"
            onClick={() => void copyReport()}
            className={`mt-2 text-[10px] transition-colors ${
              copyFailed
                ? "text-danger"
                : copied
                  ? "text-accent"
                  : "text-ink-faint hover:text-ink-dim"
            }`}
            title={t("net.diag.copyTitle")}
          >
            {copyFailed
              ? `✗ ${t("common.action.copyFailed")}`
              : copied
                ? `✓ ${t("common.action.copied")}`
                : t("net.diag.copy")}
          </button>
        </>
      ) : null}

      <SpeedCard />
    </>
  );
}

/**
 * 诊断轨道节点：median null = 探测失败（红叉）；超阈值 = 该层是瓶颈（红叉 + 读数）；
 * 正常 = 绿勾 + 中位数。sub 一律给读数，让轨道自己说话。
 */
function diagTrackNodes(
  done: {
    dns: LayerProbe | null;
    lan: LayerProbe | null;
    wan: LayerProbe | null;
  },
  locale: Locale,
): TrackNode[] {
  const { t } = i18nFor(locale);
  const node = (label: string, probe: LayerProbe | null, threshold: number): TrackNode => {
    const median = probe?.medianMs ?? null;
    if (median == null) {
      return { label, sub: probe?.note ?? t("net.diag.probeFailed"), state: "fail" };
    }
    return {
      label,
      sub: `${median.toFixed(0)} ms`,
      state: median > threshold ? "fail" : "ok",
    };
  };
  return [
    node("DNS", done.dns, DIAG_THRESHOLDS.dnsMs),
    node(t("net.diag.router"), done.lan, DIAG_THRESHOLDS.lanMs),
    node(t("net.diag.wanName"), done.wan, DIAG_THRESHOLDS.wanLanGapMs),
  ];
}

/**
 * 分层横条图：填充条 = 中位数，竖刻度 = 判定阈值，圆点 = 原始样本。
 *
 * 横轴从 0 到 `max(阈值×1.6, 读数×1.08)`：阈值始终可见，读数超阈值时自然越线。
 * 探测失败（median null）不画柱——0 柱会被读成「很快」，违背「不显示 0」纪律。
 */
function LayerChart({
  name,
  sub,
  probe,
  threshold,
}: {
  name: string;
  sub: string;
  probe: LayerProbe | null;
  threshold: number;
}) {
  const { t } = useI18n();
  const tone = probeTone(probe, threshold);
  const fill = tone === "danger" ? "bg-danger" : tone === "warn" ? "bg-warn" : "bg-accent";
  const text = tone === "danger" ? "text-danger" : tone === "warn" ? "text-warn" : "text-ink";
  const median = probe?.medianMs ?? null;
  const samples = probe?.samples ?? [];
  const scaleMax = Math.max(threshold * 1.6, median ?? 0, ...samples, 1) * 1.08;
  const pct = (v: number) => Math.min(100, (v / scaleMax) * 100);

  return (
    <div>
      <div className="flex items-baseline justify-between gap-3">
        <span className="min-w-0 text-[11.5px] text-ink">
          {name}
          <span className="ml-1.5 text-[9.5px] text-ink-faint">{sub}</span>
        </span>
        <span className={`shrink-0 font-mono text-[12px] tabular-nums ${text}`}>
          {median != null
            ? `${median.toFixed(1)} ms`
            : (probe?.note ?? t("net.diag.probeFailed"))}
        </span>
      </div>

      {median == null ? (
        <p className="mt-1.5 text-[10px] text-danger/80">{t("net.layer.noReading")}</p>
      ) : (
        <>
          <div className="relative mt-2 h-2 rounded-full bg-white/[0.05]">
            <div
              className={`absolute inset-y-0 left-0 rounded-full transition-[width] duration-500 ${fill}`}
              style={{ width: `${pct(median)}%` }}
            />
            <div
              className="absolute -inset-y-1 w-px bg-ink-faint/70"
              style={{ left: `${pct(threshold)}%` }}
              title={t("net.layer.thresholdTick", { n: threshold })}
            />
          </div>
          {samples.length > 1 ? (
            <div className="relative mt-1 h-2">
              {samples.map((s, i) => (
                <span
                  key={i}
                  className="absolute top-1/2 size-[5px] -translate-x-1/2 -translate-y-1/2 rounded-full bg-ink-faint/60"
                  style={{ left: `${pct(s)}%` }}
                  title={t("net.layer.sample", { index: i + 1, n: s.toFixed(1) })}
                />
              ))}
            </div>
          ) : null}
          <div className="mt-0.5 flex items-baseline justify-between text-[9.5px] text-ink-faint">
            <span>
              {t("net.layer.samplesMedian", {
                count: samples.length,
                median: median.toFixed(1),
              })}
            </span>
            <span>{t("net.layer.thresholdFoot", { n: threshold })}</span>
          </div>
          {probe?.note ? (
            <p className="mt-1 text-[9.5px] leading-relaxed text-warn">{probe.note}</p>
          ) : null}
        </>
      )}
    </div>
  );
}

// ---------------------------------------------------------------- 测速（macOS only，capability 门控）

/**
 * 测速卡：系统 networkQuality + RPM（bufferbloat 官方口径），图表化为对照条。
 * Windows 无系统级等价物 → capability 为 false 时**整卡不渲染**（W2 先例：
 * 隐藏优于「不可用」占位），不是显示一个灰按钮。
 */
function SpeedCard() {
  const { t, locale } = useI18n();
  const capability = useNetDiagStore((s) => s.capability);
  const speed = useNetDiagStore((s) => s.speed);
  const speedRunning = useNetDiagStore((s) => s.speedRunning);
  const runSpeed = useNetDiagStore((s) => s.runSpeed);

  if (!capability?.speedTest) return null;

  /** 归一化参考：1 Gbps（§7.6）。万兆 / 2.5G 网卡会顶满——数字不会撒谎，
   *  满条只说明「不低于参考值」，caption 里写明。 */
  const GIGABIT_BPS = (1e9 * (1 - 0.024)) / 8; // 扣 TCP/IP 头开销的实际可用带宽

  return (
    <section className="mt-5 border-t border-hairline-soft pt-4">
      <div className="flex items-center justify-between">
        <SectionHead title={t("net.speed.title")} hint={t("net.speed.hint")} />
        <button
          type="button"
          onClick={() => void runSpeed()}
          disabled={speedRunning}
          className="shrink-0 rounded-md border border-hairline-soft px-2.5 py-1 text-[10.5px] text-ink-dim transition-colors hover:bg-white/[0.05] disabled:opacity-50"
        >
          {speedRunning ? t("common.progress.speedTesting") : t("net.speed.start")}
        </button>
      </div>

      {speedRunning ? (
        <p className="mt-2 text-[11px] text-ink-faint">{t("net.speed.runningNote")}</p>
      ) : speed ? (
        <div className="mt-2.5 space-y-2.5">
          <div>
            <div className="flex items-baseline justify-between gap-2">
              <span className="text-[11px] text-ink-dim">{t("net.down")}</span>
              <span className="font-mono text-xs tabular-nums text-ink">
                {formatMbps(speed.downBps)}
              </span>
            </div>
            <MeterBar
              className="mt-1.5"
              value={speed.downBps}
              max={GIGABIT_BPS}
              tone="accent"
              height={5}
              label={t("net.speed.barAriaDown", { speed: formatMbps(speed.downBps) })}
            />
          </div>
          <div>
            <div className="flex items-baseline justify-between gap-2">
              <span className="text-[11px] text-ink-dim">{t("net.up")}</span>
              <span className="font-mono text-xs tabular-nums text-ink">
                {formatMbps(speed.upBps)}
              </span>
            </div>
            <MeterBar
              className="mt-1.5"
              value={speed.upBps}
              max={GIGABIT_BPS}
              tone="info"
              height={5}
              label={t("net.speed.barAriaUp", { speed: formatMbps(speed.upBps) })}
            />
          </div>
          <p className="text-[10px] text-ink-faint">{t("net.speed.gigabit")}</p>
          <div className="flex items-baseline justify-between gap-3">
            <span className="text-[11px] text-ink-dim">
              {t("net.speed.rpm")}
              <span className="ml-1.5 text-[9.5px] text-ink-faint">
                {t("net.speed.baseRtt", { n: speed.baseRttMs.toFixed(0) })}
              </span>
            </span>
            <span className="flex shrink-0 items-center gap-1.5">
              <span className="font-mono text-[13px] tabular-nums text-ink">
                {speed.rpm.toFixed(0)}
              </span>
              <Badge tone={rpmTone(speed.rpmLevel)}>
                {rpmLevelLabel(speed.rpmLevel, locale)}
              </Badge>
            </span>
          </div>
          <p
            className="text-[10px] leading-relaxed text-ink-faint"
            title={t("net.speed.rpmTitle")}
          >
            {t("net.speed.rpmNote")}
          </p>
        </div>
      ) : (
        <p className="mt-2 text-[11px] text-ink-faint">{t("net.speed.idle")}</p>
      )}
    </section>
  );
}

/** Apple RPM 分级 → 徽标语义色（只做文案色映射，不发明新阈值） */
function rpmTone(level: string): "danger" | "warn" | "accent" | "neutral" {
  switch (level) {
    case "Poor":
      return "danger";
    case "Low":
      return "warn";
    case "Medium":
    case "Good":
      return "accent";
    default:
      return "neutral";
  }
}

// ---------------------------------------------------------------- 小工具

function SectionHead({ title, hint }: { title: string; hint: string }) {
  return (
    <div className="flex items-baseline justify-between">
      <span className="text-[11px] font-medium tracking-wide text-ink-dim">{title}</span>
      <span className="text-[10px] text-ink-faint">{hint}</span>
    </div>
  );
}

/** 速率拆成「数值 + 单位」：主读数要让数字自己占一行，单位小一号 */
function splitSpeed(bps: number): { value: string; unit: string } {
  if (!Number.isFinite(bps) || bps < 1) return { value: "0", unit: "B/s" };
  const units = ["B/s", "KB/s", "MB/s", "GB/s"];
  let v = bps;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i += 1;
  }
  return { value: v >= 100 ? String(Math.round(v)) : v.toFixed(1), unit: units[i] };
}
