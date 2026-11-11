import {
  cpuReadout,
  diskReadout,
  memoryReadout,
  netDirectionReadout,
  netReadout,
  portReadout,
} from "../lib/statsReadout";
import { useI18n } from "../i18n/store";
import { dayString } from "../lib/format";
import { useVolumeInfo } from "../lib/useVolumeInfo";
import { AreaChart } from "./charts/AreaChart";
import { CountUp } from "./charts/CountUp";
import { DotMatrix } from "./charts/DotMatrix";
import { RingGauge, type RingSegment } from "./charts/RingGauge";
import { MeterBar } from "./charts/MeterBar";
import { ChipIcon, DatabaseIcon, DownloadIcon, MemoryCloudIcon, PortJackIcon } from "./icons";
import { Meter } from "./primitives";
import { useDiskStore } from "../stores/diskStore";
import { useMemStore } from "../stores/memStore";
import { useNetTotalStore } from "../stores/netTotalStore";
import { usePortStore } from "../stores/portStore";
import { formatBytes, formatPercent } from "../lib/format";
import { cutReport, type CutReport } from "../lib/ipc";
import { NetDirectionReading } from "./stats/NetReading";
import { DomainCard } from "./stats/DomainCard";

import { useEffect, useState } from "react";

/**
 * 下钻目标：总览卡只导航，不做任何清理/结束动作（M9 §4，UX v3 §6.1）。
 * v3.1 起 CPU 卡下钻到「应用」页（按 CPU 排序），内存卡到合并后的「内存」页。
 */
export type DrillTab = "stats" | "resources" | "memory" | "disk" | "ports" | "net";

/**
 * 总览页（UI v4「Quiet Cockpit」§5.1）。
 *
 * 两层驾驶舱结构：
 * 1. **Hero 可回收环形仪表**：外环 L1–L4 按 GB 分弧（L4 降透明度——不计入主数字），
 *    环心主数字走 CountUp；未扫描时虚线空轨 +「立即扫描」，绝不显示 0（假事实红线）。
 * 2. **六张域卡**（DomainCard）：域色图标 chip + 大数字 + 微图表；
 *    口径 caption 全部降级为 hover tooltip（少文字原则）。
 *
 * 读数一律来自 `lib/statsReadout.ts`（与菜单栏面板同源），只换排版不换口径；
 * 所有微图表由现有快照前端派生，零新增 IPC、零新增采样。
 */
export function StatsBoard({
  onDrill,
  refreshNonce,
}: {
  onDrill: (tab: DrillTab) => void;
  /** 头部「刷新」按钮的递增信号：变了就重取卷容量（内存/端口由各自 store 刷新） */
  refreshNonce: number;
}) {
  const { t, locale } = useI18n();
  const snapshot = useMemStore((s) => s.snapshot);
  const ports = usePortStore((s) => s.snapshot);
  const report = useDiskStore((s) => s.report);
  const diskScanning = useDiskStore((s) => s.scanning);
  const refreshDisk = useDiskStore((s) => s.refresh);
  const netSample = useNetTotalStore((s) => s.sample);
  const netHistory = useNetTotalStore((s) => s.history);
  // 卷容量：进入 tab、点刷新时各取一次（statvfs 微秒级，无需缓存）
  const { volume, known } = useVolumeInfo(refreshNonce);
  // 剪除成绩单（M21）：进入 tab / 点刷新各取一次；tab 切换会重挂载，天然重新拉取
  const [cut, setCut] = useState<CutReport | null>(null);
  useEffect(() => {
    let alive = true;
    void cutReport()
      .then((r) => {
        if (alive) setCut(r);
      })
      .catch(() => {
        if (alive) setCut(null); // 读不到就当「无记录」，不弹错——成绩单是彩蛋不是告警
      });
    return () => {
      alive = false;
    };
  }, [refreshNonce]);

  const mem = memoryReadout(snapshot, locale);
  const cpu = cpuReadout(snapshot?.cpu, locale);
  const disk = diskReadout(volume, report?.totals.reclaimable ?? null, known, locale);
  const port = portReadout(ports, locale);
  const net = netReadout(netSample, netHistory, locale);
  // 方向读数（M16 v6）：下载 / 上传各一张卡，口径在 readout 层派生
  const netDown = netDirectionReadout(netSample, netHistory, "down", locale);
  const netUp = netDirectionReadout(netSample, netHistory, "up", locale);

  const totals = report?.totals ?? null;

  // Hero 环四段：L1 绿 / L2 蓝 / L3 黄 / L4 红（dim——只给命令不计入主数字）
  const reclaimSegments: RingSegment[] = totals
    ? [
        { value: totals.deadweight, color: "var(--color-accent)" },
        { value: totals.l2, color: "var(--color-info)" },
        { value: totals.l3, color: "var(--color-warn)" },
        { value: totals.l4, color: "var(--color-danger)", dim: true },
      ]
    : [];

  // swap 阈值条（内存卡微图表）：读不到就不渲染，绝不画 0
  const swap = snapshot?.swap;
  const swapRatio = swap && swap.total > 0 ? swap.used / swap.total : null;

  // CPU 近 10 min 走势：history 随快照下发（60 点 @10 s），零新增采样；
  // 点数不足 2 不渲染（AreaChart 内部同规则）
  const cpuHistory = snapshot?.cpu?.history ?? [];

  // 端口点阵：琥珀=对外暴露 / 绿=关注层 / 灰=其他，与端口页分组口径一致
  const portEntries = ports?.entries ?? [];
  const portDots = portEntries.map((e) => ({
    tone: (e.exposure === "any" ? "warn" : e.watched ? "accent" : "neutral") as
      | "warn"
      | "accent"
      | "neutral",
    title: `:${e.port} ${e.proto}${e.listeners[0]?.comm ? ` · ${e.listeners[0].comm}` : ""}`,
  }));

  return (
    <div className="min-h-0 flex-1 overflow-y-auto px-5 py-4">
      <div className="flex items-baseline justify-between">
        <div>
          <h1 className="text-[15px] font-semibold tracking-tight">{t("stats.title")}</h1>
          <p className="mt-0.5 text-[11px] text-ink-faint">{t("stats.subtitle")}</p>
        </div>
        <span className="text-[10px] text-ink-faint">{t("common.hint.statsCadence")}</span>
      </div>

      {/* Hero：可回收环形仪表（GB 可行动，百分比可描述） */}
      <button
        type="button"
        onClick={() => onDrill("disk")}
        title={t("stats.hero.title")}
        className="group mt-4 flex w-full items-center gap-6 rounded-[18px] border border-accent/25 bg-accent/[0.05] px-6 py-4 text-left transition-colors hover:border-accent/45 hover:bg-accent/[0.08]"
      >
        <RingGauge
          segments={reclaimSegments}
          size={124}
          stroke={12}
          empty={!totals}
          className="prune-anim shrink-0"
          ariaLabel={
            totals
              ? t("stats.hero.aria", {
                  total: formatBytes(totals.reclaimable),
                  l1: formatBytes(totals.deadweight),
                  l2: formatBytes(totals.l2),
                  l3: formatBytes(totals.l3),
                  l4: formatBytes(totals.l4),
                })
              : t("stats.hero.ariaEmpty")
          }
        >
          {totals ? (
            <span className="flex flex-col items-center">
              <CountUp
                value={totals.reclaimable}
                format={formatBytes}
                className="font-mono text-[24px] leading-none tabular-nums text-ink"
              />
              <span className="mt-1 text-[9.5px] text-ink-faint">{t("stats.hero.reclaimable")}</span>
            </span>
          ) : (
            <span className="font-mono text-[20px] text-ink-faint">—</span>
          )}
        </RingGauge>

        <div className="min-w-0 flex-1">
          <div className="flex items-baseline justify-between gap-2">
            <span className="text-[11px] font-medium tracking-wide text-accent">{t("stats.hero.breakdown")}</span>
            <span className="text-[10px] text-ink-faint transition-colors group-hover:text-ink-dim">
              {t("stats.hero.toStorage")}
            </span>
          </div>
          {totals ? (
            <>
              <div className="mt-2.5 grid grid-cols-2 gap-x-6 gap-y-1.5">
                <LegendDot color="var(--color-accent)" label={t("stats.tier.l1")} value={formatBytes(totals.deadweight)} />
                <LegendDot color="var(--color-info)" label={t("stats.tier.l2")} value={formatBytes(totals.l2)} />
                <LegendDot color="var(--color-warn)" label={t("stats.tier.l3")} value={formatBytes(totals.l3)} />
                <LegendDot color="var(--color-danger)" label={t("stats.tier.l4")} value={formatBytes(totals.l4)} dim />
              </div>
            </>
          ) : (
            <div className="mt-2.5 flex items-center gap-3">
              <p className="text-[11px] leading-relaxed text-ink-faint">
                {diskScanning ? t("common.progress.scanning") : t("common.hint.firstScan")}
              </p>
              {!diskScanning ? (
                <span
                  role="button"
                  tabIndex={0}
                  onClick={(e) => {
                    e.stopPropagation();
                    void refreshDisk();
                  }}
                  onKeyDown={(e) => {
                    if (e.key === "Enter" || e.key === " ") {
                      e.stopPropagation();
                      void refreshDisk();
                    }
                  }}
                  className="shrink-0 rounded-md bg-white/[0.06] px-2.5 py-1 text-[11px] text-ink-dim transition-colors hover:bg-white/[0.1] hover:text-ink"
                >
                  {t("common.action.scanNow")}
                </span>
              ) : null}
            </div>
          )}
        </div>
      </button>

      {/* 六张域卡：图标 + 大数字 + 微图表，口径 hover */}
      <div className="mt-3 grid grid-cols-2 gap-3">
        <DomainCard
          title={t("stats.card.memory")}
          caption={t("stats.caption.memory")}
          color="var(--color-info)"
          icon={<MemoryCloudIcon size={14} />}
          readout={mem}
          onOpen={() => onDrill("memory")}
          chart={
            swap && swapRatio != null ? (
              <div>
                <div className="flex items-baseline justify-between text-[10px] text-ink-faint">
                  <span>
                    {t("stats.swap.label", {
                      used: formatBytes(swap.used),
                      total: formatBytes(swap.total),
                    })}
                  </span>
                  <span
                    className={`font-mono tabular-nums ${
                      swapRatio >= 0.9 ? "text-danger" : swapRatio >= 0.75 ? "text-warn" : "text-ink-dim"
                    }`}
                  >
                    {formatPercent(swapRatio)}
                  </span>
                </div>
                <MeterBar
                  className="mt-1"
                  value={swap.used}
                  max={swap.total}
                  tone={swapRatio >= 0.9 ? "danger" : swapRatio >= 0.75 ? "warn" : "accent"}
                  thresholds={[0.75, 0.9]}
                  height={4}
                  label={t("stats.swap.aria", { pct: formatPercent(swapRatio) })}
                />
              </div>
            ) : null
          }
        />
        <DomainCard
          title={t("stats.card.cpu")}
          caption={t("stats.caption.cpu")}
          color="var(--color-domain-cpu)"
          icon={<ChipIcon size={14} />}
          readout={cpu}
          onOpen={() => onDrill("resources")}
          chart={
            <div>
              <div className="flex items-baseline justify-between text-[10px] text-ink-faint">
                <span>{t("stats.cpuTrend.label")}</span>
              </div>
              <AreaChart
                className="mt-1"
                values={cpuHistory}
                color="var(--color-domain-cpu)"
                height={26}
                ariaLabel={t("stats.cpuTrend.aria")}
              />
            </div>
          }
        />
        <DomainCard
          title={t("stats.card.disk")}
          caption={t("stats.caption.disk")}
          color="var(--color-accent)"
          icon={<DatabaseIcon size={14} />}
          readout={disk}
          onOpen={() => onDrill("disk")}
          chart={
            disk.ratio != null ? (
              <Meter ratio={disk.ratio} tone={disk.tone} height={4} />
            ) : null
          }
        />
        <DomainCard
          title={t("stats.card.ports")}
          caption={t("stats.caption.ports")}
          color="var(--color-warn)"
          icon={<PortJackIcon size={14} />}
          readout={port}
          onOpen={() => onDrill("ports")}
          chart={
            ports ? (
              <DotMatrix
                items={portDots}
                dot={9}
                ariaLabel={t("stats.portsDotsAria", { n: portEntries.length })}
              />
            ) : null
          }
        />
        {/* 网络拆成两卡（M16 v6）：行数不变（3 行），574/592 高度算式原样成立 */}
        <DomainCard
          title={t("stats.card.down")}
          caption={t("stats.caption.down")}
          color="var(--color-domain-net)"
          icon={<DownloadIcon size={14} />}
          readout={net}
          body={<NetDirectionReading readout={netDown} />}
          onOpen={() => onDrill("net")}
        />
        <DomainCard
          title={t("stats.card.up")}
          caption={t("stats.caption.up")}
          color="var(--color-domain-cpu)"
          icon={<DownloadIcon size={14} className="rotate-180" />}
          readout={net}
          body={<NetDirectionReading readout={netUp} />}
          onOpen={() => onDrill("net")}
        />
      </div>

      {/* 剪除成绩单（M21）：只统计你亲手剪掉的，本地 SQLite append-only */}
      {cut && (
        <div className="mt-3 rounded-[18px] border border-hairline bg-white/[0.02] px-5 py-4">
          <div className="flex items-baseline justify-between">
            <span className="text-[11px] font-medium tracking-wide text-ink-dim">{t("stats.scorecard.title")}</span>
            <span className="text-[10px] text-ink-faint">{t("stats.scorecard.hint")}</span>
          </div>
          {cut.totalItems > 0 ? (
            <>
              <div className="mt-2 flex items-baseline gap-3">
                <CountUp
                  value={cut.totalBytes}
                  format={formatBytes}
                  className="font-mono text-[22px] leading-none tabular-nums text-ink"
                />
                <span className="text-[11px] text-ink-faint">{cutEquivalence(cut.totalBytes, t)}</span>
              </div>
              <p className="mt-1 text-[10px] text-ink-faint">
                {/* 口径：cut_log 一行 = 一个被剪掉的条目，所以这里是「项」不是「次」 */}
                {t("stats.scorecard.items", { n: cut.totalItems })}
                {cut.firstTs != null
                  ? t("stats.scorecard.since", { date: dayString(new Date(cut.firstTs * 1000)) })
                  : ""}
              </p>
              {cut.top.length > 0 && (
                <div className="mt-2.5 grid gap-1">
                  {cut.top.slice(0, 3).map((item, i) => (
                    <div key={item.label} className="flex items-baseline gap-2 text-[11px]">
                      <span className="w-3 shrink-0 font-mono text-ink-faint">{i + 1}</span>
                      <span className="truncate text-ink-dim">{item.label}</span>
                      <span className="ml-auto shrink-0 font-mono tabular-nums text-ink">
                        {formatBytes(item.bytes)}
                      </span>
                      <span className="w-9 shrink-0 text-right text-[10px] text-ink-faint">
                        {t("stats.scorecard.count", { n: item.count })}
                      </span>
                    </div>
                  ))}
                </div>
              )}
            </>
          ) : (
            <div className="mt-2 flex items-center justify-between gap-3">
              <p className="text-[11px] text-ink-faint">{t("stats.scorecard.empty")}</p>
              <button
                type="button"
                onClick={() => onDrill("disk")}
                className="shrink-0 rounded-md bg-white/[0.06] px-2.5 py-1 text-[11px] text-ink-dim transition-colors hover:bg-white/[0.1] hover:text-ink"
              >
                {t("stats.scorecard.goStorage")}
              </button>
            </div>
          )}
        </div>
      )}

      <p className="mt-4 text-[10px] leading-relaxed text-ink-faint">{t("common.hint.statsPrinciple")}</p>
    </div>
  );
}

/** 换算彩蛋：≥4 GB 用「高清电影」（4 GB/部），否则用「照片」（3 MB/张） */
function cutEquivalence(totalBytes: number, t: ReturnType<typeof useI18n>["t"]): string {
  const MOVIE = 4e9;
  const PHOTO = 3e6;
  if (totalBytes >= MOVIE) {
    return t("stats.scorecard.movies", { n: Math.max(1, Math.round(totalBytes / MOVIE)) });
  }
  return t("stats.scorecard.photos", { n: Math.max(1, Math.round(totalBytes / PHOTO)) });
}

function LegendDot({
  color,
  label,
  value,
  dim = false,
}: {
  color: string;
  label: string;
  value: string;
  dim?: boolean;
}) {
  return (
    <span
      className="flex items-center gap-1.5 text-[10px] text-ink-dim"
      style={dim ? { opacity: 0.65 } : undefined}
    >
      <span
        className="size-2 shrink-0 rounded-[3px]"
        style={{ background: color, boxShadow: `0 0 6px color-mix(in srgb, ${color} 35%, transparent)` }}
      />
      <span className="truncate">{label}</span>
      <span className="ml-auto shrink-0 font-mono tabular-nums text-ink">{value}</span>
    </span>
  );
}
