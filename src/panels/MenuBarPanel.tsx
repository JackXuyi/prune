import { useState, type ReactNode } from "react";

import { AppMemoryList, AppMemorySummary } from "../components/AppMemoryList";
import { LeakBanner } from "../components/LeakCard";
import { formatBytes, formatPercent, formatRate, formatRelative, numOf, unitOf } from "../lib/format";
import { openMain } from "../lib/ipc";
import { useDiskStore } from "../stores/diskStore";
import { useMemStore } from "../stores/memStore";
import { LEVEL_META } from "../types/disk";
import { swappedRatio, type FamilyMem, type MemSnapshot, type ProcMem } from "../types/mem";
import { ChevronRightIcon, DiskIcon, ExpandIcon, RefreshIcon, ScissorsIcon } from "../components/icons";
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
        <header className="flex h-11 shrink-0 items-center gap-2 border-b border-hairline-soft px-3">
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

        <DiskHero />

        <div className="flex h-8 shrink-0 items-center justify-between border-b border-hairline-soft px-2.5">
          <Tabs
            size="sm"
            value={tab}
            onChange={setTab}
            items={[
              { id: "apps", label: "应用", hint: "每个应用的绝对内存占用" },
              { id: "overview", label: "概况", hint: "内存压力与 swap" },
            ]}
          />
          {snapshot && tab === "overview" ? (
            <span className="text-[10px] text-ink-faint">
              swap {formatPercent(snapshot.swap.total > 0 ? snapshot.swap.used / snapshot.swap.total : 0)}
            </span>
          ) : null}
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto px-3.5 py-3">
          {error ? (
            <div className="rounded-lg border border-danger/30 bg-danger/[0.08] p-3 text-[11px] leading-relaxed text-danger">
              {error}
            </div>
          ) : !snapshot ? (
            <Skeleton loading={loading} />
          ) : tab === "apps" ? (
            <AppsSection />
          ) : (
            <>
              <MemSection />
              <DiskSection />
            </>
          )}
        </div>

        <footer className="flex shrink-0 items-center justify-between border-t border-hairline-soft px-3.5 py-2 text-[10px] text-ink-faint">
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
      <div className="flex items-baseline justify-between gap-2">
        <h2 className="shrink-0 text-[11px] font-medium tracking-wide text-ink-dim">
          每个应用占用
        </h2>
        <AppMemorySummary families={families} />
      </div>
      <div className="mt-1">
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

  const ratio = swap.total > 0 ? swap.used / swap.total : 0;
  const tone = ratio >= 0.9 ? "danger" : ratio >= 0.75 ? "warn" : "accent";
  const ratioText = ratio >= 0.9 ? "text-danger" : ratio >= 0.75 ? "text-warn" : "text-ink-dim";

  return (
    <section>
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <span className="text-[11px] text-ink-dim">内存压力</span>
          <PressureDot level={pressure.level} />
        </div>
        <div className="text-[10px] text-ink-faint">
          压缩 <span className="font-mono tabular-nums text-ink-dim">{formatBytes(compressorBytes)}</span>
        </div>
      </div>

      <div className="mt-2.5 rounded-lg border border-hairline-soft bg-surface-raised/50 p-2.5">
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
        <div className="mt-1.5 flex items-center justify-between text-[10px] text-ink-faint">
          <span>↳ 正在吃掉磁盘 {formatBytes(swap.used)}</span>
          <span>{swap.newToday > 0 ? `+${swap.newToday} 今日` : "今日 +0"}</span>
        </div>
        <div className="mt-1 flex items-center justify-between text-[10px] text-ink-faint">
          <span>
            颠簸 in <span className="font-mono tabular-nums">{formatRate(thrash.swapinsPerSec)}</span>/s
          </span>
          <span>
            out <span className="font-mono tabular-nums">{formatRate(thrash.swapoutsPerSec)}</span>/s
          </span>
        </div>
      </div>

      <div className="mt-3.5 flex items-baseline justify-between">
        <h2 className="text-[11px] font-medium tracking-wide text-ink-dim">换出受害者</h2>
        <span className="text-[10px] text-ink-faint">
          {snapshot.deepAvailable ? "按已换出量 · 点开检视" : "暂无深探数据"}
        </span>
      </div>

      {topSwapped.length === 0 ? (
        <p className="mt-1.5 rounded-lg border border-dashed border-hairline px-2.5 py-3 text-[11px] text-ink-faint">
          当前没有明显被换出的进程。
        </p>
      ) : (
        <ul className="mt-1">
          {topSwapped.map((p) => (
            <VictimRow key={`${p.pid}-${p.startTime}`} proc={p} />
          ))}
        </ul>
      )}

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

function DiskSection() {
  const report = useDiskStore((s) => s.report);
  const scanning = useDiskStore((s) => s.scanning);

  if (!report) {
    return (
      <section className="mt-4 border-t border-hairline-soft pt-3">
        <div className="flex items-center gap-1.5 text-[11px] text-ink-dim">
          <DiskIcon size={12} />
          磁盘可回收
        </div>
        <p className="mt-1 text-[10px] leading-relaxed text-ink-faint">
          {scanning ? "正在扫描缓存目录…" : "尚未扫描（启动 30 秒后自动扫，也可在主窗口手动触发）"}
        </p>
      </section>
    );
  }

  const rows = [
    { level: "l1" as const, label: "跨平台二进制", bytes: report.totals.deadweight },
    { level: "l2" as const, label: "包管理器缓存", bytes: report.totals.l2 },
    { level: "l3" as const, label: "项目依赖 / 模拟器", bytes: report.totals.l3 },
  ];

  return (
    <section className="mt-4 border-t border-hairline-soft pt-3">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-1.5 text-[11px] text-ink-dim">
          <DiskIcon size={12} />
          可回收分层
        </div>
        <button
          type="button"
          onClick={() => void openMain(undefined, "disk")}
          className="text-[10px] text-ink-faint transition-colors hover:text-accent"
        >
          查看全部 →
        </button>
      </div>
      <ul className="mt-1.5 space-y-1">
        {rows.map((r) => (
          <li key={r.level} className="flex items-baseline justify-between text-[11px]">
            <span className="text-ink-dim">{LEVEL_META[r.level].label} · {r.label}</span>
            <span className="font-mono tabular-nums text-ink">{formatBytes(r.bytes)}</span>
          </li>
        ))}
      </ul>
      {report.totals.l4 > 0 ? (
        <p className="mt-1.5 text-[10px] text-ink-faint">
          🔴 Docker 等高风险项 {formatBytes(report.totals.l4)} · 只提供命令，不计入主数字
        </p>
      ) : null}
    </section>
  );
}

/** 首屏主数字：可回收 GB（功能方案 §3.1 —— 主数字是可行动的，不是内存百分比） */
function DiskHero() {
  const report = useDiskStore((s) => s.report);
  const scanning = useDiskStore((s) => s.scanning);
  const progress = useDiskStore((s) => s.progress);
  const a1 = report?.groups.find((g) => g.deadweight != null)?.deadweight ?? null;

  return (
    <button
      type="button"
      onClick={() => void openMain(undefined, "disk")}
      className="group shrink-0 border-b border-hairline-soft px-3.5 py-2.5 text-left transition-colors hover:bg-white/[0.03]"
    >
      <div className="flex items-baseline justify-between gap-2">
        <span className="text-[16px] font-semibold tracking-tight text-ink">
          <span className="mr-1 text-accent">⚡</span>
          {report ? formatBytes(report.totals.reclaimable) : scanning ? "扫描中…" : "—"}
          <span className="ml-1 text-[10px] font-normal text-ink-faint">可回收</span>
        </span>
        <ChevronRightIcon
          size={12}
          className="shrink-0 text-ink-faint transition-transform group-hover:translate-x-0.5"
        />
      </div>
      {a1 ? (
        <p className="mt-0.5 text-[10px] leading-tight text-ink-faint">
          🟢 <span className="font-mono tabular-nums text-ink-dim">{formatBytes(a1.bytes)}</span>{" "}
          跨平台二进制 · 你的 Mac 永远加载不了
        </p>
      ) : (
        <p className="mt-0.5 text-[10px] leading-tight text-ink-faint">
          {scanning
            ? progress
              ? `${progress.done}/${progress.total} 组 · ${progress.phase}`
              : "正在扫描缓存目录…"
            : "点击查看可回收空间"}
        </p>
      )}
      {scanning ? (
        <div className="mt-1.5 h-0.5 w-full overflow-hidden rounded-full bg-white/[0.06]">
          <div
            className="h-full rounded-full bg-accent transition-[width] duration-300"
            style={{ width: `${progress ? Math.round((progress.done / Math.max(progress.total, 1)) * 100) : 8}%` }}
          />
        </div>
      ) : null}
    </button>
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
