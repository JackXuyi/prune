import {
  cpuReadout,
  diskReadout,
  memoryReadout,
  netDirectionReadout,
  netReadout,
  portReadout,
} from "../lib/statsReadout";
import { dayString } from "../lib/format";
import { UI_TEXT } from "../lib/uiText";
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

  const mem = memoryReadout(snapshot);
  const cpu = cpuReadout(snapshot?.cpu);
  const disk = diskReadout(volume, report?.totals.reclaimable ?? null, known);
  const port = portReadout(ports);
  const net = netReadout(netSample, netHistory);
  // 方向读数（M16 v6）：下载 / 上传各一张卡，口径在 readout 层派生
  const netDown = netDirectionReadout(netSample, netHistory, "down");
  const netUp = netDirectionReadout(netSample, netHistory, "up");

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
          <h1 className="text-[15px] font-semibold tracking-tight">总览</h1>
          <p className="mt-0.5 text-[11px] text-ink-faint">
            可回收优先 · 点击卡片看明细 · 只读，不在这里做任何清理
          </p>
        </div>
        <span className="text-[10px] text-ink-faint">{UI_TEXT.statsCadence}</span>
      </div>

      {/* Hero：可回收环形仪表（GB 可行动，百分比可描述） */}
      <button
        type="button"
        onClick={() => onDrill("disk")}
        title="点击进入「存储」查看分级明细与清理"
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
              ? `可回收 ${formatBytes(totals.reclaimable)}：L1 ${formatBytes(totals.deadweight)}，L2 ${formatBytes(totals.l2)}，L3 ${formatBytes(totals.l3)}，L4 ${formatBytes(totals.l4)} 不计入`
              : "可回收空间尚未扫描"
          }
        >
          {totals ? (
            <span className="flex flex-col items-center">
              <CountUp
                value={totals.reclaimable}
                format={formatBytes}
                className="font-mono text-[24px] leading-none tabular-nums text-ink"
              />
              <span className="mt-1 text-[9.5px] text-ink-faint">可回收</span>
            </span>
          ) : (
            <span className="font-mono text-[20px] text-ink-faint">—</span>
          )}
        </RingGauge>

        <div className="min-w-0 flex-1">
          <div className="flex items-baseline justify-between gap-2">
            <span className="text-[11px] font-medium tracking-wide text-accent">可回收空间构成</span>
            <span className="text-[10px] text-ink-faint transition-colors group-hover:text-ink-dim">
              → 存储
            </span>
          </div>
          {totals ? (
            <>
              <div className="mt-2.5 grid grid-cols-2 gap-x-6 gap-y-1.5">
                <LegendDot color="var(--color-accent)" label="L1 跨平台二进制" value={formatBytes(totals.deadweight)} />
                <LegendDot color="var(--color-info)" label="L2 包管理器等缓存" value={formatBytes(totals.l2)} />
                <LegendDot color="var(--color-warn)" label="L3 项目依赖 / 模拟器" value={formatBytes(totals.l3)} />
                <LegendDot color="var(--color-danger)" label="L4 Docker · 只给命令不计入" value={formatBytes(totals.l4)} dim />
              </div>
            </>
          ) : (
            <div className="mt-2.5 flex items-center gap-3">
              <p className="text-[11px] leading-relaxed text-ink-faint">
                {diskScanning ? UI_TEXT.scanning : UI_TEXT.firstScanHint}
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
                  {UI_TEXT.scanNow}
                </span>
              ) : null}
            </div>
          )}
        </div>
      </button>

      {/* 六张域卡：图标 + 大数字 + 微图表，口径 hover */}
      <div className="mt-3 grid grid-cols-2 gap-3">
        <DomainCard
          title="内存"
          caption="physicalUsed / 总量 · 含被压缩与换出的部分（footprint 口径）"
          color="var(--color-info)"
          icon={<MemoryCloudIcon size={14} />}
          readout={mem}
          onOpen={() => onDrill("memory")}
          chart={
            swap && swapRatio != null ? (
              <div>
                <div className="flex items-baseline justify-between text-[10px] text-ink-faint">
                  <span>
                    swap {formatBytes(swap.used)} / {formatBytes(swap.total)}
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
                  label={`swap 已用 ${formatPercent(swapRatio)}，阈值刻度 75 与 90`}
                />
              </div>
            ) : null
          }
        />
        <DomainCard
          title="CPU"
          caption="整机每核平均（0–100%）· 与进程的单核口径不同，不能相减"
          color="var(--color-domain-cpu)"
          icon={<ChipIcon size={14} />}
          readout={cpu}
          onOpen={() => onDrill("resources")}
          chart={
            <div>
              <div className="flex items-baseline justify-between text-[10px] text-ink-faint">
                <span>近 10 min · 10 s 一个点</span>
              </div>
              <AreaChart
                className="mt-1"
                values={cpuHistory}
                color="var(--color-domain-cpu)"
                height={26}
                ariaLabel="CPU 近 10 分钟走势"
              />
            </div>
          }
        />
        <DomainCard
          title="硬盘"
          caption="home 所在卷 · 已用 = 总量 − 用户可用（statvfs 粗略口径）"
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
          title="端口"
          caption="去重后的监听端口 · 琥珀=对外暴露 / 绿=关注层 / 灰=其他，悬停看「端口 · 进程」"
          color="var(--color-warn)"
          icon={<PortJackIcon size={14} />}
          readout={port}
          onOpen={() => onDrill("ports")}
          chart={
            ports ? (
              <DotMatrix
                items={portDots}
                dot={9}
                ariaLabel={`监听端口点阵：共 ${portEntries.length} 个`}
              />
            ) : null
          }
        />
        {/* 网络拆成两卡（M16 v6）：行数不变（3 行），574/592 高度算式原样成立 */}
        <DomainCard
          title="下载"
          caption="物理网卡合计 · 虚拟接口不计入，VPN 不会双计"
          color="var(--color-domain-net)"
          icon={<DownloadIcon size={14} />}
          readout={net}
          body={<NetDirectionReading readout={netDown} />}
          onOpen={() => onDrill("net")}
        />
        <DomainCard
          title="上传"
          caption="物理网卡合计 · 与下载各自刻度，量级常差 10 倍"
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
            <span className="text-[11px] font-medium tracking-wide text-ink-dim">剪除成绩单</span>
            <span className="text-[10px] text-ink-faint">只记录你亲手剪掉的 · 本地留存两年</span>
          </div>
          {cut.totalItems > 0 ? (
            <>
              <div className="mt-2 flex items-baseline gap-3">
                <CountUp
                  value={cut.totalBytes}
                  format={formatBytes}
                  className="font-mono text-[22px] leading-none tabular-nums text-ink"
                />
                <span className="text-[11px] text-ink-faint">{cutEquivalence(cut.totalBytes)}</span>
              </div>
              <p className="mt-1 text-[10px] text-ink-faint">
                {cut.totalItems} 次清理
                {cut.firstTs != null ? ` · 自 ${dayString(new Date(cut.firstTs * 1000))} 起` : ""}
              </p>
              {cut.top.length > 0 && (
                <div className="mt-2.5 grid gap-1">
                  {cut.top.slice(0, 3).map((t, i) => (
                    <div key={t.label} className="flex items-baseline gap-2 text-[11px]">
                      <span className="w-3 shrink-0 font-mono text-ink-faint">{i + 1}</span>
                      <span className="truncate text-ink-dim">{t.label}</span>
                      <span className="ml-auto shrink-0 font-mono tabular-nums text-ink">
                        {formatBytes(t.bytes)}
                      </span>
                      <span className="w-9 shrink-0 text-right text-[10px] text-ink-faint">
                        {t.count} 次
                      </span>
                    </div>
                  ))}
                </div>
              )}
            </>
          ) : (
            <div className="mt-2 flex items-center justify-between gap-3">
              <p className="text-[11px] text-ink-faint">
                还没有剪除记录 —— 去存储页挑一项缓存，完成第一次清理。
              </p>
              <button
                type="button"
                onClick={() => onDrill("disk")}
                className="shrink-0 rounded-md bg-white/[0.06] px-2.5 py-1 text-[11px] text-ink-dim transition-colors hover:bg-white/[0.1] hover:text-ink"
              >
                去存储
              </button>
            </div>
          )}
        </div>
      )}

      <p className="mt-4 text-[10px] leading-relaxed text-ink-faint">{UI_TEXT.statsPrinciple}</p>
    </div>
  );
}

/** 换算彩蛋：≥4 GB 用「高清电影」（4 GB/部），否则用「照片」（3 MB/张） */
function cutEquivalence(totalBytes: number): string {
  const MOVIE = 4e9;
  const PHOTO = 3e6;
  if (totalBytes >= MOVIE) {
    return `相当于 ${Math.max(1, Math.round(totalBytes / MOVIE))} 部高清电影`;
  }
  return `相当于 ${Math.max(1, Math.round(totalBytes / PHOTO))} 张照片`;
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
