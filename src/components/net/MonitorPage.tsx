import { useEffect, useMemo, useState } from "react";

import type { NetTotalSample, NetTotalStatus } from "../../types/net";
import { formatBytes, formatClock } from "../../lib/format";
import { useI18n } from "../../i18n/store";
import { isTauri, pubIpGet } from "../../lib/ipc";
import { useNetTotalStore, TRAFFIC_DAYS } from "../../stores/netTotalStore";
import { formatSpeed, splitIfaces } from "../../types/net";
import { AreaChart } from "../charts/AreaChart";
import { Badge } from "../primitives";
import { DayBars, IfaceList, AppRanking } from "./MonitorCharts";
import { SectionHead } from "./bits";

/**
 * 网络监测二级页（M14）：本机下载 / 上传速率、走势、网卡分布、今日流量、按应用排名。
 * 自测试页重设计起从 NetBoard 抽出——页面本体不变，纯搬家。
 *
 * 三条纪律（方案 §3 / §7）：
 * - 主数字只算**物理网卡**：开 VPN 时 `utun3` 与 `en0` 是同一批字节，相加等于双计
 * - `warmup` 显示「采样中…」、`unavailable` 显示「不可读 + 原因」，**都不显示 0**
 * - 「今日」是 **Prune 运行期间累计**：系统计数器只给「开机至今」，未运行时段无法回填
 *
 * 组件内不订阅、不采样：数据全部来自 store，保持「谁可见谁负责」的既有分工。
 */
export function MonitorPage({ onOpenResources }: { onOpenResources: () => void }) {
  const { t } = useI18n();
  const sample = useNetTotalStore((s) => s.sample);
  const history = useNetTotalStore((s) => s.history);
  const days = useNetTotalStore((s) => s.days);
  const refreshNet = useNetTotalStore((s) => s.refresh);

  const status: NetTotalStatus = sample?.status ?? "warmup";
  const { physical, virtual } = splitIfaces(sample?.ifaces ?? []);
  const avg = useMemo(() => weightedAverage(history), [history]);
  const downSeries = useMemo(() => history.map((p) => p.downBps), [history]);
  const upSeries = useMemo(() => history.map((p) => p.upBps), [history]);

  return (
    <>
      {/* 速率不可读横幅（spec §4.5）：橙色软底 + 底线 + 重试。重试走 store.refresh，
          不新开命令；连着点只是多读一次系统计数器，无害。 */}
      {status === "unavailable" ? (
        <div className="mt-3 flex h-[30px] items-center justify-between gap-3 rounded-md border-b border-warn/25 bg-warn/[0.1] px-3">
          <span
            className="min-w-0 truncate text-[10.5px] text-warn"
            title={t("net.unavailable", { why: sample?.note ?? t("net.unavailableFallback") })}
          >
            {t("net.unavailable", { why: sample?.note ?? t("net.unavailableFallback") })}
          </span>
          <button
            type="button"
            onClick={() => void refreshNet()}
            className="shrink-0 text-[10.5px] text-warn transition-opacity hover:opacity-75"
          >
            {t("common.action.retry")}
          </button>
        </div>
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

      <PublicIp />

      <section className="mt-4 border-b border-hairline-soft pb-4">
        <SectionHead title={t("net.days.title")} hint={t("net.days.hint")} />
        <DayBars days={days} count={TRAFFIC_DAYS} />
      </section>

      <section className="mt-4 border-b border-hairline-soft pb-4">
        <SectionHead title={t("net.iface.title")} hint={t("net.iface.hint")} />
        <IfaceList status={status} physical={physical} virtual={virtual} />
      </section>

      <AppRanking netStatus={status} onOpenResources={onOpenResources} />

      <p className="mt-3 text-[10px] leading-relaxed text-ink-faint">{t("net.footnote")}</p>
    </>
  );
}

/** 时间加权平均（net.ts 同义实现，监测页自己用） */
function weightedAverage(points: { at: number; downBps: number; upBps: number }[]): {
  downBps: number;
  upBps: number;
} {
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
          {ready && bps === 0 ? t("net.noTraffic") : ready ? formatSpeed(bps) : "—"}
        </span>
      </div>
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

// ---------------------------------------------------------------- 公网 IP（f26 G-08）

/**
 * 公网 IP 读数行：进监测页拉一次（后端 10 min 缓存），失败显「不可用 + 重试」。
 * 隐私口径（方案 D5）：只发一个无参数 GET，不携带用户数据；设置页可关。
 */
function PublicIp() {
  const { t } = useI18n();
  const [ip, setIp] = useState<{ ip: string; source: string } | null>(null);
  const [failed, setFailed] = useState(false);

  const load = (force: boolean) => {
    if (!isTauri()) return;
    void pubIpGet(force)
      .then((r) => {
        setIp(r);
        setFailed(r == null);
      })
      .catch(() => setFailed(true));
  };

  useEffect(() => {
    load(false);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return (
    <div className="flex items-baseline justify-between border-b border-hairline-soft py-3">
      <span
        className="text-[10px] tracking-wide text-ink-faint"
        title={ip ? t("net.pubip.title", { source: ip.source }) : undefined}
      >
        {t("net.pubip.label")}
      </span>
      {ip ? (
        <button
          type="button"
          onClick={() => load(true)}
          title={t("net.pubip.refreshTitle")}
          className="font-mono text-[13px] tabular-nums text-ink transition-opacity hover:opacity-70"
        >
          {ip.ip}
        </button>
      ) : (
        <span className="flex items-center gap-2 text-[11px] text-ink-faint">
          {failed ? t("net.pubip.unavailable") : t("common.progress.sampling")}
          <button
            type="button"
            onClick={() => load(true)}
            className="text-[10px] text-ink-faint transition-colors hover:text-ink-dim"
          >
            {t("common.action.retry")}
          </button>
        </span>
      )}
    </div>
  );
}
