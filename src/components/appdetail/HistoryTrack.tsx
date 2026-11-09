/**
 * D5：应用详情「近 7 天」历史区（三轨 + 区间切换 + 覆盖率）。
 *
 * 口径纪律（v1.1 §5.5）：
 * - 历史内存是 **RSS**，当前读数是 footprint——脚注里写明，两套不相减；
 * - 缺失小时/天画灰虚线，不当 0；
 * - 覆盖率 < 60% 角标转 warn 并说明原因（Prune 未运行 / 后台采样关闭）。
 */
import { useMemo, useState } from "react";

import { BarSeries, type BarDatum } from "../charts/BarSeries";
import { EmptyState } from "../EmptyArt";
import { formatBytes } from "../../lib/format";
import { aggregateDays, lastDays, lastHours } from "../../lib/useAppHistory";
import type { HistoryBucket } from "../../types/appHistory";

type Range = "24h" | "7d";

const CPU_COLOR = "var(--color-domain-cpu)";
const MEM_COLOR = "var(--color-info)";
const NET_COLOR = "var(--color-domain-net)";

export function HistoryTrack({
  rows,
  loading,
  error,
}: {
  rows: HistoryBucket[];
  loading: boolean;
  error: string | null;
}) {
  const [range, setRange] = useState<Range>("7d");

  const view = useMemo(() => buildView(rows, range), [rows, range]);

  if (!loading && rows.length === 0) {
    return (
      <section className="rounded-xl border border-hairline-soft bg-white/[0.02] py-4">
        <EmptyState
          title="还没有历史"
          note={
            error
              ? `读取失败：${error}`
              : "Prune 运行满一小时后，这里会出现 CPU / 内存 / 流量的近 7 天走势"
          }
        />
      </section>
    );
  }

  return (
    <section className="rounded-xl border border-hairline-soft bg-white/[0.02] px-3 py-2.5">
      <div className="flex items-center gap-2 text-[10px] text-ink-dim">
        <span className="text-[11px] font-medium text-ink">近 7 天</span>
        <div className="grow" />
        {(["7d", "24h"] as const).map((r) => (
          <button
            key={r}
            type="button"
            onClick={() => setRange(r)}
            className={`rounded-full border px-2 py-[2px] text-[9px] transition-colors ${
              range === r
                ? "border-[color-mix(in_srgb,var(--color-domain-cpu)_35%,transparent)] bg-[color-mix(in_srgb,var(--color-domain-cpu)_10%,transparent)] text-domain-cpu"
                : "border-hairline text-ink-faint hover:text-ink-dim"
            }`}
            aria-pressed={range === r}
          >
            {r === "7d" ? "7 天" : "24 h"}
          </button>
        ))}
        <span
          className={`font-mono text-[9px] ${
            view.coverage != null && view.coverage < 0.6 ? "text-warn" : "text-accent"
          }`}
          title={
            view.coverage != null && view.coverage < 0.6
              ? "记录不完整：Prune 只在这段时间的一部分里运行，或后台流量采样被关闭"
              : "覆盖 = 有样本的小时数 / 该区间应有小时数"
          }
        >
          {view.coverage == null ? "覆盖 —" : `覆盖 ${Math.round(view.coverage * 100)}%`}
        </span>
      </div>

      <Track
        label="CPU"
        bars={view.cpu}
        color={CPU_COLOR}
        right={view.cpuSummary}
        format={(v) => `${Math.round(v)}%`}
      />
      <Track
        label="内存"
        bars={view.mem}
        color={MEM_COLOR}
        right={view.memSummary}
        format={(v) => formatBytes(v)}
      />
      <Track
        label="流量"
        bars={view.net}
        color={NET_COLOR}
        right={view.netSummary}
        format={(v) => formatBytes(v)}
      />

      <p className="mt-1.5 text-[9px] leading-snug text-ink-faint">
        历史为 <span className="text-ink-dim">RSS</span> 口径（每 10 s 采样、按小时聚合），
        与上方 footprint 当前值不是同一口径，不能相减；灰虚线 = 该时段没有记录。
      </p>
    </section>
  );
}

function Track({
  label,
  bars,
  color,
  right,
  format,
}: {
  label: string;
  bars: BarDatum[];
  color: string;
  right: string;
  format: (v: number) => string;
}) {
  return (
    <div className="mt-2 grid grid-cols-[30px_minmax(0,1fr)_132px] items-end gap-2">
      <span className="pb-[2px] text-[9px] text-ink-faint">{label}</span>
      <BarSeries
        bars={bars}
        color={color}
        height={28}
        ariaLabel={`${label} 历史：${bars.filter((b) => b.hasData).length}/${bars.length} 个时段有记录`}
        formatValue={format}
      />
      <span className="pb-[1px] text-right font-mono text-[9px] tabular-nums text-ink-faint">
        {right}
      </span>
    </div>
  );
}

function buildView(rows: HistoryBucket[], range: Range) {
  const now = new Date();
  const byBucket = new Map(rows.map((r) => [r.bucket, r]));

  const bars: { cpu: BarDatum[]; mem: BarDatum[]; net: BarDatum[]; expectedHours: number } =
    range === "24h"
      ? hoursView(byBucket, now)
      : daysView(rows, now);

  const inRange = range === "24h" ? bars.expectedHours : 7 * 24;
  const withSamples = rows.filter((r) => r.samples > 0).length;
  // 只统计窗口内的小时：窗口外（8 天里多出的那天）不算进覆盖率分母
  const windowKeys = new Set(
    range === "24h" ? lastHours(24, now) : lastDays(7, now).flatMap(dayHours),
  );
  const covered = rows.filter((r) => r.samples > 0 && windowKeys.has(r.bucket)).length;
  const coverage = inRange > 0 ? Math.min(1, covered / inRange) : null;

  const cpuPeaks = rows
    .filter((r) => windowKeys.has(r.bucket) && r.cpuPeak != null)
    .map((r) => r.cpuPeak as number);
  const cpuAvgVals = rows
    .filter((r) => windowKeys.has(r.bucket) && r.cpuAvg != null)
    .map((r) => r.cpuAvg as number);
  const cpuPeak = cpuPeaks.length ? Math.max(...cpuPeaks) : null;
  const cpuAvg = cpuAvgVals.length
    ? cpuAvgVals.reduce((a, b) => a + b, 0) / cpuAvgVals.length
    : null;

  const memPeak = rows
    .filter((r) => windowKeys.has(r.bucket) && r.rssPeak != null)
    .reduce((m, r) => Math.max(m, r.rssPeak as number), 0);
  const today = lastHours(1, now)[0];
  const todayRow = byBucket.get(today);

  const netDown = rows
    .filter((r) => windowKeys.has(r.bucket))
    .reduce((s, r) => s + r.netDown, 0);
  const netUp = rows
    .filter((r) => windowKeys.has(r.bucket))
    .reduce((s, r) => s + r.netUp, 0);

  return {
    ...bars,
    coverage: withSamples === 0 ? null : coverage,
    cpuSummary:
      cpuPeak == null
        ? "峰值 — · 均值 —"
        : `峰值 ${Math.round(cpuPeak)}% · 均值 ${Math.round(cpuAvg ?? 0)}%`,
    memSummary:
      memPeak > 0
        ? `峰值 ${formatBytes(memPeak)} · 今日 ${todayRow?.rssAvg != null ? formatBytes(todayRow.rssAvg) : "—"}`
        : "峰值 — · 今日 —",
    netSummary: `↓ ${formatBytes(netDown)} · ↑ ${formatBytes(netUp)}`,
  };
}

function dayHours(day: string): string[] {
  return Array.from({ length: 24 }, (_, h) => `${day}T${String(h).padStart(2, "0")}`);
}

function hoursView(byBucket: Map<string, HistoryBucket>, now: Date) {
  const keys = lastHours(24, now);
  const bars = keys.map((k) => byBucket.get(k));
  const cpu: BarDatum[] = bars.map((r, i) => ({
    label: kLabel(keys[i], true),
    value: r?.cpuPeak ?? 0,
    hasData: (r?.samples ?? 0) > 0 && r?.cpuPeak != null,
  }));
  const mem: BarDatum[] = bars.map((r, i) => ({
    label: kLabel(keys[i], true),
    value: r?.rssPeak ?? 0,
    hasData: (r?.samples ?? 0) > 0 && r?.rssPeak != null,
  }));
  const net: BarDatum[] = bars.map((r, i) => ({
    label: kLabel(keys[i], true),
    value: (r?.netDown ?? 0) + (r?.netUp ?? 0),
    hasData: (r?.samples ?? 0) > 0,
  }));
  return { cpu, mem, net, expectedHours: 24 };
}

function daysView(rows: HistoryBucket[], now: Date) {
  const days = lastDays(7, now);
  const agg = aggregateDays(rows);
  const cpu: BarDatum[] = days.map((d) => {
    const a = agg.get(d);
    return {
      label: kLabel(d, false),
      value: a?.cpuPeak ?? 0,
      hasData: (a?.samples ?? 0) > 0,
    };
  });
  const mem: BarDatum[] = days.map((d) => {
    const a = agg.get(d);
    return { label: kLabel(d, false), value: a?.rssPeak ?? 0, hasData: (a?.samples ?? 0) > 0 };
  });
  const net: BarDatum[] = days.map((d) => {
    const a = agg.get(d);
    return {
      label: kLabel(d, false),
      value: (a?.netDown ?? 0) + (a?.netUp ?? 0),
      hasData: (a?.samples ?? 0) > 0,
    };
  });
  return { cpu, mem, net, expectedHours: 7 * 24 };
}

function kLabel(key: string, hourly: boolean): string {
  const day = key.slice(5, 10);
  return hourly ? `${day} ${key.slice(11, 13)}:00` : day;
}
