import { useEffect, useState } from "react";
import type { ReactNode } from "react";

import { devVolume } from "../lib/devFixture";
import { diskVolume, isTauri } from "../lib/ipc";
import { formatBytes, formatCpu, formatPercent, splitBytes } from "../lib/format";
import { useDiskStore } from "../stores/diskStore";
import { useMemStore } from "../stores/memStore";
import { usePortStore } from "../stores/portStore";
import type { VolumeInfo } from "../types/disk";
import type { CpuInfo, MemSnapshot } from "../types/mem";
import type { PortSnapshot } from "../types/net";
import { ChevronRightIcon } from "./icons";
import { Meter, PressureDot } from "./primitives";
import { Sparkline } from "./Sparkline";

/** 下钻目标：统计卡只导航，不做任何清理/结束动作（方案 §4） */
export type DrillTab = "overview" | "disk" | "ports";

/**
 * 统计 tab（M9）：系统级总览，作为默认页。
 *
 * 四个域各一张卡，每张卡「主读数 + 占比条 + 次级读数」：
 * - 内存 / CPU：读 memStore（常采 10 s 推送，零额外开销）
 * - 硬盘：读 `disk_volume`（statvfs 直读，不依赖全盘扫描）
 * - 端口：读 portStore（首次进入扫一轮，不挂定时轮）
 *
 * 假事实红线（方案 §6）：没扫描/读不到一律「尚未扫描 / 不可读」，绝不填 0。
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
  const portLoading = usePortStore((s) => s.loading);
  const report = useDiskStore((s) => s.report);
  const [volume, setVolume] = useState<VolumeInfo | null>(null);
  const [volumeKnown, setVolumeKnown] = useState(false);

  // 卷容量：进入 tab、点刷新时各取一次（statvfs 微秒级，无需缓存）
  useEffect(() => {
    let cancelled = false;
    const load = async () => {
      try {
        const v = !isTauri() ? (import.meta.env.DEV ? devVolume() : null) : await diskVolume();
        if (!cancelled) {
          setVolume(v);
          setVolumeKnown(true);
        }
      } catch {
        // statvfs 失败：保持「容量不可读」，不猜
        if (!cancelled) setVolumeKnown(true);
      }
    };
    void load();
    return () => {
      cancelled = true;
    };
  }, [refreshNonce]);

  return (
    <div className="min-h-0 flex-1 overflow-y-auto px-5 py-4">
      <div className="flex items-baseline justify-between">
        <div>
          <h1 className="text-[15px] font-semibold tracking-tight">本机资源总览</h1>
          <p className="mt-0.5 text-[11px] text-ink-faint">
            系统级读数 · 点击卡片看明细 · 只读，不在这里做任何清理
          </p>
        </div>
        <span className="text-[10px] text-ink-faint">每 10 s 随内存常采更新</span>
      </div>

      <div className="mt-4 grid grid-cols-2 gap-3">
        <MemoryCard snapshot={snapshot} onOpen={() => onDrill("overview")} />
        <CpuCard cpu={snapshot?.cpu ?? null} />
        <DiskCard
          volume={volume}
          known={volumeKnown}
          reclaimable={report?.totals.reclaimable ?? null}
          onOpen={() => onDrill("disk")}
        />
        <PortCard ports={ports} loading={portLoading} onOpen={() => onDrill("ports")} />
      </div>

      <p className="mt-4 text-[10px] leading-relaxed text-ink-faint">
        统计只呈现事实读数与口径。清理入口在「存储」tab（走白名单与确认），
        结束进程在进程明细上——统计页不放破坏性按钮。
      </p>
    </div>
  );
}

/** 卡片外壳：`onOpen` 存在时整卡可点（只做导航） */
function Card({
  title,
  caption,
  onOpen,
  children,
}: {
  title: string;
  caption: string;
  onOpen?: () => void;
  children: ReactNode;
}) {
  const body = (
    <>
      <div className="flex items-baseline justify-between gap-2">
        <span className="text-[11px] font-medium tracking-wide text-ink-dim">{title}</span>
        {onOpen ? (
          <ChevronRightIcon
            size={12}
            className="shrink-0 text-ink-faint transition-transform group-hover:translate-x-0.5"
          />
        ) : null}
      </div>
      <span className="mt-0.5 block text-[10px] leading-snug text-ink-faint">{caption}</span>
      <div className="mt-3">{children}</div>
    </>
  );

  const shell =
    "flex min-h-[152px] flex-col rounded-xl border border-hairline-soft bg-white/[0.02] px-4 py-3.5";
  if (!onOpen) return <div className={shell}>{body}</div>;
  return (
    <button
      type="button"
      onClick={onOpen}
      title={`查看${title}明细`}
      className={`group text-left transition-colors hover:border-hairline hover:bg-white/[0.035] ${shell}`}
    >
      {body}
    </button>
  );
}

/** 主读数 + 副读数共用的排版：大号等宽数字，单位小一号 */
function Reading({ value, unit, suffix }: { value: string; unit?: string; suffix?: string }) {
  return (
    <div className="flex items-baseline gap-1.5">
      <span className="font-mono text-[24px] leading-none tabular-nums text-ink">{value}</span>
      {unit ? <span className="text-[11px] text-ink-faint">{unit}</span> : null}
      {suffix ? <span className="text-[11px] text-ink-dim">{suffix}</span> : null}
    </div>
  );
}

/** 占比条 tone：满/紧张才变色，正常占用不要制造焦虑 */
function loadTone(ratio: number): "accent" | "warn" | "danger" {
  if (ratio >= 0.9) return "danger";
  if (ratio >= 0.75) return "warn";
  return "accent";
}

function MemoryCard({ snapshot, onOpen }: { snapshot: MemSnapshot | null; onOpen: () => void }) {
  const used = snapshot?.physicalUsedBytes;
  const total = snapshot?.physicalTotal ?? 0;
  const ratio = used != null && total > 0 ? used / total : null;
  const swapRatio =
    snapshot && snapshot.swap.total > 0 ? snapshot.swap.used / snapshot.swap.total : null;
  const usedPart = used != null ? splitBytes(used) : null;
  const totalPart = total > 0 ? splitBytes(total) : null;

  return (
    <Card
      title="内存"
      caption="physicalUsed / 总量 · 含被压缩与换出的部分"
      onOpen={onOpen}
    >
      {usedPart && totalPart ? (
        <>
          <div className="flex items-baseline justify-between gap-2">
            <Reading value={usedPart.value} unit={usedPart.unit} />
            <span className="font-mono text-[12px] tabular-nums text-ink-dim">
              / {totalPart.value} {totalPart.unit}
            </span>
          </div>
          <div className="mt-2.5">
            <Meter ratio={ratio ?? 0} tone={loadTone(ratio ?? 0)} />
          </div>
          <div className="mt-2 flex flex-wrap items-center gap-x-3 gap-y-1 text-[10px] text-ink-faint">
            <span className="font-mono tabular-nums text-ink-dim">{formatPercent(ratio)}</span>
            <span>
              可用 <span className="font-mono tabular-nums text-ink-dim">{formatBytes(snapshot?.freeBytes ?? 0)}</span>
            </span>
            {swapRatio != null ? (
              <span className={swapRatio >= 0.75 ? "text-warn" : undefined}>
                swap <span className="font-mono tabular-nums">{formatPercent(swapRatio)}</span>
              </span>
            ) : null}
            {snapshot?.pressure ? <PressureDot level={snapshot.pressure.level} /> : null}
          </div>
        </>
      ) : (
        <p className="text-[11px] text-ink-faint">正在采样…</p>
      )}
    </Card>
  );
}

function CpuCard({ cpu }: { cpu: CpuInfo | null }) {
  const usage = cpu?.usage ?? null;
  const ratio = usage != null ? usage / 100 : null;

  return (
    <Card
      title="CPU"
      caption="整机每核平均（0–100%）· 与进程的单核口径不同，不能相减"
    >
      {cpu && usage != null ? (
        <>
          <div className="flex items-baseline justify-between gap-2">
            <Reading value={formatCpu(usage)} />
            <span className="text-[10px] text-ink-faint">{cpu.cores} 核</span>
          </div>
          <div className="mt-2.5">
            <Meter ratio={ratio ?? 0} tone={loadTone(ratio ?? 0)} />
          </div>
          <div className="mt-2 flex items-center justify-between gap-2 text-[10px] text-ink-faint">
            <span>
              {cpu.load
                ? `load ${cpu.load.one.toFixed(2)} / ${cpu.load.five.toFixed(2)} / ${cpu.load.fifteen.toFixed(2)}`
                : "该平台无 load 口径"}
            </span>
            <span>近 10 min</span>
          </div>
          <div className="mt-1.5 h-8">
            {cpu.history.length >= 2 ? (
              <Sparkline values={cpu.history} height={32} />
            ) : (
              <p className="text-[10px] text-ink-faint">走势积累中（每 10 s 一个点）</p>
            )}
          </div>
        </>
      ) : (
        <p className="text-[11px] leading-relaxed text-ink-faint">
          采样中…（需要两轮间隔才能算出占比，约 10 s）
        </p>
      )}
    </Card>
  );
}

function DiskCard({
  volume,
  known,
  reclaimable,
  onOpen,
}: {
  volume: VolumeInfo | null;
  known: boolean;
  reclaimable: number | null;
  onOpen: () => void;
}) {
  const total = volume?.totalBytes ?? 0;
  const used = volume ? Math.max(0, volume.totalBytes - volume.userAvailableBytes) : null;
  const ratio = used != null && total > 0 ? used / total : null;
  const usedPart = used != null ? splitBytes(used) : null;
  const totalPart = total > 0 ? splitBytes(total) : null;

  return (
    <Card
      title="硬盘"
      caption="home 所在卷 · 已用 = 总量 − 用户可用（statvfs 粗略口径）"
      onOpen={onOpen}
    >
      {usedPart && totalPart ? (
        <>
          <div className="flex items-baseline justify-between gap-2">
            <Reading value={usedPart.value} unit={usedPart.unit} />
            <span className="font-mono text-[12px] tabular-nums text-ink-dim">
              / {totalPart.value} {totalPart.unit}
            </span>
          </div>
          <div className="mt-2.5">
            <Meter ratio={ratio ?? 0} tone={loadTone(ratio ?? 0)} />
          </div>
          <div className="mt-2 flex flex-wrap items-center gap-x-3 gap-y-1 text-[10px] text-ink-faint">
            <span className="font-mono tabular-nums text-ink-dim">{formatPercent(ratio)}</span>
            <span>
              可用{" "}
              <span className="font-mono tabular-nums text-ink-dim">
                {formatBytes(volume?.userAvailableBytes ?? 0)}
              </span>
            </span>
            {reclaimable != null ? (
              <span title="来自最近一次存储扫描（L1+L2+L3），不是可保证释放的空间">
                可回收约{" "}
                <span className="font-mono tabular-nums text-accent">
                  {formatBytes(reclaimable)}
                </span>
              </span>
            ) : null}
          </div>
        </>
      ) : (
        <p className="text-[11px] leading-relaxed text-ink-faint">
          {known ? "容量不可读（statvfs 失败）" : "读取中…"}
        </p>
      )}
    </Card>
  );
}

function PortCard({
  ports,
  loading,
  onOpen,
}: {
  ports: PortSnapshot | null;
  loading: boolean;
  onOpen: () => void;
}) {
  const entries = ports?.entries ?? [];
  const watched = entries.filter((e) => e.watched).length;
  const exposed = entries.filter((e) => e.exposure === "any").length;
  const conns = Object.values(ports?.connsByPid ?? {}).reduce(
    (sum, c) => sum + c.established,
    0,
  );
  const closeWait = Object.values(ports?.connsByPid ?? {}).reduce(
    (sum, c) => sum + c.closeWait,
    0,
  );

  return (
    <Card title="端口" caption="去重后的监听端口 · 关注层口径见端口 tab" onOpen={onOpen}>
      {ports ? (
        <>
          <div className="flex items-baseline justify-between gap-2">
            <Reading value={String(entries.length)} unit="个监听端口" />
            <span className="text-[10px] text-ink-faint">
              关注层 <span className="font-mono tabular-nums text-ink-dim">{watched}</span>
            </span>
          </div>
          <div className="mt-2 flex flex-wrap items-center gap-x-3 gap-y-1 text-[10px] text-ink-faint">
            <span className={exposed > 0 ? "text-warn" : undefined}>
              对外暴露 <span className="font-mono tabular-nums">{exposed}</span>
            </span>
            <span>
              活跃连接{" "}
              <span className="font-mono tabular-nums text-ink-dim">{conns}</span>
            </span>
            {closeWait > 0 ? (
              <span className="text-warn" title="对端已关、本端未释放：可能是泄漏，也可能只是慢关闭">
                CLOSE_WAIT {closeWait}
              </span>
            ) : null}
          </div>
          {ports.coverage.note ? (
            <p className="mt-2 text-[10px] leading-snug text-warn">{ports.coverage.note}</p>
          ) : null}
          <p className="mt-2 text-[10px] text-ink-faint">
            连接多 ≠ 流量大：本机没有 per-process 字节计数口径
          </p>
        </>
      ) : (
        <p className="text-[11px] leading-relaxed text-ink-faint">
          {loading ? "扫描中…" : "尚未扫描：进入本 tab 会自动扫一轮，也可点右上角刷新"}
        </p>
      )}
    </Card>
  );
}
