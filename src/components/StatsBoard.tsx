import {
  cpuReadout,
  diskReadout,
  memoryReadout,
  netDirectionReadout,
  portReadout,
} from "../lib/statsReadout";
import { useI18n } from "../i18n/store";
import { dayString, formatBytes, formatPercent } from "../lib/format";
import { useVolumeInfo } from "../lib/useVolumeInfo";
import { AreaChart } from "./charts/AreaChart";
import { CountUp } from "./charts/CountUp";
import { DotMatrix } from "./charts/DotMatrix";
import { MeterBar } from "./charts/MeterBar";
import { RingGauge, type RingSegment } from "./charts/RingGauge";
import {
  ChevronRightIcon,
  ChipIcon,
  DatabaseIcon,
  DeviceIcon,
  DownloadIcon,
  MemoryCloudIcon,
  PortJackIcon,
  ScissorsIcon,
  UploadIcon,
} from "./icons";
import { Btn, Meter } from "./primitives";
import { useDiskStore } from "../stores/diskStore";
import { useLanStore } from "../stores/lanStore";
import { useMemStore } from "../stores/memStore";
import { useNetTotalStore } from "../stores/netTotalStore";
import { usePortStore } from "../stores/portStore";
import { portConflicts } from "../types/net";
import { cutReport, isTauri, type CutReport } from "../lib/ipc";
import { devCutReport } from "../lib/devFixture";
import { InfoTile } from "./stats/InfoTile";
import { StatusRibbon } from "./stats/StatusRibbon";
import { useHardwareReadouts } from "./stats/useHardwareReadouts";

import { useEffect, useState } from "react";

import type { DrillTab } from "../types/drill";

/** 总览下钻目标（含设备页）；类型本体在 types/drill（避免与 StatusRibbon 成环） */
export type { DrillTab };

/**
 * 总览页（eul 式图标优先重设计，设计稿 output/design/stats-eul-mockup.html）。
 *
 * 一屏四段：
 * 1. **状态带**：7 段「仪表/图标 + mono 读数」，hairline 分隔——CPU / 内存 /
 *    磁盘 / 下载 / 上传 / 端口 / 电池，扫一眼即得全部活值，口径全在 tooltip；
 * 2. **Hero 可回收环**：环 + L1–L4 徽标图例（色条 + 数值）+ 一行弱化说明；
 *    未扫描时整块换成「还没扫描过」+ 立即扫描，绝不显示 0（假事实红线）；
 * 3. **六磁贴**：仪表即语义 + 标题 + 大读数 + 一行口径 + 微图表；
 * 4. **剪除成绩单**：奖章位 + 大数字 + Top3 迷你条，本地 append-only。
 *
 * 相比 v5 的取舍（已获确认）：NBA 提案条与本地服务卡撤出总览——NBA 的核心
 * 量（可回收）已在磁盘磁贴口径行，端口关注/冲突聚合进端口磁贴，行级明细
 * 在端口页；硬件读数条拆进状态带电池段与设备磁贴（useHardwareReadouts）。
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
  // 设备磁贴：局域网设备数（init 只拉缓存快照，绝不触发扫描/权限弹窗）
  const lan = useLanStore((s) => s.snapshot);
  const lanInit = useLanStore((s) => s.init);
  // 状态带电池段 + 设备磁贴（门控采样在 hook 内，谁可见谁负责）
  const { battery, accessories, temp } = useHardwareReadouts();
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
  useEffect(() => {
    void lanInit();
  }, [lanInit]);

  const mem = memoryReadout(snapshot, locale);
  const cpu = cpuReadout(snapshot?.cpu, locale);
  const disk = diskReadout(volume, report?.totals.reclaimable ?? null, known, locale);
  const port = portReadout(ports, locale);
  // 方向读数（M16 v6）：下载 / 上传各自口径，状态带与网络磁贴共用
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

  // swap 阈值条（内存磁贴微图表）：读不到就不渲染，绝不画 0
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
  const conflicts = portConflicts(portEntries).length;
  const exposedCount = ports ? portEntries.filter((e) => e.exposure === "any").length : null;

  // 设备磁贴数据：局域网设备（排除本机）+ 硬件读数（失败即隐藏）
  const lanDevices = lan ? lan.devices.filter((d) => !d.isSelf) : null;
  const deviceSubParts = [
    temp ? t("stats.tile.deviceTemp", { temp: Math.round(temp.celsius) }) : null,
    battery?.percent != null
      ? t("hw.strip.batteryAria", { pct: battery.percent.toFixed(0) })
      : null,
    accessories.length > 0 ? t("stats.tile.accessories", { n: accessories.length }) : null,
    lanDevices != null ? t("stats.tile.lanDevices", { n: lanDevices.length }) : null,
  ].filter((s): s is string => s != null);

  // 网络磁贴微图表：下载/上传各占一根的比例（warmup 时 bps 为 0，不给假比例）
  const netTotalBps = netDown.bps + netUp.bps;
  const netDownShare =
    netDown.status === "ok" && netTotalBps > 0 ? netDown.bps / netTotalBps : null;

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-2.5 overflow-y-auto px-5 pt-3 pb-3">
      {/* UI v5：页名与副标已上移到顶栏（spec §1.3），board 内不重复同名标题 */}

      {/* ① 状态带：7 段活值，口径在 tooltip，点段直达对应页 */}
      <StatusRibbon
        cpu={cpu}
        mem={mem}
        disk={disk}
        port={port}
        netDown={netDown}
        netUp={netUp}
        battery={battery}
        exposedCount={exposedCount}
        onDrill={onDrill}
      />

      {totals ? (
        /* ② Hero：可回收环形仪表（GB 可行动，百分比可描述） */
        <button
          type="button"
          onClick={() => onDrill("disk")}
          title={t("stats.hero.title")}
          className="group mt-1 grid w-full grid-cols-[138px_minmax(0,1fr)_auto] items-center gap-5 rounded-hero border border-accent/25 bg-surface-raised px-5 py-3 text-left transition-colors hover:border-accent/45"
          style={{
            backgroundImage:
              "linear-gradient(120deg, color-mix(in srgb, var(--color-accent) 10%, transparent), color-mix(in srgb, var(--color-accent) 3%, transparent) 48%, transparent 78%)",
          }}
        >
          <span
            className="grid shrink-0 place-items-center"
            style={{ filter: "drop-shadow(0 0 16px color-mix(in srgb, var(--color-accent) 22%, transparent))" }}
          >
            <RingGauge
              segments={reclaimSegments}
              size={134}
              stroke={9}
              gapDeg={2}
              className="prune-anim"
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
                  className="font-mono text-[26px] leading-none font-semibold tabular-nums text-ink"
                />
                <span className="mt-1 text-[9.5px] text-ink-faint">{t("stats.hero.reclaimable")}</span>
              </span>
            </RingGauge>
          </span>

          <div className="min-w-0">
            <div className="grid content-start gap-2">
              <TierRow
                lv="L1"
                label={t("stats.tier.l1")}
                value={formatBytes(totals.deadweight)}
                bytes={totals.deadweight}
                max={tierMax(totals)}
                color="var(--color-accent)"
              />
              <TierRow
                lv="L2"
                label={t("stats.tier.l2")}
                value={formatBytes(totals.l2)}
                bytes={totals.l2}
                max={tierMax(totals)}
                color="var(--color-info)"
              />
              <TierRow
                lv="L3"
                label={t("stats.tier.l3")}
                value={formatBytes(totals.l3)}
                bytes={totals.l3}
                max={tierMax(totals)}
                color="var(--color-warn)"
              />
              <TierRow
                lv="L4"
                label={t("stats.tier.l4")}
                value={formatBytes(totals.l4)}
                bytes={totals.l4}
                max={tierMax(totals)}
                color="var(--color-danger)"
                hatch
                dim
              />
            </div>
            <p className="mt-2.5 text-[9.5px] leading-none text-ink-faint">{t("stats.hero.note")}</p>
          </div>

          <span
            className="grid size-[30px] place-items-center rounded-full text-accent transition-transform group-hover:translate-x-0.5"
            style={{ background: "color-mix(in srgb, var(--color-accent) 14%, transparent)" }}
            aria-hidden
          >
            <ChevronRightIcon size={14} />
          </span>
        </button>
      ) : (
        /* 未扫描：环画成虚线空轨 + 中央「—」，绝不用 0 冒充事实 */
        <div className="mt-1 flex items-start gap-5 rounded-hero border border-hairline-soft bg-gradient-to-b from-chip-soft to-transparent bg-surface-raised px-5 py-4">
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

      {/* ③ 六磁贴：仪表即语义 + 标题 + 大读数 + 一行口径 + 微图表 */}
      <div className="grid flex-1 grid-cols-3 grid-rows-2 gap-3">
        <InfoTile
          title={t("stats.card.memory")}
          caption={`${t("stats.caption.memory")}${mem.sub[0] ? ` · ${mem.sub[0]}` : ""}`}
          color="var(--color-info)"
          icon={<MemoryCloudIcon size={12} />}
          gaugeRatio={mem.status === "ok" ? mem.ratio : null}
          main={{ value: mem.main, unit: mem.unit, tail: mem.mainTail, tone: mem.tone }}
          note={mem.note}
          sub={
            swap && swapRatio != null
              ? t("stats.swap.footnote", {
                  used: formatBytes(swap.used),
                  total: formatBytes(swap.total),
                  pct: formatPercent(swapRatio),
                })
              : mem.sub[0]
          }
          viz={
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
          onOpen={() => onDrill("memory")}
        />
        <InfoTile
          title={t("stats.card.cpu")}
          caption={`${t("stats.caption.cpu")}${cpu.sub[0] ? ` · ${cpu.sub[0]}` : ""}`}
          color="var(--color-domain-cpu)"
          icon={<ChipIcon size={12} />}
          gaugeRatio={cpu.status === "ok" ? cpu.ratio : null}
          main={{ value: cpu.main, unit: cpu.unit, tone: cpu.tone }}
          note={cpu.note}
          sub={t("stats.cpuTrend.label")}
          viz={
            <AreaChart
              values={cpuHistory}
              color="var(--color-domain-cpu)"
              height={16}
              ariaLabel={t("stats.cpuTrend.aria")}
            />
          }
          onOpen={() => onDrill("resources")}
        />
        <InfoTile
          title={t("stats.card.disk")}
          caption={`${t("stats.caption.disk")}${disk.sub[0] ? ` · ${disk.sub[0]}` : ""}`}
          color="var(--color-accent)"
          icon={<DatabaseIcon size={12} />}
          gaugeRatio={disk.status === "ok" ? disk.ratio : null}
          main={{ value: disk.main, unit: disk.unit, tail: disk.mainTail, tone: disk.tone }}
          note={disk.note}
          sub={disk.sub[0] ?? disk.sub[1]}
          viz={disk.ratio != null ? <Meter ratio={disk.ratio} tone={disk.tone} height={3} /> : null}
          onOpen={() => onDrill("disk")}
        />
        <InfoTile
          title={t("stats.card.ports")}
          caption={`${t("stats.caption.ports")}${port.sub[0] ? ` · ${port.sub[0]}` : ""}`}
          color="var(--color-domain-port)"
          icon={<PortJackIcon size={13} />}
          main={port.status === "ok" ? { value: port.main } : undefined}
          note={port.note}
          sub={
            port.status === "ok"
              ? [port.sub[0], port.sub[1], conflicts > 0 ? t("stats.tile.conflicts", { n: conflicts }) : null]
                  .filter((s): s is string => s != null)
                  .join(" · ")
              : undefined
          }
          viz={
            ports ? (
              <DotMatrix
                items={portDots}
                dot={6}
                gap={4}
                max={17}
                ariaLabel={t("stats.portsDotsAria", { n: portEntries.length })}
              />
            ) : null
          }
          onOpen={() => onDrill("ports")}
        />
        <InfoTile
          title={t("stats.card.down")}
          caption={`${t("stats.caption.down")} · ${t("stats.net.m5", { value: netDown.avgText })}`}
          color="var(--color-domain-net)"
          icon={<DownloadIcon size={13} />}
          main={
            netDown.status === "ok"
              ? { value: netDown.main, unit: netDown.unit, tone: "accent" }
              : undefined
          }
          note={netDown.note}
          sub={
            netDown.todayText != null
              ? t("stats.tile.netSub", {
                  up: `${netUp.main} ${netUp.unit}`.trim(),
                  today: netDown.todayText,
                })
              : t("stats.net.m5", { value: netDown.avgText })
          }
          viz={
            netDownShare != null ? (
              <div className="flex items-center gap-2" aria-hidden>
                <DownloadIcon size={12} className="shrink-0 text-domain-net" />
                <UploadIcon size={12} className="shrink-0 text-domain-cpu" />
                <span className="relative h-[3px] min-w-0 flex-1 overflow-hidden rounded-full bg-chip">
                  <i
                    className="absolute inset-y-0 left-0 rounded-full bg-domain-net"
                    style={{ width: `${netDownShare * 100}%` }}
                  />
                  <i
                    className="absolute inset-y-0 right-0 rounded-full bg-domain-cpu"
                    style={{ width: `${(1 - netDownShare) * 100}%` }}
                  />
                </span>
              </div>
            ) : null
          }
          onOpen={() => onDrill("net")}
        />
        <InfoTile
          title={t("nav.device")}
          caption={t("nav.deviceHint")}
          color="var(--color-domain-device)"
          icon={<DeviceIcon size={13} />}
          main={
            lanDevices != null
              ? { value: String(lanDevices.length), unit: t("stats.tile.deviceUnit") }
              : undefined
          }
          note={lanDevices == null ? t("common.state.notScanned") : undefined}
          sub={deviceSubParts.length > 0 ? deviceSubParts.slice(0, 3).join(" · ") : undefined}
          viz={<DeviceViz devices={lanDevices} tempCelsius={temp?.celsius ?? null} />}
          onOpen={() => onDrill("device")}
        />
      </div>

      {/* ④ 剪除成绩单（M21）：只统计你亲手剪掉的，本地 SQLite append-only。
          奖章位 + 大数字 + Top3 迷你条——一行读完，不与上面几段抢纵向空间。 */}
      {cut ? (
        <div
          className="shrink-0 rounded-card border border-hairline-soft bg-gradient-to-b from-chip-soft to-transparent bg-surface-raised px-4 py-2.5"
          title={t("stats.scorecard.title")}
        >
          <div className="flex items-center gap-4">
            <span
              className="grid size-10 shrink-0 place-items-center rounded-[12px] text-accent"
              style={{
                background: "color-mix(in srgb, var(--color-accent) 13%, transparent)",
                boxShadow: "0 0 14px color-mix(in srgb, var(--color-accent) 20%, transparent)",
              }}
              aria-hidden
            >
              <ScissorsIcon size={17} />
            </span>
            <div className="min-w-0 flex-1">
              {cut.totalItems > 0 ? (
                <>
                  <div className="flex items-baseline gap-2.5">
                    <CountUp
                      value={cut.totalBytes}
                      format={formatBytes}
                      className="font-mono text-[22px] leading-none font-semibold tabular-nums text-ink"
                    />
                    <span className="truncate text-[10px] text-ink-faint">
                      {cutEquivalence(cut.totalBytes, t)}
                    </span>
                  </div>
                  <p className="mt-0.5 truncate text-[9.5px] text-ink-faint">
                    {/* 口径：cut_log 一行 = 一个被剪掉的条目，所以这里是「项」不是「次」 */}
                    {t("stats.scorecard.items", { n: cut.totalItems })}
                    {cut.firstTs != null
                      ? t("stats.scorecard.since", { date: dayString(new Date(cut.firstTs * 1000)) })
                      : ""}
                    {" · "}
                    {t("stats.scorecard.hint")}
                  </p>
                </>
              ) : (
                <div className="flex items-center gap-2.5">
                  <p className="text-[10.5px] leading-relaxed text-ink-faint">{t("stats.scorecard.empty")}</p>
                  <Btn variant="ghost" onClick={() => onDrill("disk")}>
                    {t("stats.scorecard.goStorage")}
                  </Btn>
                </div>
              )}
            </div>
            {cut.totalItems > 0 && cut.top.length > 0 ? (
              <div className="flex w-[236px] shrink-0 flex-col gap-1.5 border-l border-hairline-soft pl-4">
                {cut.top.slice(0, 3).map((item) => {
                  const topBytes = cut.top[0]?.bytes ?? 0;
                  return (
                    <div key={item.label} className="min-w-0">
                      <div className="flex items-baseline justify-between gap-2">
                        <span className="min-w-0 truncate text-[10px] text-ink-dim">{item.label}</span>
                        <span className="shrink-0 font-mono text-[10.5px] tabular-nums text-ink">
                          {formatBytes(item.bytes)}
                        </span>
                      </div>
                      <div className="mt-0.5 h-[3px] overflow-hidden rounded-full bg-chip">
                        <div
                          className="h-full rounded-full bg-accent opacity-75"
                          style={{ width: `${topBytes > 0 ? (item.bytes / topBytes) * 100 : 0}%` }}
                        />
                      </div>
                    </div>
                  );
                })}
              </div>
            ) : null}
          </div>
        </div>
      ) : null}

      <p className="shrink-0 text-[9px] leading-relaxed text-ink-faint">
        {t("common.hint.statsPrinciple")}
      </p>
    </div>
  );
}

/** 四档里最大的一档：图例色条的比例基准 */
function tierMax(totals: { deadweight: number; l2: number; l3: number; l4: number }): number {
  return Math.max(totals.deadweight, totals.l2, totals.l3, totals.l4);
}

/**
 * Hero 图例行（eul 式）：L 徽标 + 比例色条 + 数值——完整档位名在 tooltip。
 * L4 用斜纹填充，沿用「L4 不计入」的视觉约定。
 */
function TierRow({
  lv,
  label,
  value,
  bytes,
  max,
  color,
  hatch = false,
  dim = false,
}: {
  lv: string;
  label: string;
  value: string;
  bytes: number;
  max: number;
  color: string;
  hatch?: boolean;
  dim?: boolean;
}) {
  const share = max > 0 ? Math.max(0.02, bytes / max) : 0;
  return (
    <span
      className="grid grid-cols-[9px_18px_minmax(0,150px)_auto] items-center gap-2"
      title={label}
      style={dim ? { opacity: 0.55 } : undefined}
    >
      <i
        className="size-[9px] shrink-0 rounded-[3px]"
        style={{
          background: hatch
            ? "repeating-linear-gradient(45deg, var(--color-hatch) 0 2px, transparent 2px 4px)"
            : color,
          boxShadow: hatch ? undefined : `0 0 6px color-mix(in srgb, ${color} 35%, transparent)`,
        }}
        aria-hidden
      />
      <span className="font-mono text-[9px] leading-none text-ink-faint">{lv}</span>
      <span className="h-[4px] overflow-hidden rounded-full bg-chip">
        <i
          className="block h-full rounded-full"
          style={{
            width: `${share * 100}%`,
            background: hatch
              ? "repeating-linear-gradient(45deg, var(--color-hatch) 0 2px, transparent 2px 4px)"
              : `linear-gradient(90deg, color-mix(in srgb, ${color} 55%, transparent), color-mix(in srgb, ${color} 18%, transparent))`,
          }}
          aria-hidden
        />
      </span>
      <span className="shrink-0 text-right font-mono text-[13px] font-medium tabular-nums text-ink">
        {value}
      </span>
    </span>
  );
}

/** 设备磁贴微图表：局域网设备点阵（亮=在线 / 弱=离线）+ 温度读数 */
function DeviceViz({
  devices,
  tempCelsius,
}: {
  devices: { online: boolean }[] | null;
  tempCelsius: number | null;
}) {
  if (devices == null && tempCelsius == null) return null;
  return (
    <div className="flex items-center gap-1.5" aria-hidden>
      {(devices ?? []).slice(0, 8).map((d, i) => (
        <i
          key={i}
          className="size-[6px] rounded-full"
          style={{
            background: "var(--color-domain-device)",
            opacity: d.online ? 1 : 0.4,
          }}
        />
      ))}
      {devices != null && devices.length > 8 ? (
        <span className="font-mono text-[9px] tabular-nums text-ink-faint">+{devices.length - 8}</span>
      ) : null}
      {tempCelsius != null ? (
        <span className="ml-auto font-mono text-[9px] tabular-nums text-ink-faint">
          {Math.round(tempCelsius)} ℃
        </span>
      ) : null}
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
