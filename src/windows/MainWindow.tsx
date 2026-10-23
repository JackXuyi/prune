import { useMemo, useState } from "react";

import { Sparkline } from "../components/Sparkline";
import {
  ChevronRightIcon,
  RefreshIcon,
  ScissorsIcon,
} from "../components/icons";
import { Badge, CompareBar, PressureDot, Stat } from "../components/primitives";
import { formatBytes, formatClock, formatPercent, formatRate, formatRelative, numOf, unitOf } from "../lib/format";
import { useMemHistory, useMemStore } from "../stores/memStore";
import { swappedRatio, totalOf, type FamilyMem, type ProcMem } from "../types/mem";

/** 主窗口（960×640 双栏）：左家族聚合，右进程检视器 */
export function MainWindow() {
  const snapshot = useMemStore((s) => s.snapshot);
  const processes = useMemStore((s) => s.processes);
  const loading = useMemStore((s) => s.loading);
  const refreshing = useMemStore((s) => s.refreshing);
  const error = useMemStore((s) => s.error);
  const selectedPid = useMemStore((s) => s.selectedPid);
  const select = useMemStore((s) => s.select);
  const refresh = useMemStore((s) => s.refresh);

  const [expanded, setExpanded] = useState<string | null>(null);

  const families = snapshot?.families ?? [];
  const selected = useMemo(
    () => processes.find((p) => p.pid === selectedPid) ?? null,
    [processes, selectedPid],
  );

  return (
    <div className="flex h-screen flex-col bg-surface text-ink">
      <header className="flex h-12 shrink-0 items-center gap-2.5 border-b border-hairline px-4">
        <ScissorsIcon size={15} className="text-accent" />
        <span className="text-[13px] font-semibold tracking-tight">Prune</span>
        <span className="text-hairline">/</span>
        <span className="text-[12px] text-ink-dim">内存真相</span>
        {snapshot ? (
          snapshot.deepAvailable ? (
            <Badge tone="accent" title="footprint 口径：含被换出到磁盘的部分">
              footprint 口径
            </Badge>
          ) : (
            <Badge tone="warn" title="footprint 不可用，全部读数退回 RSS，不含换出量">
              RSS 口径 · 不含换出量
            </Badge>
          )
        ) : null}
        <div className="flex-1" />
        <span className="text-[10px] text-ink-faint">
          上次深探 {formatRelative(snapshot?.deepUpdatedAt ?? null)}
        </span>
        <button
          type="button"
          onClick={() => void refresh()}
          className="flex items-center gap-1.5 rounded-md border border-hairline bg-surface-raised px-2.5 py-1.5 text-[11px] text-ink-dim transition-colors hover:border-hairline hover:bg-surface-overlay hover:text-ink"
        >
          <RefreshIcon size={12} className={refreshing ? "animate-spin" : ""} />
          {refreshing ? "采样中" : "刷新"}
        </button>
      </header>

      {error ? (
        <div className="m-4 rounded-lg border border-danger/30 bg-danger/[0.08] px-3 py-2 text-[11px] text-danger">
          {error}
        </div>
      ) : null}

      <div className="flex min-h-0 flex-1">
        <aside className="flex w-[350px] shrink-0 flex-col border-r border-hairline bg-[#0c0f12]">
          <div className="flex h-10 shrink-0 items-center justify-between border-b border-hairline-soft px-3.5">
            <span className="text-[11px] font-medium tracking-wide text-ink-dim">进程家族</span>
            <span className="text-[10px] text-ink-faint">
              {families.length} 个 · 按总量降序
            </span>
          </div>
          <div className="min-h-0 flex-1 overflow-y-auto p-1.5">
            {loading && !snapshot ? (
              <p className="px-2 py-3 text-[11px] text-ink-faint">正在采样…</p>
            ) : families.length === 0 ? (
              <p className="px-2 py-3 text-[11px] text-ink-faint">暂无进程数据</p>
            ) : (
              <ul>
                {families.map((f) => (
                  <FamilyRow
                    key={f.name}
                    family={f}
                    open={expanded === f.name}
                    selectedPid={selectedPid}
                    processes={processes}
                    onToggle={() => setExpanded(expanded === f.name ? null : f.name)}
                    onSelect={select}
                  />
                ))}
              </ul>
            )}
          </div>
        </aside>

        <main className="min-w-0 flex-1 overflow-y-auto px-5 py-4">
          {selected ? <ProcessInspector proc={selected} /> : <Overview />}
        </main>
      </div>
    </div>
  );
}

function FamilyRow({
  family,
  open,
  selectedPid,
  processes,
  onToggle,
  onSelect,
}: {
  family: FamilyMem;
  open: boolean;
  selectedPid: number | null;
  processes: ProcMem[];
  onToggle: () => void;
  onSelect: (pid: number | null) => void;
}) {
  const total = totalOf(family);
  const ratio = swappedRatio(family);
  const children = useMemo(
    () =>
      processes
        .filter((p) => p.family === family.name)
        .sort((a, b) => totalOf(b) - totalOf(a)),
    [processes, family.name],
  );

  return (
    <li>
      <button
        type="button"
        onClick={onToggle}
        aria-expanded={open}
        className="group flex w-full items-center gap-1.5 rounded-md px-2 py-2 text-left transition-colors hover:bg-white/[0.04]"
      >
        <ChevronRightIcon
          size={12}
          className={`shrink-0 text-ink-faint transition-transform ${open ? "rotate-90" : ""}`}
        />
        <span className="min-w-0 flex-1">
          <span className="block truncate text-[12.5px] leading-tight text-ink">
            {family.name}
          </span>
          <span className="mt-0.5 block text-[10px] text-ink-faint">
            {family.processCount} 个进程
            {family.footprint == null ? " · RSS" : ""}
          </span>
        </span>
        <span className="shrink-0 text-right">
          <span className="block font-mono text-[12px] leading-tight tabular-nums text-ink">
            {formatBytes(total)}
          </span>
          <span className="mt-0.5 block text-[10px] text-ink-faint">
            {ratio != null ? `${formatPercent(ratio)} 在磁盘` : "—"}
          </span>
        </span>
      </button>

      {open ? (
        <ul className="mb-1 ml-3.5 border-l border-hairline-soft pl-1.5">
          {children.map((p) => {
            const isActive = p.pid === selectedPid;
            return (
              <li key={`${p.pid}-${p.startTime}`}>
                <button
                  type="button"
                  onClick={() => onSelect(p.pid)}
                  className={`flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left transition-colors ${
                    isActive ? "bg-accent/[0.10] text-ink" : "text-ink-dim hover:bg-white/[0.04]"
                  }`}
                >
                  <span className="min-w-0 flex-1 truncate text-[11.5px]">{p.name}</span>
                  <span className="shrink-0 font-mono text-[10px] tabular-nums text-ink-faint">
                    {formatBytes(totalOf(p))}
                  </span>
                </button>
              </li>
            );
          })}
        </ul>
      ) : null}
    </li>
  );
}

function Overview() {
  const snapshot = useMemStore((s) => s.snapshot);
  if (!snapshot) return null;

  const { swap, pressure, thrash, compressorBytes, pagesFreeBytes, physicalTotal, topSwapped } =
    snapshot;
  const swapRatio = swap.total > 0 ? swap.used / swap.total : 0;

  return (
    <div className="space-y-4">
      <div>
        <h1 className="text-[15px] font-semibold tracking-tight">系统总览</h1>
        <p className="mt-0.5 text-[11px] text-ink-faint">
          点击左侧家族展开进程，查看 footprint 与换出明细
        </p>
      </div>

      <div className="grid grid-cols-2 gap-2.5">
        <Stat label="物理内存" value={numOf(physicalTotal)} unit={unitOf(physicalTotal)} />
        <Stat
          label="压缩内存"
          value={formatBytes(compressorBytes)}
          hint="vm_stat 压缩页"
        />
        <Stat label="空闲页" value={formatBytes(pagesFreeBytes)} hint="Pages free" />
        <Stat
          label="swap 已用"
          value={formatPercent(swapRatio)}
          tone={swapRatio >= 0.9 ? "danger" : swapRatio >= 0.75 ? "warn" : "accent"}
          hint={`${formatBytes(swap.used)} / ${formatBytes(swap.total)} · ${swap.fileCount} 个 swapfile`}
        />
      </div>

      <section className="rounded-lg border border-hairline-soft bg-surface-raised/50 p-3">
        <div className="flex items-center justify-between">
          <span className="text-[11px] text-ink-dim">内存压力</span>
          <PressureDot level={pressure.level} />
        </div>
        <div className="mt-2 flex items-center justify-between text-[10px] text-ink-faint">
          <span>
            内核原始值 <span className="font-mono text-ink-dim">{pressure.raw}</span>（1 正常 / 2 警告 / 4 紧张）
          </span>
        </div>
        <div className="mt-2.5 grid grid-cols-2 gap-2 text-[10px] text-ink-faint">
          <span>
            换入速率{" "}
            <span className="font-mono tabular-nums text-ink-dim">
              {formatRate(thrash.swapinsPerSec)}/s
            </span>
          </span>
          <span>
            换出速率{" "}
            <span className="font-mono tabular-nums text-ink-dim">
              {formatRate(thrash.swapoutsPerSec)}/s
            </span>
          </span>
        </div>
        <div className="mt-1.5 text-[10px] text-ink-faint">
          今日新增 swapfile <span className="font-mono text-ink-dim">+{swap.newToday}</span>
        </div>
      </section>

      <section className="rounded-lg border border-hairline-soft bg-surface-raised/50 p-3">
        <div className="flex items-baseline justify-between">
          <span className="text-[11px] text-ink-dim">换出受害者</span>
          <span className="text-[10px] text-ink-faint">存活但大部分在磁盘上</span>
        </div>
        {topSwapped.length === 0 ? (
          <p className="mt-2 text-[11px] text-ink-faint">当前没有明显被换出的进程。</p>
        ) : (
          <ul className="mt-1.5">
            {topSwapped.map((p) => (
              <VictimRow key={`${p.pid}-${p.startTime}`} proc={p} />
            ))}
          </ul>
        )}
      </section>

      {!snapshot.deepAvailable ? (
        <p className="rounded-lg border border-warn/25 bg-warn/[0.06] px-3 py-2 text-[11px] leading-relaxed text-warn">
          深探不可用：当前全部为 RSS 口径（不含换出量）。请确认 /usr/bin/footprint 可执行。
        </p>
      ) : null}
    </div>
  );
}

function VictimRow({ proc }: { proc: ProcMem }) {
  const select = useMemStore((s) => s.select);
  const ratio = swappedRatio(proc);
  return (
    <li>
      <button
        type="button"
        onClick={() => select(proc.pid)}
        className="flex w-full items-center gap-2 rounded-md px-1 py-1.5 text-left transition-colors hover:bg-white/[0.04]"
      >
        <span className="min-w-0 flex-1 truncate text-[11.5px] text-ink">{proc.name}</span>
        <span className="shrink-0 font-mono text-[11px] tabular-nums text-warn">
          {formatBytes(proc.swapped ?? 0)}
        </span>
        <span className="w-14 shrink-0 text-right text-[10px] text-ink-faint">
          {ratio != null ? formatPercent(ratio) : "—"}
        </span>
      </button>
    </li>
  );
}

function ProcessInspector({ proc }: { proc: ProcMem }) {
  const points = useMemHistory(proc.pid, 240);
  const snapshot = useMemStore((s) => s.snapshot);
  const footprint = proc.footprint;
  const rss = proc.rss;
  const swapped = proc.swapped ?? 0;
  const ratio = swappedRatio(proc);
  const max = Math.max(1, footprint ?? 0, rss);

  const seriesKey = footprint == null ? "rss" : "footprint";
  const values = points.map((p) => (seriesKey === "footprint" ? (p.footprint ?? p.rss) : p.rss));
  const seriesMax = values.length ? Math.max(...values) : 0;
  const seriesMin = values.length ? Math.min(...values) : 0;

  return (
    <div className="space-y-4">
      <div className="flex items-start gap-3">
        <div className="min-w-0 flex-1">
          <h1 className="truncate text-[15px] font-semibold tracking-tight">{proc.name}</h1>
          <p className="mt-0.5 truncate text-[11px] text-ink-faint">
            {proc.family} · pid {proc.pid} · ppid {proc.ppid}
          </p>
        </div>
        {footprint == null ? (
          <Badge tone="warn" title="该进程尚未完成 footprint 深探">RSS 口径</Badge>
        ) : (
          <Badge tone="accent" title="footprint 含被换出到磁盘的部分">footprint 口径</Badge>
        )}
      </div>

      <section className="rounded-lg border border-hairline-soft bg-surface-raised/50 p-3.5">
        <div className="mb-3 flex items-baseline justify-between">
          <span className="text-[11px] font-medium text-ink-dim">内存构成</span>
          <span className="text-[10px] text-ink-faint">同量纲对照</span>
        </div>

        <div className="space-y-3">
          <CompareBar
            label="footprint（含换出）"
            value={footprint == null ? "未深探" : formatBytes(footprint)}
            ratio={(footprint ?? 0) / max}
            tone="accent"
          />
          <CompareBar
            label="RSS（物理常驻）"
            value={formatBytes(rss)}
            ratio={rss / max}
            tone="neutral"
            sub={footprint != null && footprint > 0 ? `占 footprint 的 ${formatPercent(rss / footprint)}` : undefined}
          />
          <CompareBar
            label="已换出到磁盘"
            value={formatBytes(swapped)}
            ratio={footprint ? swapped / footprint : 0}
            tone="warn"
            sub={ratio != null ? `占 footprint 的 ${formatPercent(ratio)}` : "需要 footprint 深探数据"}
          />
        </div>
      </section>

      <section className="rounded-lg border border-hairline-soft bg-surface-raised/50 p-3.5">
        <div className="flex items-baseline justify-between">
          <span className="text-[11px] font-medium text-ink-dim">
            {seriesKey === "footprint" ? "footprint 走势" : "RSS 走势"}
          </span>
          <span className="text-[10px] text-ink-faint">最近 4 小时 · 5 min 采样</span>
        </div>
        <div className="mt-2.5">
          <Sparkline values={values} tone={seriesKey === "footprint" ? "accent" : "warn"} />
        </div>
        {points.length > 0 ? (
          <div className="mt-1.5 flex items-center justify-between text-[10px] text-ink-faint">
            <span>
              低 <span className="font-mono tabular-nums text-ink-dim">{formatBytes(seriesMin)}</span>
            </span>
            <span>{points.length} 个点</span>
            <span>
              高 <span className="font-mono tabular-nums text-ink-dim">{formatBytes(seriesMax)}</span>
            </span>
          </div>
        ) : null}
      </section>

      <section className="rounded-lg border border-hairline-soft bg-surface-raised/50 p-3.5">
        <div className="mb-2 text-[11px] font-medium text-ink-dim">进程信息</div>
        <dl className="space-y-1.5 text-[11px]">
          <Row label="可执行文件" value={proc.exe ?? "不可读"} mono />
          <Row label="启动时间" value={formatClock(proc.startTime)} mono />
          <Row label="家族" value={proc.family} />
          <Row
            label="读数来源"
            value={proc.source === "footprint" ? "/usr/bin/footprint --swapped" : "sysinfo RSS"}
            mono
          />
        </dl>
        {snapshot?.deepAvailable === false ? (
          <p className="mt-2.5 rounded border border-warn/25 bg-warn/[0.06] px-2 py-1.5 text-[10px] leading-relaxed text-warn">
            深探不可用，当前为 RSS 口径（不含换出量）。
          </p>
        ) : null}
      </section>
    </div>
  );
}

function Row({ label, value, mono }: { label: string; value: string; mono?: boolean }) {
  return (
    <div className="flex items-start gap-3">
      <dt className="w-20 shrink-0 text-ink-faint">{label}</dt>
      <dd
        className={`min-w-0 flex-1 break-all text-ink-dim ${mono ? "font-mono text-[10.5px]" : ""}`}
        title={value}
      >
        {value}
      </dd>
    </div>
  );
}
