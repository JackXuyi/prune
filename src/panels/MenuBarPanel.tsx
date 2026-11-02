import { useState, type ReactNode } from "react";

import { AppMemoryList, AppMemorySummary } from "../components/AppMemoryList";
import { LeakBanner } from "../components/LeakCard";
import { formatBytes, formatPercent, formatRate, formatRelative, numOf, unitOf } from "../lib/format";
import { openMain } from "../lib/ipc";
import { useDiskStore } from "../stores/diskStore";
import { useMemStore } from "../stores/memStore";
import { swappedRatio, type FamilyMem, type MemSnapshot, type ProcMem } from "../types/mem";
import { ChevronRightIcon, ExpandIcon, RefreshIcon, ScissorsIcon } from "../components/icons";
import { Badge, Meter, PressureDot } from "../components/primitives";
import { Tabs } from "../components/Tabs";

type PanelTab = "apps" | "overview";

const EMPTY_FAMILIES: FamilyMem[] = [];
const EMPTY_PROCESSES: ProcMem[] = [];

/** 菜单栏面板（380×560）：应用内存 / 概况 两个 tab */
export function MenuBarPanel() {
  const [tab, setTab] = useState<PanelTab>("apps");
  const snapshot = useMemStore((s) => s.snapshot);
  const loading = useMemStore((s) => s.loading);
  const refreshing = useMemStore((s) => s.refreshing);
  const error = useMemStore((s) => s.error);
  const refresh = useMemStore((s) => s.refresh);

  return (
    <div className="flex h-screen flex-col p-2">
      <div className="flex min-h-0 flex-1 flex-col overflow-hidden rounded-[14px] border border-white/[0.07] bg-[#0f1216]/95 shadow-[0_18px_50px_-12px_rgba(0,0,0,0.8)] backdrop-blur-2xl">
        <header className="flex h-11 shrink-0 items-center gap-2 px-3.5">
          <ScissorsIcon size={14} className="text-accent" />
          <span className="text-[13px] font-semibold tracking-tight text-ink">Prune</span>
          {snapshot ? (
            snapshot.deepAvailable ? (
              <Badge tone="accent" title="footprint 口径：含被换出到磁盘的部分">
                footprint
              </Badge>
            ) : (
              <Badge tone="warn" title="footprint 不可用，当前全部为 RSS 口径，不含换出量">
                RSS 口径
              </Badge>
            )
          ) : null}
          <div className="flex-1" />
          <IconButton label="立即刷新" onClick={() => void refresh()}>
            <RefreshIcon size={13} className={refreshing ? "animate-spin" : ""} />
          </IconButton>
          <IconButton label="打开主窗口" onClick={() => void openMain()}>
            <ExpandIcon size={13} />
          </IconButton>
        </header>

        <DomainHero />

        <div className="flex h-9 shrink-0 items-center justify-between px-3.5">
          <Tabs
            size="sm"
            value={tab}
            onChange={setTab}
            items={[
              { id: "apps", label: "应用", hint: "每个应用的绝对内存占用" },
              { id: "overview", label: "概况", hint: "内存压力与 swap" },
            ]}
          />
          {snapshot ? (
            tab === "overview" ? (
              <span className="text-[10px] text-ink-faint">
                swap {formatPercent(snapshot.swap.total > 0 ? snapshot.swap.used / snapshot.swap.total : 0)}
              </span>
            ) : (
              <AppMemorySummary families={snapshot.families} />
            )
          ) : null}
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto px-3.5 pb-3">
          {error ? (
            <div className="rounded-lg bg-danger/[0.08] p-3 text-[11px] leading-relaxed text-danger">
              {error}
            </div>
          ) : !snapshot ? (
            <Skeleton loading={loading} />
          ) : tab === "apps" ? (
            <AppsSection />
          ) : (
            <MemSection />
          )}
        </div>

        <footer className="flex shrink-0 items-center justify-between px-3.5 pb-2.5 pt-1 text-[10px] text-ink-faint/80">
          <span>上次深探 {formatRelative(snapshot?.deepUpdatedAt ?? null)}</span>
          <span>每 10 秒刷新</span>
        </footer>
      </div>
    </div>
  );
}

/** tab 1：每个应用占用的绝对内存（含换出，按总量降序） */
function AppsSection() {
  const families = useMemStore((s) => s.snapshot?.families ?? EMPTY_FAMILIES);
  const processes = useMemStore((s) => s.processes ?? EMPTY_PROCESSES);
  const leaks = useMemStore((s) => s.snapshot?.leaks);

  return (
    <section>
      {/* 泄漏嫌疑只在有命中时露头：没命中就是「沉默即正常」，不占版面 */}
      {leaks ? <LeakBanner report={leaks} onOpen={() => void openMain(undefined, "apps")} /> : null}
      <div className={leaks ? "mt-1" : ""}>
        <AppMemoryList
          compact
          limit={10}
          families={families}
          processes={processes}
          leaks={leaks}
          onOpen={(pid) => void openMain(pid)}
        />
      </div>
    </section>
  );
}

function MemSection() {
  const snapshot = useMemStore((s) => s.snapshot)!;
  const { swap, compressorBytes, pressure, thrash, topSwapped } = snapshot;
  // 没有换页计数 = 该平台不提供换出读数：换出受害者与颠簸速率整块隐藏
  const hasSwapMetrics = thrash != null;

  const ratio = swap.total > 0 ? swap.used / swap.total : 0;
  const tone = ratio >= 0.9 ? "danger" : ratio >= 0.75 ? "warn" : "accent";
  const ratioText = ratio >= 0.9 ? "text-danger" : ratio >= 0.75 ? "text-warn" : "text-ink-dim";

  return (
    <section>
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <span className="text-[11px] text-ink-dim">内存压力</span>
          {pressure ? (
            <PressureDot level={pressure.level} />
          ) : (
            <span className="text-[10px] text-ink-faint">不可用</span>
          )}
        </div>
        {compressorBytes != null ? (
          <div className="text-[10px] text-ink-faint">
            压缩 <span className="font-mono tabular-nums text-ink-dim">{formatBytes(compressorBytes)}</span>
          </div>
        ) : null}
      </div>

      <div className="mt-3.5">
        <div className="flex items-baseline justify-between">
          <span className="text-[11px] text-ink-dim">swap 已用</span>
          <span className="font-mono text-[12px] tabular-nums text-ink">
            {numOf(swap.used)}
            <span className="text-ink-faint">
              {" "}
              / {numOf(swap.total)} {unitOf(swap.total)}
            </span>
            <span className={`ml-1.5 ${ratioText}`}>{formatPercent(ratio)}</span>
          </span>
        </div>
        <Meter ratio={ratio} tone={tone} className="mt-2" />
        <div className="mt-2 flex items-center justify-between text-[10px] text-ink-faint">
          <span>
            {swap.fileCount != null
              ? `↳ swapfile 占用 ${formatBytes(swap.used)}（主窗口「存储」可查）`
              : `↳ pagefile 占用 ${formatBytes(swap.used)}`}
          </span>
          {swap.newToday != null ? (
            <span>{swap.newToday > 0 ? `+${swap.newToday} 今日` : "今日 +0"}</span>
          ) : null}
        </div>
        {hasSwapMetrics ? (
          <div className="mt-1 flex items-center justify-between text-[10px] text-ink-faint">
            <span>
              颠簸 in <span className="font-mono tabular-nums">{formatRate(thrash.swapinsPerSec)}</span>/s
            </span>
            <span>
              out <span className="font-mono tabular-nums">{formatRate(thrash.swapoutsPerSec)}</span>/s
            </span>
          </div>
        ) : null}
      </div>

      {hasSwapMetrics ? (
        <>
          <div className="mt-4 flex items-baseline justify-between border-t border-hairline-soft pt-3">
            <h2 className="text-[11px] font-medium tracking-wide text-ink-dim">换出受害者</h2>
            <span className="text-[10px] text-ink-faint">
              {snapshot.deepAvailable ? "按已换出量 · 点开检视" : "暂无深探数据"}
            </span>
          </div>
          {topSwapped.length === 0 ? (
            <p className="mt-2 py-2 text-[11px] leading-relaxed text-ink-faint">
              当前没有明显被换出的进程。
            </p>
          ) : (
            <ul className="mt-1">
              {topSwapped.map((p) => (
                <VictimRow key={`${p.pid}-${p.startTime}`} proc={p} />
              ))}
            </ul>
          )}
        </>
      ) : null}

      {/* 没有命中时也要区分「检查过没问题」和「数据不够/没在检查」 */}
      {leakFootnote(snapshot.leaks)}
    </section>
  );
}

function leakFootnote(leaks: MemSnapshot["leaks"]): ReactNode {
  if (leaks.items.length > 0) return null;
  if (leaks.note) {
    return (
      <p className="mt-3 text-[10px] leading-relaxed text-ink-faint">
        泄漏判定不可用：{leaks.note}
      </p>
    );
  }
  if (leaks.insufficient > 0) {
    return (
      <p className="mt-3 text-[10px] leading-relaxed text-ink-faint">
        泄漏判定需连续观测 {Math.round(leaks.windowSecs / 60)} min 才能下结论，
        当前 {leaks.insufficient} 个进程数据不足。
      </p>
    );
  }
  return null;
}

function VictimRow({ proc }: { proc: ProcMem }) {
  const ratio = swappedRatio(proc);
  const tone = ratio != null && ratio >= 0.9 ? "text-warn" : "text-ink-dim";

  return (
    <li>
      <button
        type="button"
        onClick={() => void openMain(proc.pid)}
        className="group flex w-full items-center gap-2 rounded-md px-1.5 py-1.5 text-left transition-colors hover:bg-white/[0.045]"
      >
        <span className="min-w-0 flex-1">
          <span className="block truncate text-[12px] leading-tight text-ink">{proc.name}</span>
          <span className="mt-0.5 block truncate text-[10px] text-ink-faint">
            {proc.family} · pid {proc.pid}
          </span>
        </span>
        <span className="shrink-0 text-right">
          <span className={`block font-mono text-[12px] leading-tight tabular-nums ${tone}`}>
            {formatBytes(proc.swapped ?? 0)}
          </span>
          <span className="mt-0.5 block text-[10px] text-ink-faint">
            {ratio != null ? `${formatPercent(ratio)} 在磁盘` : "—"}
          </span>
        </span>
        <ChevronRightIcon
          size={12}
          className="shrink-0 text-ink-faint opacity-0 transition-opacity group-hover:opacity-100"
        />
      </button>
    </li>
  );
}

/**
 * 首屏双行分区（M4/S1）：内存域与存储域各占一行，边界清晰、互不混排。
 * 内存行读 memStore（压力 + 已用/总量），存储行读 diskStore（可回收 + 卷剩余）；
 * 交换两个域的展示顺序等于换 tab 入口，各自点击直达主窗口对应 tab。
 */
function DomainHero() {
  const snapshot = useMemStore((s) => s.snapshot);
  const report = useDiskStore((s) => s.report);
  const scanning = useDiskStore((s) => s.scanning);
  const progress = useDiskStore((s) => s.progress);
  const volume = report?.volume ?? null;
  const used = snapshot?.physicalUsedBytes ?? 0;
  const total = snapshot?.physicalTotal ?? 0;
  const a1 = report?.groups.find((g) => g.deadweight != null)?.deadweight ?? null;

  return (
    <section className="shrink-0 border-y border-hairline-soft">
      {/* 内存域：压力点 + 已用/总量 */}
      <button
        type="button"
        onClick={() => void openMain(undefined, "overview")}
        className="group flex w-full items-center gap-2.5 px-3.5 py-2.5 text-left transition-colors hover:bg-white/[0.03]"
        title="打开内存概况"
      >
        <PressureDot level={snapshot?.pressure?.level ?? "normal"} />
        <span className="text-[11px] font-medium text-ink-dim">内存</span>
        <span className="flex-1" />
        <span className="font-mono text-[15px] font-semibold tracking-tight tabular-nums text-ink">
          {snapshot && used > 0 ? (
            <>
              {numOf(used)}
              <span className="text-[11px] font-normal text-ink-faint">
                {" "}
                / {numOf(total)} {unitOf(total)}
              </span>
            </>
          ) : (
            "—"
          )}
        </span>
        <ChevronRightIcon
          size={12}
          className="shrink-0 text-ink-faint opacity-0 transition-opacity group-hover:opacity-100"
        />
      </button>

      {/* 存储域：⚡可回收主数字 + 卷剩余 */}
      <button
        type="button"
        onClick={() => void openMain(undefined, "disk")}
        className="group flex w-full items-center gap-2.5 border-t border-hairline-soft px-3.5 py-2.5 text-left transition-colors hover:bg-white/[0.03]"
        title={
          a1
            ? `🟢 ${formatBytes(a1.bytes)} 跨平台二进制 · 你的 Mac 永远加载不了`
            : "打开存储 / 可回收空间"
        }
      >
        <span className="text-[11px] font-medium text-ink-dim">
          <span className="mr-0.5 text-accent">⚡</span>存储
        </span>
        <span className="flex-1" />
        <span className="font-mono text-[15px] font-semibold tracking-tight tabular-nums text-accent">
          {report ? formatBytes(report.totals.reclaimable) : scanning ? "扫描中…" : "—"}
        </span>
        <span className="text-[10px] text-ink-faint">
          可回收{volume ? ` · 剩余 ${formatBytes(volume.userAvailableBytes)}` : ""}
        </span>
        <ChevronRightIcon
          size={12}
          className="shrink-0 text-ink-faint opacity-0 transition-opacity group-hover:opacity-100"
        />
      </button>

      {scanning ? (
        <div className="h-0.5 w-full overflow-hidden rounded-full bg-white/[0.06]">
          <div
            className="h-full rounded-full bg-accent transition-[width] duration-300"
            style={{ width: `${progress ? Math.round((progress.done / Math.max(progress.total, 1)) * 100) : 8}%` }}
          />
        </div>
      ) : null}
    </section>
  );
}

function Skeleton({ loading }: { loading: boolean }) {
  return (
    <div className="space-y-3">
      <div className="h-3 w-24 animate-pulse rounded bg-white/[0.06]" />
      <div className="h-20 animate-pulse rounded-lg bg-white/[0.04]" />
      <div className="h-3 w-20 animate-pulse rounded bg-white/[0.06]" />
      <div className="h-24 animate-pulse rounded-lg bg-white/[0.04]" />
      <p className="text-[10px] text-ink-faint">{loading ? "正在采样…" : "无数据"}</p>
    </div>
  );
}

function IconButton({
  label,
  onClick,
  children,
}: {
  label: string;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      onClick={onClick}
      className="grid size-6 place-items-center rounded-md text-ink-faint transition-colors hover:bg-white/[0.06] hover:text-ink"
    >
      {children}
    </button>
  );
}
