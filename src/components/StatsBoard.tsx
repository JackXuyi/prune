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
import { Btn, Meter, ProposalTag, ScopeBadge } from "./primitives";
import { useDiskStore } from "../stores/diskStore";
import { useMemStore } from "../stores/memStore";
import { useNetTotalStore } from "../stores/netTotalStore";
import { usePortStore } from "../stores/portStore";
import { formatBytes, formatPercent } from "../lib/format";
import { cutReport, isTauri, type CutReport } from "../lib/ipc";
import { devCutReport } from "../lib/devFixture";
import { NetDirectionReading } from "./stats/NetReading";
import { DomainCard } from "./stats/DomainCard";

import { useEffect, useState } from "react";

/**
 * 下钻目标：总览卡只导航，不做任何清理/结束动作（M9 §4，UX v3 §6.1）。
 * v3.1 起 CPU 卡下钻到「应用」页（按 CPU 排序），内存卡到合并后的「内存」页。
 */
export type DrillTab = "stats" | "resources" | "memory" | "disk" | "ports" | "net";

/**
 * 总览页（UI v5「Quiet Cockpit」§4.1）。
 *
 * 一屏三段：
 * 1. **Hero 可回收环**（172 高）：环 148 + 四段分弧，右侧 2×2 图例 + 一行脚注；
 *    未扫描时整块换成「还没扫描过」+ 立即扫描，绝不显示 0（假事实红线）。
 * 2. **六域卡**（96 高 × 3 列 × 2 行）：19px 读数 + 微图表 + 一行口径；
 *    口径正文只在 hover tooltip 里（少文字原则）。
 * 3. **剪除成绩单**：只统计你亲手剪掉的，本地 append-only。
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
    // 浏览器预览没有 cut_report 命令：用 dev 假数据撑起设计稿（仅 DEV，生产仍诚实报错）
    const load = isTauri()
      ? cutReport()
      : import.meta.env.DEV
        ? Promise.resolve(devCutReport())
        : Promise.reject(new Error("needs desktop"));
    void load
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

  // NBA（下一步最佳操作）：只从已有报告派生，不新采集。跨平台二进制是
  // 唯一「零后悔」的量，且本机永远用不上，所以它排在任何其它建议之前。
  const a1 = report?.groups.find((g) => g.deadweight != null) ?? null;
  const dw = a1?.deadweight ?? null;
  const nba = dw && dw.bytes > 0 ? dw : null;

  return (
    <div className="min-h-0 flex-1 overflow-y-auto px-5 pt-3 pb-3">
      {/* UI v5：页名与副标已上移到顶栏（spec §1.3），board 内不再重复同名标题。
          这里只保留「采样节拍」这条只有本页才有的信息。 */}
      <div className="flex items-baseline justify-end">
        <span className="text-[10px] text-ink-faint">{t("common.hint.statsCadence")}</span>
      </div>

      {totals ? (
        /* Hero：可回收环形仪表（GB 可行动，百分比可描述） */
        <button
          type="button"
          onClick={() => onDrill("disk")}
          title={t("stats.hero.title")}
          className="group mt-2 grid w-full grid-cols-[138px_minmax(0,1fr)] items-center gap-4 rounded-hero border border-accent/25 bg-accent/[0.05] px-5 py-2.5 text-left transition-colors hover:border-accent/45 hover:bg-accent/[0.08]"
        >
          <RingGauge
            segments={reclaimSegments}
            size={134}
            stroke={9}
            gapDeg={2}
            className="prune-anim shrink-0"
            ariaLabel={t("stats.hero.aria", {
              total: formatBytes(totals.reclaimable),
              l1: formatBytes(totals.deadweight),
              l2: formatBytes(totals.l2),
              l3: formatBytes(totals.l3),
              l4: formatBytes(totals.l4),
            })}
          >
            <span className="flex flex-col items-center">
              <CountUp
                value={totals.reclaimable}
                format={formatBytes}
                className="font-mono text-[28px] leading-none font-semibold tabular-nums text-ink"
              />
              <span className="mt-1 text-[9.5px] text-ink-faint">{t("stats.hero.reclaimable")}</span>
            </span>
          </RingGauge>

          <div className="min-w-0">
            <div className="flex items-center gap-2">
              <span className="text-[12.5px] font-medium text-ink">{t("stats.hero.breakdown")}</span>
              <ScopeBadge label={t("stats.hero.scope")} tip={t("stats.hero.scopeTip")} />
              <span className="ml-auto shrink-0 text-[10px] text-ink-faint transition-colors group-hover:text-ink-dim">
                {t("stats.hero.toStorage")}
              </span>
            </div>
            <div className="mt-2.5 grid grid-cols-2 gap-x-[18px] gap-y-2.5">
              <LegendRow
                color="var(--color-accent)"
                label={t("stats.tier.l1")}
                value={formatBytes(totals.deadweight)}
              />
              <LegendRow color="var(--color-info)" label={t("stats.tier.l2")} value={formatBytes(totals.l2)} />
              <LegendRow color="var(--color-warn)" label={t("stats.tier.l3")} value={formatBytes(totals.l3)} />
              <LegendRow color="hatch" label={t("stats.tier.l4")} value={formatBytes(totals.l4)} dim />
            </div>
            <p className="mt-2.5 text-[9.5px] leading-none text-ink-faint">{t("stats.hero.l4Note")}</p>
          </div>
        </button>
      ) : (
        /* 未扫描：环画成虚线空轨 + 中央「—」，绝不用 0 冒充事实 */
        <div className="mt-2.5 flex items-start gap-5 rounded-hero border border-hairline-soft bg-surface-raised px-5 py-4">
          <RingGauge
            segments={[]}
            size={104}
            stroke={4}
            empty
            className="shrink-0"
            ariaLabel={t("stats.hero.ariaEmpty")}
          >
            <span className="flex flex-col items-center">
              <span className="font-mono text-[17px] leading-none text-ink-faint">—</span>
              <span className="mt-1 text-[9px] text-ink-faint">{t("common.state.notScanned")}</span>
            </span>
          </RingGauge>
          <div className="min-w-0 flex-1">
            <div className="text-[13px] font-medium text-ink">{t("stats.unscanned.title")}</div>
            <p className="mt-1 text-[10.5px] leading-relaxed text-ink-faint">{t("stats.unscanned.note")}</p>
            <div className="mt-2.5 flex items-center gap-2.5">
              <Btn variant="primary" onClick={() => void refreshDisk()} loading={diskScanning}>
                {t("common.action.scanNow")}
              </Btn>
              <span className="truncate text-[9.5px] text-ink-faint">{t("stats.unscanned.scope")}</span>
            </div>
          </div>
        </div>
      )}

      {/* NBA：只从已有报告派生，点一下就去存储执行 */}
      {nba ? (
        <div className="mt-2 flex items-center gap-4 rounded-card border border-hairline-soft bg-surface-raised px-4 py-2">
          <div className="min-w-0 flex-1">
            <div className="flex items-center gap-1.5">
              <span className="inline-flex shrink-0 items-center rounded-full bg-accent/12 px-1.5 py-px text-[9px] leading-[13px] font-medium whitespace-nowrap text-accent">
                {t("stats.nba.badge")}
              </span>
              <ProposalTag />
            </div>
            <div className="mt-1 truncate text-[12px] font-medium text-ink">
              {t("stats.nba.title", { size: formatBytes(nba.bytes) })}
            </div>
            <div className="mt-0.5 truncate text-[9.5px] text-ink-faint">
              {t("stats.nba.note", { n: nba.entryCount, native: formatBytes(nba.nativeBytes) })}
            </div>
          </div>
          <Btn variant="ghost" onClick={() => onDrill("disk")}>
            {t("stats.nba.action")}
          </Btn>
        </div>
      ) : null}

      {/* 六张域卡：3 列 × 2 行，固定 96 高 */}
      <div className="mt-2.5 grid grid-cols-3 gap-3">
        <DomainCard
          title={t("stats.card.memory")}
          caption={t("stats.caption.memory")}
          color="var(--color-info)"
          icon={<MemoryCloudIcon size={12} />}
          readout={mem}
          onOpen={() => onDrill("memory")}
          chart={
            swap && swapRatio != null ? (
              <MeterBar
                value={swap.used}
                max={swap.total}
                tone={swapRatio >= 0.9 ? "danger" : swapRatio >= 0.75 ? "warn" : "accent"}
                thresholds={[0.75, 0.9]}
                height={3}
                label={t("stats.swap.aria", { pct: formatPercent(swapRatio) })}
              />
            ) : null
          }
          footnote={
            swap && swapRatio != null
              ? t("stats.swap.footnote", {
                  used: formatBytes(swap.used),
                  total: formatBytes(swap.total),
                  pct: formatPercent(swapRatio),
                })
              : undefined
          }
        />
        <DomainCard
          title={t("stats.card.cpu")}
          caption={t("stats.caption.cpu")}
          color="var(--color-domain-cpu)"
          icon={<ChipIcon size={12} />}
          readout={cpu}
          onOpen={() => onDrill("resources")}
          chart={
            <AreaChart
              values={cpuHistory}
              color="var(--color-domain-cpu)"
              height={16}
              ariaLabel={t("stats.cpuTrend.aria")}
            />
          }
          footnote={t("stats.cpuTrend.label")}
        />
        <DomainCard
          title={t("stats.card.disk")}
          caption={t("stats.caption.disk")}
          color="var(--color-accent)"
          icon={<DatabaseIcon size={12} />}
          readout={disk}
          onOpen={() => onDrill("disk")}
          chart={disk.ratio != null ? <Meter ratio={disk.ratio} tone={disk.tone} height={3} /> : null}
          footnote={disk.sub[0]}
        />
        <DomainCard
          title={t("stats.card.ports")}
          caption={t("stats.caption.ports")}
          color="var(--color-domain-port)"
          icon={<PortJackIcon size={12} />}
          readout={port}
          onOpen={() => onDrill("ports")}
          chart={
            ports ? (
              /* 17 颗一行 + 「+N」收尾：96 px 卡里两行点阵会把脚注挤掉 */
              <DotMatrix
                items={portDots}
                dot={6}
                gap={4}
                max={17}
                ariaLabel={t("stats.portsDotsAria", { n: portEntries.length })}
              />
            ) : null
          }
          footnote={port.sub[0]}
        />
        {/* 网络拆成两卡（M16 v6）：下载 / 上传各自刻度，共享纵轴会把上传压平 */}
        <DomainCard
          title={t("stats.card.down")}
          caption={t("stats.caption.down")}
          color="var(--color-domain-net)"
          icon={<DownloadIcon size={12} />}
          readout={net}
          body={<NetDirectionReading readout={netDown} />}
          onOpen={() => onDrill("net")}
        />
        <DomainCard
          title={t("stats.card.up")}
          caption={t("stats.caption.up")}
          color="var(--color-domain-cpu)"
          icon={<DownloadIcon size={12} className="rotate-180" />}
          readout={net}
          body={<NetDirectionReading readout={netUp} />}
          onOpen={() => onDrill("net")}
        />
      </div>

      {/* 剪除成绩单（M21）：只统计你亲手剪掉的，本地 SQLite append-only。
          左：累计 + 换算；右：Top3。横排是为了在 640 高的窗口里和上面几段一起首屏读完。 */}
      {cut ? (
        <div className="mt-2.5 rounded-card border border-hairline-soft bg-surface-raised px-4 py-2.5">
          <div className="flex items-start gap-5">
            <div className="min-w-0 flex-1">
              <div className="flex items-baseline justify-between gap-2">
                <span className="text-[11px] font-medium tracking-wide text-ink-dim">
                  {t("stats.scorecard.title")}
                </span>
                <span className="shrink-0 text-[9px] text-ink-faint">{t("stats.scorecard.hint")}</span>
              </div>
              {cut.totalItems > 0 ? (
                <>
                  <div className="mt-1 flex items-baseline gap-2.5">
                    <CountUp
                      value={cut.totalBytes}
                      format={formatBytes}
                      className="font-mono text-[20px] leading-none tabular-nums text-ink"
                    />
                    <span className="truncate text-[10px] text-ink-faint">
                      {cutEquivalence(cut.totalBytes, t)}
                    </span>
                  </div>
                  <p className="mt-0.5 text-[9.5px] text-ink-faint">
                    {/* 口径：cut_log 一行 = 一个被剪掉的条目，所以这里是「项」不是「次」 */}
                    {t("stats.scorecard.items", { n: cut.totalItems })}
                    {cut.firstTs != null
                      ? t("stats.scorecard.since", { date: dayString(new Date(cut.firstTs * 1000)) })
                      : ""}
                  </p>
                </>
              ) : (
                <div className="mt-2 flex items-center gap-2.5">
                  <p className="text-[10.5px] leading-relaxed text-ink-faint">
                    {t("stats.scorecard.empty")}
                  </p>
                  <Btn variant="ghost" onClick={() => onDrill("disk")}>
                    {t("stats.scorecard.goStorage")}
                  </Btn>
                </div>
              )}
            </div>
            {cut.totalItems > 0 && cut.top.length > 0 ? (
              <div className="grid w-[212px] shrink-0 content-start gap-0.5 border-l border-hairline-soft pl-4">
                {cut.top.slice(0, 3).map((item, i) => (
                  <div key={item.label} className="flex items-baseline gap-1.5 text-[10px]">
                    <span className="w-2.5 shrink-0 font-mono text-ink-faint">{i + 1}</span>
                    <span className="truncate text-ink-dim">{item.label}</span>
                    <span className="ml-auto shrink-0 font-mono tabular-nums text-ink">
                      {formatBytes(item.bytes)}
                    </span>
                  </div>
                ))}
              </div>
            ) : null}
          </div>
        </div>
      ) : null}

      <p className="mt-2 text-[9px] leading-relaxed text-ink-faint">{t("common.hint.statsPrinciple")}</p>
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

/**
 * 图例行：`8px 1fr auto` 三列，数值列右对齐 —— 四项数值必须在同一条竖线上，
 * 否则「谁大谁小」要靠读数字，图例就白画了。
 */
function LegendRow({
  color,
  label,
  value,
  dim = false,
}: {
  /** 域色；传 "hatch" 时画成斜纹块（L4 不计入的视觉约定） */
  color: string;
  label: string;
  value: string;
  dim?: boolean;
}) {
  const swatch =
    color === "hatch"
      ? "repeating-linear-gradient(45deg, var(--color-hatch) 0 2px, transparent 2px 4px)"
      : color;
  return (
    <span
      className="grid grid-cols-[8px_minmax(0,1fr)_auto] items-center gap-1.5"
      style={dim ? { opacity: 0.55 } : undefined}
    >
      <i
        className="size-2 shrink-0 rounded-[2px]"
        style={{
          background: swatch,
          boxShadow: color === "hatch" ? undefined : `0 0 6px color-mix(in srgb, ${color} 35%, transparent)`,
        }}
        aria-hidden
      />
      <span className="truncate text-[11px] text-ink-dim">{label}</span>
      <span className="shrink-0 text-right font-mono text-[11.5px] font-medium tabular-nums text-ink">
        {value}
      </span>
    </span>
  );
}
