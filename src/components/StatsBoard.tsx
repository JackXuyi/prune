import {
  cpuReadout,
  diskReadout,
  memoryReadout,
  netDirectionReadout,
  netReadout,
  portReadout,
} from "../lib/statsReadout";
import { UI_TEXT } from "../lib/uiText";
import { useVolumeInfo } from "../lib/useVolumeInfo";
import { MeterBar } from "./charts/MeterBar";
import { StackBar } from "./charts/StackBar";
import { useDiskStore } from "../stores/diskStore";
import { useMemStore } from "../stores/memStore";
import { useNetTotalStore } from "../stores/netTotalStore";
import { usePortStore } from "../stores/portStore";
import { formatBytes, formatPercent } from "../lib/format";
import { NetDirectionReading } from "./stats/NetReading";
import { StatsCard } from "./stats/StatsCard";

/**
 * 下钻目标：总览卡只导航，不做任何清理/结束动作（M9 §4，UX v3 §6.1）。
 * v3.1 起 CPU 卡下钻到「应用」页（按 CPU 排序），内存卡到合并后的「内存」页。
 */
export type DrillTab = "stats" | "resources" | "memory" | "disk" | "ports" | "net";

/**
 * 总览页（UX v3 §6.1，原「统计」tab）。
 *
 * v3.1 两处升级：
 * 1. **可回收主卡**（回归 README 产品主张「首屏主数字是可回收 GB」）：L1–L4
 *    StackBar 构成条 + 分段图例；未扫描时「尚未扫描 + 立即扫描」，绝不显示 0。
 * 2. **数字图表化**（§7.6）：swap 阈值条（75/90 刻度）、端口分组堆叠条——
 *    全部由现有快照前端派生，零新增 IPC、零新增采样。
 *
 * 读数一律来自 `lib/statsReadout.ts`（与菜单栏面板同源）；
 * 假事实红线（M9 §8）：没扫描/读不到一律「尚未扫描 / 不可读」，绝不填 0。
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

  const mem = memoryReadout(snapshot);
  const cpu = cpuReadout(snapshot?.cpu);
  const disk = diskReadout(volume, report?.totals.reclaimable ?? null, known);
  const port = portReadout(ports);
  const net = netReadout(netSample, netHistory);
  // 方向读数（M16 v6）：下载 / 上传各一张卡，口径在 readout 层派生
  const netDown = netDirectionReadout(netSample, netHistory, "down");
  const netUp = netDirectionReadout(netSample, netHistory, "up");

  const totals = report?.totals ?? null;

  // 端口分组（总览卡脚注的 StackBar）：三段互斥——「对外暴露」可能同时命中
  // 关注层，先扣掉再算其他，否则三段之和大于端口总数、比例虚增
  const portEntries = ports?.entries ?? [];
  const watchedCount = portEntries.filter((e) => e.watched && e.exposure !== "any").length;
  const exposedCount = portEntries.filter((e) => e.exposure === "any").length;
  const otherCount = Math.max(0, portEntries.length - watchedCount - exposedCount);

  // swap 阈值条（总览内存卡脚注）：读不到就不渲染，绝不画 0
  const swap = snapshot?.swap;
  const swapRatio = swap && swap.total > 0 ? swap.used / swap.total : null;

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

      {/* 可回收主卡（UX v3 §6.1）：GB 可行动，百分比可描述 */}
      <button
        type="button"
        onClick={() => onDrill("disk")}
        title="点击进入「存储」查看分级明细与清理"
        className="group mt-4 block w-full rounded-xl border border-accent/30 bg-accent/[0.06] px-4 py-3.5 text-left transition-colors hover:border-accent/50 hover:bg-accent/[0.09]"
      >
        <div className="flex items-baseline justify-between gap-2">
          <span className="text-[11px] font-medium tracking-wide text-accent">可回收</span>
          <span className="text-[10px] text-ink-faint transition-colors group-hover:text-ink-dim">
            → 存储
          </span>
        </div>
        {totals ? (
          <>
            <div className="mt-1 flex items-baseline gap-1">
              <span className="font-mono text-[26px] leading-none tabular-nums text-ink">
                {formatBytes(totals.reclaimable)}
              </span>
            </div>
            <StackBar
              className="mt-2.5"
              ariaLabel={`可回收构成：L1 ${formatBytes(totals.deadweight)}，L2 ${formatBytes(totals.l2)}，L3 ${formatBytes(totals.l3)}，L4 ${formatBytes(totals.l4)} 不计入`}
              segments={[
                { key: "l1", label: "L1 跨平台二进制", value: totals.deadweight, tone: "accent" },
                { key: "l2", label: "L2 包管理器等缓存", value: totals.l2, tone: "info" },
                { key: "l3", label: "L3 项目依赖 / 模拟器", value: totals.l3, tone: "neutral" },
                { key: "l4", label: "L4 Docker 等高风险项", value: totals.l4, muted: true },
              ]}
            />
            <p className="mt-1.5 text-[10px] text-ink-faint">
              L1 {formatBytes(totals.deadweight)} · L2 {formatBytes(totals.l2)} · L3{" "}
              {formatBytes(totals.l3)} · L4 {formatBytes(totals.l4)} 只给命令，不计入
            </p>
          </>
        ) : (
          <div className="mt-2 flex items-center gap-3">
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
      </button>

      <div className="mt-3 grid grid-cols-2 gap-3">
        <StatsCard
          title="内存"
          caption="physicalUsed / 总量 · 含被压缩与换出的部分"
          readout={mem}
          onOpen={() => onDrill("memory")}
          footnote={
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
        <StatsCard
          title="CPU"
          caption="整机每核平均（0–100%）· 与进程的单核口径不同，不能相减"
          readout={cpu}
          onOpen={() => onDrill("resources")}
        />
        <StatsCard
          title="硬盘"
          caption="home 所在卷 · 已用 = 总量 − 用户可用（statvfs 粗略口径）"
          readout={disk}
          onOpen={() => onDrill("disk")}
        />
        <StatsCard
          title="端口"
          caption="去重后的监听端口 · 关注层口径见端口页"
          readout={port}
          onOpen={() => onDrill("ports")}
          footnote={
            ports ? (
              <StackBar
                height={4}
                ariaLabel={`端口分组：关注层 ${watchedCount}，对外 ${exposedCount}，其他 ${otherCount}`}
                segments={[
                  { key: "w", label: "关注层", value: watchedCount, tone: "accent" },
                  { key: "e", label: "对外暴露", value: exposedCount, tone: "warn" },
                  { key: "o", label: "其他", value: otherCount, tone: "neutral" },
                ]}
              />
            ) : null
          }
        />
        {/* 网络拆成两卡（M16 v6）：行数不变（3 行），574/592 高度算式原样成立 */}
        <StatsCard
          title="下载"
          caption="物理网卡合计 · 虚拟接口不计入，VPN 不会双计"
          readout={net}
          body={<NetDirectionReading readout={netDown} />}
          onOpen={() => onDrill("net")}
        />
        <StatsCard
          title="上传"
          caption="物理网卡合计 · 与下载各自刻度，量级常差 10 倍"
          readout={net}
          body={<NetDirectionReading readout={netUp} />}
          onOpen={() => onDrill("net")}
        />
      </div>

      <p className="mt-4 text-[10px] leading-relaxed text-ink-faint">{UI_TEXT.statsPrinciple}</p>
    </div>
  );
}
