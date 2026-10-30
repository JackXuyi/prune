import { useEffect, useMemo, useState } from "react";

import { listen } from "@tauri-apps/api/event";

import { AppMemoryList, AppMemorySummary } from "../components/AppMemoryList";
import { LeakCard, LeakEvidence } from "../components/LeakCard";
import { ReclaimBoard } from "../components/ReclaimBoard";
import { Sparkline } from "../components/Sparkline";
import {
  ChevronRightIcon,
  RefreshIcon,
  ScissorsIcon,
} from "../components/icons";
import { Badge, CompareBar, PressureDot, Stat } from "../components/primitives";
import { Tabs } from "../components/Tabs";
import { formatBytes, formatClock, formatPercent, formatRate, formatRelative, numOf, unitOf } from "../lib/format";
import { isTauri, killProcess } from "../lib/ipc";
import { useDiskStore } from "../stores/diskStore";
import { useMemHistory, useMemStore } from "../stores/memStore";
import { isExact, swappedRatio, totalOf, type FamilyMem, type ProcMem, type WorkspaceSource } from "../types/mem";

type MainTab = "apps" | "overview" | "disk";

/** 右键「结束进程」的目标：哪个进程、菜单弹在哪 */
type KillTarget = { pid: number; name: string; mem: string; x: number; y: number };

/** 工作区归属的置信度说明（UI 必须让用户知道这个路径是怎么来的） */
function sourceLabel(s: WorkspaceSource): string {
  return s === "handle"
    ? "依据打开的文件句柄"
    : s === "label"
      ? "依据编辑器窗口标签"
      : "继承所属窗口";
}

/** 主窗口（960×640）：应用内存 / 系统概况 两个 tab */
export function MainWindow() {
  const [tab, setTab] = useState<MainTab>("apps");
  const snapshot = useMemStore((s) => s.snapshot);
  const processes = useMemStore((s) => s.processes);
  const loading = useMemStore((s) => s.loading);
  const refreshing = useMemStore((s) => s.refreshing);
  const error = useMemStore((s) => s.error);
  const selectedPid = useMemStore((s) => s.selectedPid);
  const select = useMemStore((s) => s.select);
  const refresh = useMemStore((s) => s.refresh);
  const diskScanning = useDiskStore((s) => s.scanning);
  const refreshDisk = useDiskStore((s) => s.refresh);

  const [expanded, setExpanded] = useState<string | null>(null);
  const [killTarget, setKillTarget] = useState<KillTarget | null>(null);

  /** 右键进程行 → 弹「结束进程」菜单（位置取鼠标点） */
  const openKillMenu = (proc: ProcMem, x: number, y: number) => {
    setKillTarget({ pid: proc.pid, name: proc.name, mem: formatBytes(totalOf(proc)), x, y });
  };

  // 面板点「可回收」→ 主窗口直接切到对应 tab（open_main 会让 Rust 侧广播 ui://tab）
  useEffect(() => {
    if (!isTauri()) return;
    const un = listen<string>("ui://tab", (e) => {
      if (e.payload === "apps" || e.payload === "overview" || e.payload === "disk") {
        setTab(e.payload);
      }
    });
    return () => {
      void un.then((fn) => fn());
    };
  }, []);

  const families = snapshot?.families ?? [];
  const selected = useMemo(
    () => processes.find((p) => p.pid === selectedPid) ?? null,
    [processes, selectedPid],
  );

  return (
    <div className="flex h-screen flex-col bg-surface text-ink">
      <header className="flex h-12 shrink-0 items-center gap-4 border-b border-hairline px-5">
        <div className="flex items-center gap-2">
          <ScissorsIcon size={15} className="text-accent" />
          <span className="text-[13px] font-semibold tracking-tight">Prune</span>
        </div>
        <Tabs
          value={tab}
          onChange={setTab}
          items={[
            { id: "apps", label: "内存 · 应用", hint: "每个应用的绝对内存占用" },
            { id: "overview", label: "内存 · 概况", hint: "内存压力、swap 与换出受害者" },
            { id: "disk", label: "存储", hint: "卷容量、可回收空间与清理" },
          ]}
        />
        <div className="flex-1" />
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
        <span className="text-[10px] text-ink-faint">
          上次深探 {formatRelative(snapshot?.deepUpdatedAt ?? null)}
        </span>
        <button
          type="button"
          onClick={() => void (tab === "disk" ? refreshDisk() : refresh())}
          disabled={tab === "disk" ? diskScanning : refreshing}
          className="flex items-center gap-1.5 rounded-md bg-white/[0.05] px-2.5 py-1.5 text-[11px] text-ink-dim transition-colors hover:bg-white/[0.09] hover:text-ink disabled:opacity-50"
        >
          <RefreshIcon size={12} className={(tab === "disk" ? diskScanning : refreshing) ? "animate-spin" : ""} />
          {tab === "disk"
            ? diskScanning
              ? "扫描中"
              : "重新扫描"
            : refreshing
              ? "采样中"
              : "刷新"}
        </button>
      </header>

      {error ? (
        <div className="mx-5 mt-3 rounded-lg bg-danger/[0.08] px-3 py-2 text-[11px] text-danger">
          {error}
        </div>
      ) : null}

      {tab === "disk" ? (
        <ReclaimBoard />
      ) : tab === "apps" ? (
        <div className="min-h-0 flex-1 overflow-y-auto px-5 py-4">
          <div className="flex items-baseline justify-between">
            <div>
              <h1 className="text-[15px] font-semibold tracking-tight">每个应用占用的绝对内存</h1>
              <p className="mt-0.5 text-[11px] text-ink-faint">
                footprint 口径（含被换出到磁盘的部分）· 按总量降序 · 点击查看进程明细
              </p>
            </div>
            <AppMemorySummary families={families} />
          </div>
          {/* 命中或「无法判定」时才出现；没有命中就是沉默即正常 */}
          {snapshot?.leaks && (snapshot.leaks.items.length > 0 || snapshot.leaks.note) ? (
            <div className="mt-3">
              <LeakCard
                report={snapshot.leaks}
                onOpen={(pid) => {
                  select(pid);
                  setTab("overview");
                }}
              />
            </div>
          ) : null}
          <div className="mt-3">
            <div className="grid grid-cols-[26px_minmax(0,1fr)_112px_86px] items-center gap-2.5 border-b border-hairline-soft px-2 pb-2 text-[10px] text-ink-faint">
              <span>#</span>
              <span>应用</span>
              <span>相对占用</span>
              <span className="text-right">总量</span>
            </div>
            <AppMemoryList
              families={families}
              processes={processes}
              leaks={snapshot?.leaks}
              onOpen={(pid) => {
                select(pid);
                setTab("overview");
              }}
            />
          </div>
        </div>
      ) : (
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
                      onKillMenu={openKillMenu}
                    />
                  ))}
                </ul>
              )}
            </div>
          </aside>

          <main className="min-w-0 flex-1 overflow-y-auto px-5 py-4">
            {selected ? (
              <ProcessInspector proc={selected} />
            ) : (
              <Overview onKillMenu={openKillMenu} />
            )}
          </main>
        </div>
      )}

      {killTarget ? <ProcKillMenu target={killTarget} onClose={() => setKillTarget(null)} /> : null}
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
  onKillMenu,
}: {
  family: FamilyMem;
  open: boolean;
  selectedPid: number | null;
  processes: ProcMem[];
  onToggle: () => void;
  onSelect: (pid: number | null) => void;
  onKillMenu: (proc: ProcMem, x: number, y: number) => void;
}) {
  const total = totalOf(family);
  const ratio = swappedRatio(family);
  const exact = isExact(family);
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
            {family.workspaces.length > 0 ? ` · ${family.workspaces.length} 个项目` : ""}
            {family.footprint == null ? " · RSS" : exact ? "" : " · ≈"}
          </span>
        </span>
        <span className="shrink-0 text-right">
          <span className="block font-mono text-[12px] leading-tight tabular-nums text-ink">
            {exact ? "" : "≈"}
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
                  onContextMenu={(e) => {
                    e.preventDefault();
                    onKillMenu(p, e.clientX, e.clientY);
                  }}
                  title="点击查看明细 · 右键结束进程"
                  className={`flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left transition-colors ${
                    isActive ? "bg-accent/[0.10] text-ink" : "text-ink-dim hover:bg-white/[0.04]"
                  }`}
                >
                  <span className="min-w-0 flex-1 truncate text-[11.5px]">
                    {p.name}
                    {p.workspace ? (
                      <span
                        className="ml-1.5 text-[10px] text-ink-faint"
                        title={`${p.workspace.path}（${sourceLabel(p.workspace.source)}）`}
                      >
                        {p.workspace.label}
                      </span>
                    ) : null}
                  </span>
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

function Overview({ onKillMenu }: { onKillMenu: (proc: ProcMem, x: number, y: number) => void }) {
  const snapshot = useMemStore((s) => s.snapshot);
  if (!snapshot) return null;

  const { swap, pressure, thrash, compressorBytes, pagesFreeBytes, physicalTotal, topSwapped } =
    snapshot;
  const swapRatio = swap.total > 0 ? swap.used / swap.total : 0;

  return (
    <div className="space-y-4">
      <div>
        <h1 className="text-[15px] font-semibold tracking-tight">内存概况</h1>
        <p className="mt-0.5 text-[11px] text-ink-faint">
          内存域系统级读数 · 点击左侧家族展开进程 · 磁盘读数见「存储」tab
        </p>
      </div>

      <div className="grid grid-cols-2 gap-x-8 gap-y-5 border-b border-hairline-soft pb-5">
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
          hint={`${formatBytes(swap.used)} / ${formatBytes(swap.total)} · ${swap.fileCount} 个 swapfile · 占用存储见「存储」tab`}
        />
      </div>

      <section className="border-b border-hairline-soft pb-5">
        <div className="flex items-center justify-between">
          <span className="text-[11px] text-ink-dim">内存压力</span>
          <PressureDot level={pressure.level} />
        </div>
        <div className="mt-2.5 flex items-center justify-between text-[10px] text-ink-faint">
          <span>
            内核原始值 <span className="font-mono text-ink-dim">{pressure.raw}</span>（1 正常 / 2 警告 / 4 紧张）
          </span>
          <span>
            今日新增 swapfile <span className="font-mono text-ink-dim">+{swap.newToday}</span>
          </span>
        </div>
        <div className="mt-2 grid grid-cols-2 gap-2 text-[10px] text-ink-faint">
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
      </section>

      <section>
        <div className="flex items-baseline justify-between">
          <span className="text-[11px] text-ink-dim">换出受害者</span>
          <span className="text-[10px] text-ink-faint">存活但大部分在磁盘上</span>
        </div>
        {topSwapped.length === 0 ? (
          <p className="mt-2 py-1 text-[11px] text-ink-faint">当前没有明显被换出的进程。</p>
        ) : (
          <ul className="mt-1.5">
            {topSwapped.map((p) => (
              <VictimRow
                key={`${p.pid}-${p.startTime}`}
                proc={p}
                onKillMenu={onKillMenu}
              />
            ))}
          </ul>
        )}
      </section>

      {!snapshot.deepAvailable ? (
        <p className="mt-4 rounded-lg bg-warn/[0.07] px-3 py-2 text-[11px] leading-relaxed text-warn">
          深探不可用：当前全部为 RSS 口径（不含换出量）。请确认 /usr/bin/footprint 可执行。
        </p>
      ) : null}
    </div>
  );
}

function VictimRow({
  proc,
  onKillMenu,
}: {
  proc: ProcMem;
  onKillMenu: (proc: ProcMem, x: number, y: number) => void;
}) {
  const select = useMemStore((s) => s.select);
  const ratio = swappedRatio(proc);
  return (
    <li>
      <button
        type="button"
        onClick={() => select(proc.pid)}
        onContextMenu={(e) => {
          e.preventDefault();
          onKillMenu(proc, e.clientX, e.clientY);
        }}
        title="点击查看明细 · 右键结束进程"
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
  // B5：该进程被判定为「疑似泄漏」时展示证据（pid 相同即可，startTime 也需一致才对上同一进程）
  const leak =
    snapshot?.leaks.items.find(
      (i) => i.pid === proc.pid && (i.startTime === 0 || i.startTime === proc.startTime),
    ) ?? null;
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

      <section className="border-b border-hairline-soft pb-4">
        <div className="mb-3 flex items-baseline justify-between">
          <span className="text-[11px] font-medium text-ink-dim">内存构成</span>
          <span className="text-[10px] text-ink-faint">同量纲对照</span>
        </div>

        <div className="space-y-3.5">
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

      <section className="border-b border-hairline-soft pb-4">
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

      {leak ? <LeakEvidence item={leak} /> : null}

      <section>
        <div className="mb-2 text-[11px] font-medium text-ink-dim">进程信息</div>
        <dl className="space-y-1.5 text-[11px]">
          <Row label="可执行文件" value={proc.exe ?? "不可读"} mono />
          <Row
            label="工作区"
            value={proc.workspace ? proc.workspace.path : "未归属（没有可用的项目线索）"}
            mono={!!proc.workspace}
          />
          {proc.workspace ? (
            <Row label="归属依据" value={sourceLabel(proc.workspace.source)} />
          ) : null}
          <Row label="启动时间" value={formatClock(proc.startTime)} mono />
          <Row label="家族" value={proc.family} />
          <Row
            label="读数来源"
            value={proc.source === "footprint" ? "/usr/bin/footprint --swapped" : "sysinfo RSS"}
            mono
          />
        </dl>
        {snapshot?.deepAvailable === false ? (
          <p className="mt-2.5 rounded bg-warn/[0.07] px-2 py-1.5 text-[10px] leading-relaxed text-warn">
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

/**
 * 进程右键菜单：结束进程（SIGTERM）/ 强制结束（SIGKILL）。
 * 破坏性操作走两步确认——先选项、再确认，防误杀。SIGTERM 给应用优雅退出的机会，
 * 卡死的进程才需要 SIGKILL。执行成功后清空选中并刷新一轮采样。
 */
function ProcKillMenu({ target, onClose }: { target: KillTarget; onClose: () => void }) {
  const [phase, setPhase] = useState<"idle" | "confirm" | "confirm-force">("idle");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const refresh = useMemStore((s) => s.refresh);
  const select = useMemStore((s) => s.select);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  // 视口内收口，避免贴边弹出被裁掉
  const x = Math.min(target.x, window.innerWidth - 210);
  const y = Math.min(target.y, window.innerHeight - 140);

  const run = async (force: boolean) => {
    setBusy(true);
    setErr(null);
    try {
      await killProcess(target.pid, force);
      select(null);
      await refresh();
      onClose();
    } catch (e) {
      setErr(String(e));
      setBusy(false);
    }
  };

  return (
    <>
      {/* 全屏透明层：点任意处关闭，同时挡住下层的点击 */}
      <div className="fixed inset-0 z-40" onClick={onClose} onContextMenu={(e) => { e.preventDefault(); onClose(); }} />
      <div
        className="fixed z-50 w-[196px] rounded-lg border border-hairline bg-surface-raised py-1 shadow-2xl shadow-black/50"
        style={{ left: x, top: y }}
      >
        {phase === "idle" ? (
          <>
            <MenuItem
              label="结束进程"
              hint="SIGTERM · 可优雅退出"
              onClick={() => setPhase("confirm")}
            />
            <MenuItem
              label="强制结束"
              hint="SIGKILL · 立即终止"
              tone="danger"
              onClick={() => setPhase("confirm-force")}
            />
          </>
        ) : (
          <div className="px-3 py-1.5">
            <p className="text-[11px] leading-snug text-ink-dim">
              {phase === "confirm" ? "结束" : "强制结束"}
              <span className="mx-1 font-medium text-ink">「{target.name}」</span>
              <span className="font-mono text-[10px] tabular-nums text-ink-faint">
                pid {target.pid} · {target.mem}
              </span>
              ？
            </p>
            <p className="mt-0.5 text-[10px] leading-snug text-ink-faint">
              {phase === "confirm"
                ? "发送 SIGTERM，应用可保存数据后退出"
                : "立即杀死，未保存的数据会丢失"}
            </p>
            {err ? <p className="mt-1 text-[10px] leading-snug text-danger">{err}</p> : null}
            <div className="mt-2 flex gap-1.5">
              <button
                type="button"
                disabled={busy}
                onClick={() => void run(phase === "confirm-force")}
                className="flex-1 rounded-md bg-danger/90 px-2 py-1 text-[10.5px] text-white transition-colors hover:bg-danger disabled:opacity-50"
              >
                {busy ? "执行中…" : "确认"}
              </button>
              <button
                type="button"
                disabled={busy}
                onClick={() => (err ? onClose() : setPhase("idle"))}
                className="flex-1 rounded-md border border-hairline px-2 py-1 text-[10.5px] text-ink-dim transition-colors hover:bg-white/[0.05] hover:text-ink disabled:opacity-50"
              >
                {err ? "关闭" : "取消"}
              </button>
            </div>
          </div>
        )}
      </div>
    </>
  );
}

function MenuItem({
  label,
  hint,
  tone = "default",
  onClick,
}: {
  label: string;
  hint: string;
  tone?: "default" | "danger";
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={`flex w-full flex-col items-start px-3 py-1.5 text-left transition-colors ${
        tone === "danger"
          ? "text-danger hover:bg-danger/[0.08]"
          : "text-ink hover:bg-white/[0.05]"
      }`}
    >
      <span className="text-[11.5px] leading-tight">{label}</span>
      <span className="text-[9.5px] leading-tight text-ink-faint">{hint}</span>
    </button>
  );
}
