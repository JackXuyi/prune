/**
 * D5：应用详情「历史走势」区（三轨 + 区间切换 + 覆盖率）。
 *
 * 口径纪律（v1.1 §5.5）：
 * - 历史内存是 **RSS**，当前读数是 footprint——脚注里写明，两套不相减；
 * - 缺失小时/天画灰虚线，不当 0；
 * - 覆盖率 < 60% 角标转 warn 并说明原因（Prune 未运行 / 后台采样关闭）；
 * - M25：30 天视图直接消费后端天粒度聚合（只回有记录的天），小时视图照旧前端聚合。
 */
import { useMemo, useState } from "react";

import { BarSeries, type BarDatum } from "../charts/BarSeries";
import { EmptyState } from "../EmptyArt";
import { useI18n } from "../../i18n/store";
import type { NoParamKey } from "../../i18n/types";
import { formatBytes } from "../../lib/format";
import { aggregateDays, lastDays, lastHours } from "../../lib/useAppHistory";
import type { DailyBucket, HistoryBucket } from "../../types/appHistory";

type Range = "24h" | "7d" | "30d";

const CPU_COLOR = "var(--color-domain-cpu)";
const MEM_COLOR = "var(--color-info)";
const NET_COLOR = "var(--color-domain-net)";

const RANGE_LABEL: Record<Range, NoParamKey> = {
  "24h": "resources.history.range24h",
  "7d": "resources.history.range7d",
  "30d": "resources.history.range30d",
};

export function HistoryTrack({
  rows,
  daily,
  loading,
  error,
}: {
  rows: HistoryBucket[];
  /** M25：天粒度聚合行（只含有记录的天），30 天视图直接消费 */
  daily: DailyBucket[];
  loading: boolean;
  error: string | null;
}) {
  const [range, setRange] = useState<Range>("7d");
  const { t } = useI18n();

  const view = useMemo(() => buildView(rows, daily, range), [rows, daily, range]);

  if (!loading && rows.length === 0 && daily.length === 0) {
    return (
      <section className="rounded-xl border border-hairline-soft bg-chip-soft py-4">
        <EmptyState
          title={t("resources.history.emptyTitle")}
          note={error ? t("resources.history.emptyReadError", { err: error }) : t("resources.history.emptyNote")}
        />
      </section>
    );
  }

  return (
    <section className="rounded-xl border border-hairline-soft bg-chip-soft px-3 py-2.5">
      <div className="flex items-center gap-2 text-[10px] text-ink-dim">
        <span className="text-[11px] font-medium text-ink">{t("resources.history.title")}</span>
        <div className="grow" />
        {(["30d", "7d", "24h"] as const).map((r) => (
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
            {t(RANGE_LABEL[r])}
          </button>
        ))}
        <span
          className={`font-mono text-[9px] ${
            view.coverage != null && view.coverage < 0.6 ? "text-warn" : "text-accent"
          }`}
          title={
            view.coverage != null && view.coverage < 0.6
              ? t("resources.history.coverageTipWarn")
              : t("resources.history.coverageTip")
          }
        >
          {view.coverage == null
            ? t("resources.history.coverageNone")
            : t("resources.history.coverage", { pct: Math.round(view.coverage * 100) })}
        </span>
      </div>

      <Track
        label={t("resources.history.cpu")}
        bars={view.cpu}
        color={CPU_COLOR}
        right={
          view.cpuPeak == null
            ? t("resources.history.cpuSummaryEmpty")
            : t("resources.history.cpuSummary", {
                peak: `${Math.round(view.cpuPeak)}%`,
                avg: `${Math.round(view.cpuAvg ?? 0)}%`,
              })
        }
        format={(v) => `${Math.round(v)}%`}
      />
      <Track
        label={t("resources.history.mem")}
        bars={view.mem}
        color={MEM_COLOR}
        right={
          view.memPeak > 0
            ? t("resources.history.memSummary", {
                peak: formatBytes(view.memPeak),
                today: view.todayRss != null ? formatBytes(view.todayRss) : "—",
              })
            : t("resources.history.memSummaryEmpty")
        }
        format={(v) => formatBytes(v)}
      />
      <Track
        label={t("resources.history.net")}
        bars={view.net}
        color={NET_COLOR}
        right={t("resources.history.netSummary", {
          down: formatBytes(view.netDown),
          up: formatBytes(view.netUp),
        })}
        format={(v) => formatBytes(v)}
      />

      <p className="mt-1.5 text-[9px] leading-snug text-ink-faint">
        {t("resources.history.footnote")}
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
  const { t } = useI18n();
  return (
    <div className="mt-2 grid grid-cols-[30px_minmax(0,1fr)_132px] items-end gap-2">
      <span className="pb-[2px] text-[9px] text-ink-faint">{label}</span>
      <BarSeries
        bars={bars}
        color={color}
        height={28}
        ariaLabel={t("resources.history.trackAria", {
          label,
          n: bars.filter((b) => b.hasData).length,
          total: bars.length,
        })}
        formatValue={format}
      />
      <span className="pb-[1px] text-right font-mono text-[9px] tabular-nums text-ink-faint">
        {right}
      </span>
    </div>
  );
}

function buildView(rows: HistoryBucket[], daily: DailyBucket[], range: Range) {
  const now = new Date();
  const byBucket = new Map(rows.map((r) => [r.bucket, r]));
  const byDay = new Map(daily.map((r) => [r.day, r]));

  // 30 天视图用后端天桶；24h 用小时桶；7d 用小时桶前端聚合成天
  const bars =
    range === "30d"
      ? monthView(byDay, now)
      : range === "24h"
        ? hoursView(byBucket, now)
        : daysView(rows, now);

  // 覆盖率：30d 按「有记录的天 / 应有天数」，其余按「有样本的小时 / 应有小时」
  const inRange = range === "30d" ? 30 : range === "24h" ? 24 : 7 * 24;
  const covered =
    range === "30d"
      ? daily.filter((r) => lastDays(30, now).includes(r.day)).length
      : (() => {
          const windowKeys = new Set(
            range === "24h" ? lastHours(24, now) : lastDays(7, now).flatMap(dayHours),
          );
          return rows.filter((r) => r.samples > 0 && windowKeys.has(r.bucket)).length;
        })();
  const coverage = inRange > 0 && covered > 0 ? Math.min(1, covered / inRange) : null;

  const windowDayKeys = new Set(lastDays(range === "30d" ? 30 : 7, now));
  const inWindowDaily = daily.filter((r) => windowDayKeys.has(r.day));
  const cpuPeaks = inWindowDaily
    .map((r) => r.cpuPeak)
    .filter((v): v is number => v != null);
  const cpuAvgVals = inWindowDaily.map((r) => r.cpuAvg).filter((v): v is number => v != null);
  const cpuPeak = cpuPeaks.length ? Math.max(...cpuPeaks) : null;
  const cpuAvg = cpuAvgVals.length
    ? cpuAvgVals.reduce((a, b) => a + b, 0) / cpuAvgVals.length
    : null;

  const memPeak = inWindowDaily.reduce((m, r) => Math.max(m, r.rssPeak ?? 0), 0);
  const today = dayKeyOf(now);
  const todayRow = byDay.get(today);

  const netDown = inWindowDaily.reduce((s, r) => s + r.netDown, 0);
  const netUp = inWindowDaily.reduce((s, r) => s + r.netUp, 0);

  // 只回原始数字：文案在组件层按 locale 拼（同一条轨道的中英文句子结构不同）
  return {
    ...bars,
    coverage,
    cpuPeak,
    cpuAvg,
    memPeak,
    todayRss: todayRow?.rssAvg ?? null,
    netDown,
    netUp,
  };
}

function dayKeyOf(d: Date): string {
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
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

/** M25：30 天视图 —— 直接消费后端天粒度聚合（每根柱 = 一天，只有有记录的天有实心柱） */
function monthView(byDay: Map<string, DailyBucket>, now: Date) {
  const days = lastDays(30, now);
  const cpu: BarDatum[] = days.map((d) => {
    const r = byDay.get(d);
    return {
      label: kLabel(d, false),
      value: r?.cpuPeak ?? 0,
      hasData: (r?.samples ?? 0) > 0 && r?.cpuPeak != null,
    };
  });
  const mem: BarDatum[] = days.map((d) => {
    const r = byDay.get(d);
    return {
      label: kLabel(d, false),
      value: r?.rssPeak ?? 0,
      hasData: (r?.samples ?? 0) > 0 && r?.rssPeak != null,
    };
  });
  const net: BarDatum[] = days.map((d) => {
    const r = byDay.get(d);
    return {
      label: kLabel(d, false),
      value: (r?.netDown ?? 0) + (r?.netUp ?? 0),
      hasData: (r?.samples ?? 0) > 0,
    };
  });
  return { cpu, mem, net, expectedHours: 30 * 24 };
}

function kLabel(key: string, hourly: boolean): string {
  const day = key.slice(5, 10);
  return hourly ? `${day} ${key.slice(11, 13)}:00` : day;
}
