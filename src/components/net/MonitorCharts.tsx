import { useMemo, useState } from "react";

import type { DayTraffic, IfaceRate, NetTotalStatus } from "../../types/net";
import { formatBytes, recentDays } from "../../lib/format";
import { useI18n } from "../../i18n/store";
import { useMemStore } from "../../stores/memStore";
import { useNetSpeedStore } from "../../stores/netStore";
import {
  formatSpeed,
  ifaceKindLabel,
  ifaceTotalBps,
  netSpeedOfPids,
} from "../../types/net";
import { MeterBar } from "../charts/MeterBar";
import { SectionHead } from "./bits";

/**
 * 监测页图表件：近 7 天柱、网卡列表（物理 + 折叠虚拟口）、按应用排名。
 * 自 NetBoard 抽出，逻辑不变——见 MonitorPage 顶注的三条纪律。
 */

// ---------------------------------------------------------------- 7 日柱

/**
 * 近 7 天柱（纯 SVG 不引图表库）。
 *
 * 缺失日画 **1 px 灰条**而不是 0 柱：0 柱的语义是「那天确实没有流量」，
 * 而 Prune 没运行的日子是「不知道」。底部同时标「已记录 N/7 天」。
 */
export function DayBars({ days, count }: { days: DayTraffic[]; count: number }) {
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
                <div className="h-px w-full bg-chip-hover" />
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
              k === todayKey ? "font-medium text-domain-net" : "text-ink-faint"
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

// ---------------------------------------------------------------- 网卡列表

/** 物理口逐行 + 虚拟口折叠；warmup / 空数据的口径文案在这里分流 */
export function IfaceList({
  status,
  physical,
  virtual,
}: {
  status: NetTotalStatus;
  physical: IfaceRate[];
  virtual: IfaceRate[];
}) {
  const { t } = useI18n();
  const maxBps = maxRate(physical);
  return (
    <>
      {status === "warmup" ? (
        <p className="mt-2 text-[11px] text-ink-faint">{t("net.iface.warmup")}</p>
      ) : physical.length === 0 ? (
        <p className="mt-2 text-[11px] text-ink-faint">{t("net.iface.empty")}</p>
      ) : (
        <ul className="mt-2 space-y-1.5">
          {physical.map((i) => (
            <IfaceRow key={i.name} iface={i} maxBps={maxBps} />
          ))}
        </ul>
      )}
      <VirtualIfaces ifaces={virtual} maxBps={maxBps} />
    </>
  );
}

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
        className="flex w-full items-center gap-1.5 rounded-md px-1 py-1 text-left text-[10px] text-ink-faint transition-colors hover:bg-chip hover:text-ink-dim"
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

export function AppRanking({
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
      .sort((a, b) => {
        const ta = a.speed.downBps + a.speed.upBps;
        const tb = b.speed.downBps + b.speed.upBps;
        // 相差 < 5% 视为同档按名称排：瞬时差分有噪声，纯按值排会让相邻行每秒洗牌
        if (Math.abs(ta - tb) >= Math.max(ta, tb) * 0.05) return tb - ta;
        return a.family.localeCompare(b.family);
      });
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
                  className="flex w-full items-center gap-3 rounded-md px-1 py-1 text-left transition-colors hover:bg-chip"
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
