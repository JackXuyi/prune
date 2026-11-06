import { useMemo, useState } from "react";

import type { DayTraffic, IfaceRate, NetTotalSample, NetTotalStatus } from "../types/net";
import { formatBytes, formatClock, recentDays } from "../lib/format";
import { useMemStore } from "../stores/memStore";
import { useNetSpeedStore } from "../stores/netStore";
import { TRAFFIC_DAYS, useNetTotalStore } from "../stores/netTotalStore";
import {
  formatSpeed,
  ifaceKindLabel,
  ifaceTotalBps,
  netSpeedOfPids,
  splitIfaces,
  weightedAverage,
} from "../types/net";
import { Badge, Meter } from "./primitives";
import { Sparkline } from "./Sparkline";

/**
 * 网络 tab（M14）：本机下载 / 上传速率、走势、网卡分布、今日流量、按应用排名。
 *
 * 三条纪律（方案 §3 / §7）：
 * - 主数字只算**物理网卡**：开 VPN 时 `utun3` 与 `en0` 是同一批字节，相加等于双计
 * - `warmup` 显示「采样中…」、`unavailable` 显示「不可读 + 原因」，**都不显示 0**
 * - 「今日」是 **Prune 运行期间累计**：系统计数器只给「开机至今」，未运行时段无法回填
 *
 * 组件内不订阅、不采样：数据全部来自 store，保持「谁可见谁负责」的既有分工。
 */
export function NetBoard({ onOpenResources }: { onOpenResources: () => void }) {
  const sample = useNetTotalStore((s) => s.sample);
  const history = useNetTotalStore((s) => s.history);
  const days = useNetTotalStore((s) => s.days);

  const status: NetTotalStatus = sample?.status ?? "warmup";
  const { physical, virtual } = splitIfaces(sample?.ifaces ?? []);
  const avg = useMemo(() => weightedAverage(history), [history]);
  const downSeries = useMemo(() => history.map((p) => p.downBps), [history]);
  const upSeries = useMemo(() => history.map((p) => p.upBps), [history]);

  return (
    <div className="min-h-0 flex-1 overflow-y-auto px-5 py-4">
      <div className="flex items-baseline justify-between">
        <div>
          <h1 className="text-[15px] font-semibold tracking-tight">网络</h1>
          <p className="mt-0.5 text-[11px] text-ink-faint">
            系统口径的下载 / 上传速率 · 只算物理网卡（虚拟接口单列，避免与 VPN 重复计数）
          </p>
        </div>
        <span className="text-[10px] text-ink-faint">实时 1 s · 仅物理网卡</span>
      </div>

      {status === "unavailable" ? (
        <p className="mt-3 rounded-lg bg-warn/[0.08] px-3 py-2 text-[11px] leading-relaxed text-warn">
          不可读：{sample?.note ?? "系统网卡计数器读不到"}。这里不显示 0
          —— 读不到和「没有流量」是两件事。
        </p>
      ) : null}

      <div className="mt-4 grid grid-cols-2 gap-8 border-b border-hairline-soft pb-4">
        <DirectionReading
          arrow="↓"
          label="下载"
          tone="accent"
          status={status}
          bps={sample?.downBps ?? 0}
          values={downSeries}
        />
        <DirectionReading
          arrow="↑"
          label="上传"
          tone="info"
          status={status}
          bps={sample?.upBps ?? 0}
          values={upSeries}
        />
      </div>

      <Aggregates sample={sample} avgDown={avg.downBps} avgUp={avg.upBps} />

      <section className="mt-4 border-b border-hairline-soft pb-4">
        <SectionHead title="近 7 天" hint="下 / 上堆叠 · 缺失日灰显，不画成 0" />
        <DayBars days={days} count={TRAFFIC_DAYS} />
      </section>

      <section className="mt-4 border-b border-hairline-soft pb-4">
        <SectionHead title="网卡" hint="物理口合计 = 上面的数字" />
        {status === "warmup" ? (
          <p className="mt-2 text-[11px] text-ink-faint">采样中…（第一拍只建立基线，约 1–2 s）</p>
        ) : physical.length === 0 ? (
          <p className="mt-2 text-[11px] text-ink-faint">
            没有可用的物理网卡读数。断网时这里是 0 B/s，而不是「不可读」。
          </p>
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

      <p className="mt-3 text-[10px] leading-relaxed text-ink-faint">
        口径：网卡计数器差分，排除回环与虚拟接口；「今日」为 Prune 运行期间累计（本地日期，
        每分钟落库），未运行时段无法回填。按应用来自 nettop，与网卡口径不必相等
        ——一份按网卡、一份按进程，差几个百分点属正常。
      </p>
    </div>
  );
}

// ---------------------------------------------------------------- 主读数

/** 大字读数 + 独立走势（各自归一化：上下行量级常差 10 倍，共享纵轴会把上传压平） */
function DirectionReading({
  arrow,
  label,
  tone,
  status,
  bps,
  values,
}: {
  arrow: string;
  label: string;
  tone: "accent" | "info";
  status: NetTotalStatus;
  bps: number;
  values: number[];
}) {
  const { value, unit } = splitSpeed(bps);
  const valueTone = tone === "accent" ? "text-accent" : "text-info";
  const ready = status !== "warmup";

  return (
    <div>
      <div className="flex items-baseline justify-between">
        <span className="text-[11px] tracking-wide text-ink-dim">
          <span className={tone === "accent" ? "text-accent" : "text-info"}>{arrow}</span> {label}
        </span>
        <span className="text-[10px] text-ink-faint">各自刻度</span>
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
          <span className="text-[13px] leading-none text-ink-faint">采样中…</span>
        )}
      </div>
      <div className="mt-2">
        {values.length >= 2 ? (
          <Sparkline values={values} height={52} tone={tone} />
        ) : (
          <div className="flex h-[52px] items-center justify-center rounded-lg border border-dashed border-hairline-soft text-[10px] text-ink-faint">
            走势积累中（每秒一个点）
          </div>
        )}
      </div>
      <div className="mt-1 flex items-center justify-between text-[9.5px] text-ink-faint">
        <span>近 5 min</span>
        <span>
          {ready && bps === 0 ? "本拍无流量" : ready ? formatSpeed(bps) : "—"}
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
  const ready = sample?.status === "ok";
  const today = sample?.today ?? null;
  const missingPersist = (sample?.note ?? "").includes("未落库");

  return (
    <div className="grid grid-cols-2 gap-x-8 gap-y-3 border-b border-hairline-soft py-4">
      <MiniStat
        label="本次会话峰值"
        main={ready ? `↓ ${formatSpeed(sample.peakDownBps)}` : "—"}
        sub={
          ready
            ? `↑ ${formatSpeed(sample.peakUpBps)}${sample.peakAt ? ` · ${formatClock(sample.peakAt)}` : ""}`
            : "还没有读数"
        }
        title="本次会话内单拍最大值，与走势窗口无关（否则「峰值」会随窗口缩水）"
      />
      <MiniStat
        label="近 5 min 平均"
        main={`↓ ${formatSpeed(avgDown)}`}
        sub={`↑ ${formatSpeed(avgUp)} · 时间加权`}
        title="按点间隔加权：采样抖动时简单平均会放大短间隔的尖峰"
      />
      <MiniStat
        label="今日"
        main={today ? `↓ ${formatBytes(today.downBytes)}` : "不可用"}
        sub={
          today ? `↑ ${formatBytes(today.upBytes)}` : "存储打不开，实时速率与会话累计照常"
        }
        title="Prune 运行期间累计：未运行的时段无法回填（系统计数器只给「开机至今」）"
        warn={missingPersist}
        warnText="⚠ 未落库"
      />
      <MiniStat
        label="本次会话"
        main={sample ? `↓ ${formatBytes(sample.sessionDownBytes)}` : "—"}
        sub={sample ? `↑ ${formatBytes(sample.sessionUpBytes)} · 自启动起` : "还没有读数"}
        title="自 Prune 启动以来：回答「这次开了多久用了多少」，不跨重启"
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
  const labels = useMemo(() => recentDays(count), [count]);
  const map = useMemo(() => new Map(days.map((d) => [d.day, d])), [days]);
  const max = Math.max(
    1,
    ...labels.map((k) => {
      const t = map.get(k);
      return t ? t.downBytes + t.upBytes : 0;
    }),
  );
  const recorded = labels.filter((k) => map.has(k)).length;

  return (
    <div className="mt-2">
      <div className="flex h-16 items-end gap-2">
        {labels.map((k) => {
          const t = map.get(k);
          const pct = t ? ((t.downBytes + t.upBytes) / max) * 100 : 0;
          const upShare = t && t.downBytes + t.upBytes > 0 ? t.upBytes / (t.downBytes + t.upBytes) : 0;
          return (
            <div
              key={k}
              className="flex h-full min-w-0 flex-1 flex-col justify-end"
              title={
                t
                  ? `${k} · ↓ ${formatBytes(t.downBytes)} / ↑ ${formatBytes(t.upBytes)}`
                  : `${k} · 没有记录（Prune 未运行）`
              }
            >
              {t ? (
                <div
                  className="flex w-full flex-col overflow-hidden rounded-[2px]"
                  style={{ height: `${Math.max(4, pct)}%` }}
                >
                  <div className="bg-info/70" style={{ height: `${upShare * 100}%` }} />
                  <div className="flex-1 bg-accent/80" />
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
              map.has(k) ? "text-ink-faint" : "text-ink-faint/50"
            }`}
          >
            {k.slice(5)}
          </span>
        ))}
      </div>
      <div className="mt-1 flex items-center justify-between text-[10px] text-ink-faint">
        <span>
          <span className="text-accent">▮</span> 下载 <span className="text-info">▮</span> 上传 · 各日按 7 天内最大值
        </span>
        <span>
          已记录 <span className="font-mono tabular-nums text-ink-dim">{recorded}</span>/{count} 天
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
  const ratio = ifaceTotalBps(iface) / maxBps;
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
        <Meter ratio={ratio} tone={ifaceTotalBps(iface) > 0 ? "accent" : "neutral"} height={4} />
      </span>
      <span className="w-[74px] shrink-0 text-right font-mono text-[10px] tabular-nums text-ink-faint">
        {formatBytes(iface.sessionDownBytes + iface.sessionUpBytes)}
      </span>
    </li>
  );
}

/** 虚拟接口默认折叠：一排 `awdl0 0 B/s` 会抢走注意力，但信息不能丢 */
function VirtualIfaces({ ifaces, maxBps }: { ifaces: IfaceRate[]; maxBps: number }) {
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
          虚拟接口 {ifaces.length} 个（{names}）
          <span className="ml-1.5 text-ink-faint/80">不计入主数字，避免与物理口重复</span>
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
                <Meter ratio={ifaceTotalBps(i) / maxBps} tone="neutral" height={3} />
              </span>
              <span className="w-[52px] shrink-0 text-right text-[9.5px] text-ink-faint">
                {ifaceKindLabel(i.kind)}
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
  const degraded =
    netSample == null
      ? "采样中…（按应用需要 nettop，进入本 tab 后 1 s 内出数）"
      : netSample.status === "unavailable"
        ? `按应用不可读：${netSample.note ?? "nettop 不可用"}（主数字与今日不受影响）`
        : netStatus === "unavailable"
          ? "按应用暂时不可读"
          : null;

  return (
    <section className="mt-4">
      <SectionHead title="按应用" hint="nettop · 仅非回环 · 按速率降序" />
      {degraded ? (
        <p className="mt-2 text-[11px] text-ink-faint">{degraded}</p>
      ) : rows.length === 0 ? (
        <p className="mt-2 text-[11px] text-ink-faint">本拍没有应用在跑流量。</p>
      ) : (
        <>
          <ul className="mt-2 space-y-0.5">
            {shown.map((r) => (
              <li key={r.family}>
                <button
                  type="button"
                  onClick={onOpenResources}
                  title="在「应用资源」里看这个应用的完整画像"
                  className="flex w-full items-center gap-3 rounded-md px-1 py-1 text-left transition-colors hover:bg-white/[0.04]"
                >
                  <span className="min-w-0 flex-1 truncate text-[11.5px] text-ink">{r.family}</span>
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
              {expanded ? "收起" : `▸ 其余 ${restCount} 个应用`}
            </button>
          ) : null}
        </>
      )}
    </section>
  );
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
