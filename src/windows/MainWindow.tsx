import { useEffect, useMemo, useRef, useState } from "react";

import { listen } from "@tauri-apps/api/event";

import { AppResourceBoard } from "../components/AppResourceGrid";
import { AppDetail } from "../components/appdetail/AppDetail";
import { LeakCard, LeakEvidence } from "../components/LeakCard";
import { NetBoard } from "../components/NetBoard";
import { PortBoard } from "../components/PortList";
import { RailNav } from "../components/RailNav";
import { ReclaimBoard } from "../components/ReclaimBoard";
import { SettingsBoard } from "../components/SettingsBoard";
import { Sparkline } from "../components/Sparkline";
import { StackBar } from "../components/charts/StackBar";
import { RingGauge } from "../components/charts/RingGauge";
import { StatsBoard } from "../components/StatsBoard";
import {
  ChevronRightIcon,
  RefreshIcon,
} from "../components/icons";
import { Badge, CompareBar, PressureDot, Stat } from "../components/primitives";
import { formatBytes, formatClock, formatPercent, formatRate, formatRelative, numOf, unitOf } from "../lib/format";
import { isTauri, killProcess } from "../lib/ipc";
import { useDiskStore } from "../stores/diskStore";
import { useMemHistory, useMemStore } from "../stores/memStore";
import { useNetDiagStore } from "../stores/netDiagStore";
import { useNetSpeedStore } from "../stores/netStore";
import { useNetTotalStore } from "../stores/netTotalStore";
import { usePortStore } from "../stores/portStore";
import {
  isExact,
  memSourceLabel,
  swappedRatio,
  totalOf,
  type FamilyMem,
  type ProcMem,
  type WorkspaceSource,
} from "../types/mem";
import { exposureMeta, portsOfPid } from "../types/net";

/**
 * 一级导航（UX v3 方案 A）：8 tab 收敛为 6。
 * - 「统计」更名「总览」并新增可回收主卡（产品主张回归）
 * - 「内存·应用」+「内存·概况」合并为「内存」（列表 + 详情）
 * - 「设置」撤出一级导航，移到标题栏齿轮弹层
 */
type MainTab = "stats" | "resources" | "memory" | "disk" | "net" | "ports";

/** ⌘1–⌘6 的切换顺序（UX v3 §5.5） */
const TAB_ORDER: MainTab[] = ["stats", "resources", "memory", "disk", "net", "ports"];
const TAB_LABEL: Record<MainTab, string> = {
  stats: "总览",
  resources: "应用",
  memory: "内存",
  disk: "存储",
  net: "网络",
  ports: "端口",
};

/**
 * 外部 tab 事件（菜单栏面板 openMain → Rust 广播）的旧 id 映射：
 * 合并前有 "apps" / "overview" 两个内存 tab，合并后统一落「内存」。
 * Rust 侧只转发字符串（commands.rs:open_main），所以协议兼容不需要动 Rust。
 */
function mapExternalTab(payload: string): MainTab | null {
  const t = payload === "port" ? "ports" : payload === "apps" || payload === "overview" ? "memory" : payload;
  return (TAB_ORDER as string[]).includes(t) ? (t as MainTab) : null;
}

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

/** 主窗口（960×640）：总览 / 应用 / 内存 / 存储 / 网络 / 端口 + 设置齿轮 */
export function MainWindow() {
  // M9：总览是默认页——打开先看本机整体，再下钻到具体域（v3.1 起首屏含可回收主卡）
  const [tab, setTab] = useState<MainTab>("stats");
  /** 下钻来源 tab：存在时标题栏显示「← 返回{来源}」（UX v3 §4.2） */
  const [backTo, setBackTo] = useState<MainTab | null>(null);
  /** 设置弹层（齿轮）开关 */
  const [settingsOpen, setSettingsOpen] = useState(false);
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
  const portRefreshing = usePortStore((s) => s.refreshing);
  const ports = usePortStore((s) => s.snapshot);
  const initPorts = usePortStore((s) => s.init);
  const refreshPorts = usePortStore((s) => s.refresh);
  const setPortActive = usePortStore((s) => s.setActive);
  const setPortFocus = usePortStore((s) => s.setFocus);
  const hydratePorts = usePortStore((s) => s.hydrate);
  const netSample = useNetSpeedStore((s) => s.sample);
  const initNetSpeed = useNetSpeedStore((s) => s.init);
  const setNetActive = useNetSpeedStore((s) => s.setActive);
  const initNetTotal = useNetTotalStore((s) => s.init);
  const setNetTotalActive = useNetTotalStore((s) => s.setActive);
  const refreshNetTotal = useNetTotalStore((s) => s.refresh);
  const netTotalRefreshing = useNetTotalStore((s) => s.refreshing);

  const [expanded, setExpanded] = useState<string | null>(null);
  /** D2：当前打开的应用详情（null = 列表） */
  const [appDetail, setAppDetail] = useState<string | null>(null);
  const [killTarget, setKillTarget] = useState<KillTarget | null>(null);
  /** 统计 tab 的卷容量重取信号（内存/端口由各自 store 刷新，不重复驱动） */
  const [statsNonce, setStatsNonce] = useState(0);
  /** 应用资源 tab 的磁盘本体重测信号 */
  const [resourcesNonce, setResourcesNonce] = useState(0);

  /** 右键进程行 → 弹「结束进程」菜单（位置取鼠标点） */
  const openKillMenu = (proc: ProcMem, x: number, y: number) => {
    setKillTarget({ pid: proc.pid, name: proc.name, mem: formatBytes(totalOf(proc)), x, y });
  };

  /** 统一导航入口：记录下钻来源，供「返回」使用 */
  const go = (next: MainTab, from?: MainTab) => {
    setTab(next);
    setBackTo(from ?? null);
    // 离开「应用」页就退出详情：回来时回到列表（与「返回」按钮语义一致）
    if (next !== "resources") setAppDetail(null);
  };

  // 面板点「可回收」/「端口」→ 主窗口直接切到对应 tab（open_main 会让 Rust 侧广播 ui://tab）
  useEffect(() => {
    if (!isTauri()) return;
    const un = listen<string>("ui://tab", (e) => {
      const t = mapExternalTab(e.payload);
      if (t) {
        setTab(t);
        setBackTo(null);
      }
    });
    return () => {
      void un.then((fn) => fn());
    };
  }, []);

  /**
   * 端口 tab 的可见性门控（方案 §5.2）：只要用户不在这个 tab 上，
   * 定时器就完全不扫——常驻开销 0 是验收项。
   */
  useEffect(() => {
    if (tab !== "ports") {
      void setPortActive(false);
      return;
    }
    void initPorts();
    void setPortActive(true);
    return () => {
      void setPortActive(false);
    };
  }, [tab, initPorts, setPortActive]);

  // 内存页的应用列表要挂端口徽标，但只读已有快照：不为了一枚徽标在启动时跑 lsof
  useEffect(() => {
    if (tab === "memory") void hydratePorts();
  }, [tab, hydratePorts]);

  /**
   * 统计 tab 的两个「只读依赖」：
   * - 端口：一次性扫描——`init` 内部有快照就只读、没有才扫一轮。这里**不调
   *   setActive**，30 s 定时轮仍然只属于端口 tab，统计页的常驻开销为 0。
   * - 网速：只 `init`（订阅 net://total + 拉一次快照/历史），**不提速**；
   *   `setActive` 仍然只属于「网络」tab。少了这一步，统计页的网络卡会永远停在
   *   「采样中…」——`initNetTotal` 的唯一调用点本来在下面的 net 分支里。
   *   网络诊断（net-diag-plan）同场初始化：诊断是手动短任务，init 只拉 capability，
   *   不起任何采样循环。
   */
  const initNetDiag = useNetDiagStore((s) => s.init);
  useEffect(() => {
    if (tab !== "stats") return;
    void initPorts();
    void initNetTotal();
  }, [tab, initPorts, initNetTotal]);

  // 应用资源 tab 的资源卡要挂端口徽标：一次性 init，不挂 30 s 定时轮
  useEffect(() => {
    if (tab === "resources") void initPorts();
  }, [tab, initPorts]);

  /**
   * 按进程网速（nettop）的可见性门控。
   *
   * M14 起「应用资源」与「网络」两个 tab 都要这份数据（网络 tab 的「按应用」
   * 区块与资源卡同源），所以**只能有一个 effect 管这一个开关**：拆成两个
   * effect 各自 `setActive(false)`，会在切 tab 时互相把对方关掉。
   *
   * 1 s 一次 nettop 只在用户真的看着这两个 tab 时才跑，其它 tab 连子进程都不 spawn。
   */
  useEffect(() => {
    const visible = tab === "resources" || tab === "net";
    if (!visible) {
      void setNetActive(false);
      return;
    }
    void initNetSpeed();
    void setNetActive(true);
    return () => {
      void setNetActive(false);
    };
  }, [tab, initNetSpeed, setNetActive]);

  /**
   * 系统口径总网速的门控：进入网络 tab 只**提速**（2 s → 1 s）并补齐走势与 7 日柱。
   * 采样循环本身常驻（菜单栏数字要用），所以切走不清基线、不停止采样。
   */
  useEffect(() => {
    if (tab !== "net") {
      void setNetTotalActive(false);
      return;
    }
    void initNetTotal();
    void initNetDiag(); // 诊断 capability：拉一次，不起采样循环
    void setNetTotalActive(true);
    return () => {
      void setNetTotalActive(false);
    };
  }, [tab, initNetTotal, setNetTotalActive, initNetDiag]);

  const families = snapshot?.families ?? [];
  const selected = useMemo(
    () => processes.find((p) => p.pid === selectedPid) ?? null,
    [processes, selectedPid],
  );

  // 刷新按钮按 tab 分流：三个域的采样代价与语义都不同，不能混用一个 loading
  const busy =
    tab === "disk"
      ? diskScanning
      : tab === "ports"
        ? portRefreshing
        : tab === "net"
          ? netTotalRefreshing
          : tab === "stats"
            ? refreshing || portRefreshing || netTotalRefreshing
            : refreshing;
  const onRefresh = () => {
    if (tab === "disk") void refreshDisk();
    else if (tab === "ports") void refreshPorts();
    else if (tab === "net") {
      // 网络 tab 的「刷新」= 重取快照 + 走势 + 7 日柱，不打断后台采样
      void refreshNetTotal();
    } else if (tab === "stats") {
      // 四个域各刷一次：内存常采、卷容量按需取、端口显式重扫一轮（用户点了就该扫）、
      // 网速重取快照与走势（只读，不改采样节拍）
      void refresh();
      void refreshPorts();
      void refreshNetTotal();
      setStatsNonce((n) => n + 1);
    } else if (tab === "resources") {
      // 网速不用手动刷（1 s 自己跳）；这里刷内存 + 端口 + 强制重测磁盘本体
      void refresh();
      void refreshPorts();
      setResourcesNonce((n) => n + 1);
    } else void refresh();
  };
  const refreshLabel =
    tab === "disk"
      ? diskScanning
        ? "扫描中"
        : "重新扫描"
      : tab === "ports"
        ? portRefreshing
          ? "扫描中"
          : "扫描端口"
        : tab === "net"
          ? netTotalRefreshing
            ? "刷新中"
            : "刷新"
          : refreshing
            ? "采样中"
            : "刷新";

  // 快捷键（UX v3 §5.5）：⌘1–6 切 tab · ⌘R 刷新 · ⌘F 聚焦搜索 · Escape 关弹层/取消选中。
  // onRefresh 每次渲染都是新函数，用 ref 持有避免反复解绑。
  const refreshRef = useRef(onRefresh);
  refreshRef.current = onRefresh;
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const meta = e.metaKey || e.ctrlKey;
      if (meta && e.key >= "1" && e.key <= "6") {
        e.preventDefault();
        const next = TAB_ORDER[Number(e.key) - 1];
        if (next) {
          setTab(next);
          setBackTo(null);
        }
      } else if (meta && (e.key === "r" || e.key === "R")) {
        e.preventDefault();
        refreshRef.current();
      } else if (meta && (e.key === "f" || e.key === "F")) {
        e.preventDefault();
        window.dispatchEvent(new CustomEvent("prune:focus-search"));
      } else if (e.key === "Escape") {
        if (settingsOpen) setSettingsOpen(false);
        else if (tab === "memory" && selectedPid != null) select(null);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [settingsOpen, tab, selectedPid, select]);

  return (
    <div className="flex h-screen bg-surface text-ink">
      {/* UI v4：左侧 56px 图标 rail（§4.1），设置齿轮固定 rail 底部 */}
      <RailNav value={tab} onChange={(t) => go(t)} onOpenSettings={() => setSettingsOpen(true)} />

      <div className="flex min-w-0 flex-1 flex-col">
        <header className="flex h-12 shrink-0 items-center gap-3 border-b border-hairline px-5">
          <span className="text-[13px] font-semibold tracking-tight">{TAB_LABEL[tab]}</span>
          <div className="flex-1" />
          {/* 下钻返回（UX v3 §4.2）：所有跨页跳转必须可一键回来源 */}
          {backTo && backTo !== tab ? (
            <button
              type="button"
              onClick={() => go(backTo)}
              className="rounded-md bg-white/[0.05] px-2.5 py-1.5 text-[11px] text-ink-dim transition-colors hover:bg-white/[0.09] hover:text-ink"
            >
              ← 返回{TAB_LABEL[backTo]}
            </button>
          ) : null}
          {tab === "memory" ? (
            <>
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
            </>
          ) : null}
          <button
            type="button"
            onClick={onRefresh}
            disabled={busy}
            className="flex items-center gap-1.5 rounded-md bg-white/[0.05] px-2.5 py-1.5 text-[11px] text-ink-dim transition-colors hover:bg-white/[0.09] hover:text-ink disabled:opacity-50"
          >
            <RefreshIcon size={12} className={busy ? "animate-spin" : ""} />
            {refreshLabel}
          </button>
        </header>

        {error ? (
          <div className="mx-5 mt-3 rounded-lg bg-danger/[0.08] px-3 py-2 text-[11px] text-danger">
            {error}
          </div>
        ) : null}

        {tab === "stats" ? (
          <StatsBoard onDrill={(t) => go(t, "stats")} refreshNonce={statsNonce} />
        ) : tab === "resources" ? (
          appDetail ? (
            <AppDetail
              familyName={appDetail}
              onBack={() => setAppDetail(null)}
              onOpenPorts={(port) => {
                setPortFocus(String(port));
                go("ports", "resources");
              }}
              onOpenProcess={(pid) => {
                select(pid);
                go("memory", "resources");
              }}
              onKillMenu={openKillMenu}
            />
          ) : (
            <AppResourceBoard
              families={families}
              processes={processes}
              ports={ports}
              net={netSample}
              refreshNonce={resourcesNonce}
              onOpenApp={(name) => setAppDetail(name)}
            />
          )
        ) : tab === "ports" ? (
          <PortBoard
            onOpenProcess={(pid) => {
              select(pid);
              go("memory", "ports");
            }}
            onKill={(binding, port, x, y) => {
              setKillTarget({
                pid: binding.pid ?? 0,
                name: binding.comm,
                mem: `端口 ${port}`,
                x,
                y,
              });
            }}
          />
        ) : tab === "net" ? (
          <NetBoard onOpenResources={() => go("resources", "net")} />
        ) : tab === "disk" ? (
          <ReclaimBoard />
        ) : (
          // 内存页（UX v3 §6.3）：左栏家族列表 + 右栏（默认概况，选中进程后为详情）
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
                <ProcessInspector proc={selected} onBack={() => select(null)} />
              ) : (
                <div className="space-y-4">
                  {/* 泄漏卡置顶（命中或「无法判定」时才出现；没有命中就是沉默即正常） */}
                  {snapshot?.leaks &&
                  (snapshot.leaks.items.length > 0 || snapshot.leaks.note) ? (
                    <LeakCard
                      report={snapshot.leaks}
                      onOpen={(pid) => select(pid)}
                    />
                  ) : null}
                  <Overview onKillMenu={openKillMenu} />
                </div>
              )}
            </main>
          </div>
        )}

        {settingsOpen ? (
          <div
            className="fixed inset-0 z-40 bg-black/40"
            onClick={() => setSettingsOpen(false)}
            onContextMenu={(e) => {
              e.preventDefault();
              setSettingsOpen(false);
            }}
          >
            <div
              className="absolute right-4 top-14 max-h-[calc(100vh-72px)] w-[480px] overflow-y-auto rounded-xl border border-hairline bg-surface-raised shadow-2xl shadow-black/50"
              onClick={(e) => e.stopPropagation()}
            >
              <SettingsBoard />
            </div>
          </div>
        ) : null}

        {killTarget ? <ProcKillMenu target={killTarget} onClose={() => setKillTarget(null)} /> : null}
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

  const { swap, pressure, thrash, compressorBytes, freeBytes, physicalTotal, topSwapped } = snapshot;
  const swapRatio = swap.total > 0 ? swap.used / swap.total : 0;
  // 换出口径只在有换页计数的平台上存在；Windows 没有 → 整块隐藏，而不是显示「没有换出」
  const hasSwapMetrics = thrash != null;
  const swapHint =
    swap.fileCount == null
      ? `${formatBytes(swap.used)} / ${formatBytes(swap.total)} · pagefile`
      : `${formatBytes(swap.used)} / ${formatBytes(swap.total)} · ${swap.fileCount} 个 swapfile · 占用存储见「存储」tab`;

  return (
    <div className="space-y-4">
      <div>
        <h1 className="text-[15px] font-semibold tracking-tight">内存概况</h1>
        <p className="mt-0.5 text-[11px] text-ink-faint">
          内存域系统级读数 · 点击左侧家族展开进程 · 磁盘读数见「存储」tab
        </p>
      </div>

      {/* UI v4（§6.3）：左统计 + 右压力环。环比值口径随平台 metrics 不同，
          caption 写明换算，不冒充「占用率」 */}
      <div className="flex items-start gap-6 border-b border-hairline-soft pb-5">
        <div className="grid min-w-0 flex-1 grid-cols-2 gap-x-8 gap-y-5">
          <Stat label="物理内存" value={numOf(physicalTotal)} unit={unitOf(physicalTotal)} />
          <Stat
            label="已用"
            value={formatBytes(snapshot.physicalUsedBytes ?? 0)}
            hint={physicalTotal > 0 ? formatPercent((snapshot.physicalUsedBytes ?? 0) / physicalTotal) : undefined}
          />
          <Stat label="可用" value={formatBytes(freeBytes)} hint={freeBytesHint(compressorBytes)} />
          {compressorBytes != null ? (
            <Stat label="压缩内存" value={formatBytes(compressorBytes)} hint="vm_stat 压缩页" />
          ) : null}
          <Stat
            label="swap 已用"
            value={formatPercent(swapRatio)}
            tone={swapRatio >= 0.9 ? "danger" : swapRatio >= 0.75 ? "warn" : "accent"}
            hint={swapHint}
          />
        </div>
        {pressure ? <PressureRing pressure={pressure} /> : null}
      </div>

      {swap.total > 0 ? (
        <div className="border-b border-hairline-soft pb-5" title="轨道渐变即阈值：过 75% 转黄、过 90% 转红；swap 配置为 0 时不画">
          {/* UI v4 swap 渐变科技条：阈值直接画在轨道色带上（§6.3） */}
          <div className="flex items-baseline justify-between">
            <span className="text-[11px] text-ink-dim">swap 占用</span>
            <span className="font-mono text-[11px] tabular-nums text-ink-dim">
              {formatBytes(swap.used)} / {formatBytes(swap.total)}
              {swap.fileCount != null ? ` · ${swap.fileCount} 个 swapfile` : " · pagefile"}
            </span>
          </div>
          <div className="relative mt-2 h-2.5 overflow-hidden rounded-full bg-white/[0.05]">
            {/* 阈值色带：0–75% 绿 → 75–90% 黄 → 90%+ 红，固定铺满轨道 */}
            <div
              className="absolute inset-0"
              style={{
                background:
                  "linear-gradient(90deg, var(--color-accent) 0%, var(--color-accent) 75%, var(--color-warn) 75%, var(--color-warn) 90%, var(--color-danger) 90%)",
                opacity: 0.28,
              }}
            />
            {/* 实际占用填充 */}
            <div
              className="prune-anim absolute inset-y-0 left-0 rounded-full transition-[width] duration-500"
              style={{
                width: `${Math.min(100, swapRatio * 100)}%`,
                background:
                  swapRatio >= 0.9
                    ? "var(--color-danger)"
                    : swapRatio >= 0.75
                      ? "var(--color-warn)"
                      : "var(--color-accent)",
                boxShadow:
                  swapRatio >= 0.75
                    ? "0 0 8px color-mix(in srgb, var(--color-warn) 45%, transparent)"
                    : undefined,
              }}
            />
            <div
              className="absolute -inset-y-0.5 w-px bg-ink-faint/80"
              style={{ left: "75%" }}
              title="预警阈值 75%"
            />
            <div
              className="absolute -inset-y-0.5 w-px bg-ink-faint/80"
              style={{ left: "90%" }}
              title="危险阈值 90%"
            />
          </div>
          <div className="mt-1 flex items-baseline justify-between text-[9.5px] text-ink-faint">
            <span>{formatPercent(swapRatio)} 已用</span>
            <span>色带阈值 75% / 90%（竖刻度）</span>
          </div>
        </div>
      ) : null}

      {pressure ? (
        <section className="border-b border-hairline-soft pb-5">
          <div className="flex items-center justify-between">
            <span className="text-[11px] text-ink-dim">内存压力</span>
            <PressureDot level={pressure.level} />
          </div>
          <div className="mt-2.5 flex items-center justify-between text-[10px] text-ink-faint">
            <span>{pressureCaption(pressure)}</span>
            {swap.newToday != null ? (
              <span>
                今日新增 swapfile <span className="font-mono text-ink-dim">+{swap.newToday}</span>
              </span>
            ) : null}
          </div>
          {hasSwapMetrics ? (
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
          ) : null}
        </section>
      ) : null}

      {hasSwapMetrics ? (
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
      ) : null}

      {!snapshot.deepAvailable ? (
        <p className="mt-4 rounded-lg bg-warn/[0.07] px-3 py-2 text-[11px] leading-relaxed text-warn">
          深探不可用：当前全部为 RSS 口径。macOS 请确认 /usr/bin/footprint 可执行；
          Windows 通常是该进程受保护或权限不足。
        </p>
      ) : null}
    </div>
  );
}

/** 「可用」的补注：macOS 给的是 Pages free，Windows 是 AvailPhys，语义不同别混着说 */
function freeBytesHint(compressorBytes: number | null): string {
  return compressorBytes == null ? "系统可用物理内存" : "Pages free";
}

/**
 * 内存压力环（UI v4 §6.3）：黄=Warning / 红=Critical 发光弧。
 * 比值口径随平台 metrics 不同：occupancy 是占用率（raw/100），
 * kernel 是 1/2/4 档（raw/4）——caption 写明换算，不冒充占用率。
 */
function PressureRing({
  pressure,
}: {
  pressure: { raw: number; metric: "kernel" | "occupancy" };
}) {
  const ratio = pressure.metric === "occupancy" ? pressure.raw / 100 : pressure.raw / 4;
  const critical = pressure.metric === "occupancy" ? pressure.raw >= 92 : pressure.raw >= 4;
  const warning = !critical && (pressure.metric === "occupancy" ? pressure.raw >= 80 : pressure.raw >= 2);
  const color = critical
    ? "var(--color-danger)"
    : warning
      ? "var(--color-warn)"
      : "var(--color-accent)";

  return (
    <div className="shrink-0" title={pressureCaption(pressure)}>
      <RingGauge
        segments={[{ value: Math.max(0.04, Math.min(1, ratio)), color }]}
        size={104}
        stroke={9}
        ariaLabel={`内存压力：${pressureCaption(pressure)}`}
      >
        <div className="text-center">
          <span
            className="block font-mono text-[20px] leading-none tabular-nums"
            style={{ color, textShadow: `0 0 12px color-mix(in srgb, ${color} 45%, transparent)` }}
          >
            {Math.round(Math.min(1, ratio) * 100)}
          </span>
          <span className="mt-0.5 block text-[9px] text-ink-faint">
            {pressure.metric === "occupancy" ? "占用 %" : "内核档"}
          </span>
        </div>
      </RingGauge>
    </div>
  );
}

/** 压力行的原始值说明：两个平台的 raw 含义完全不同 */
function pressureCaption(pressure: { raw: number; metric: "kernel" | "occupancy" }): string {
  if (pressure.metric === "occupancy") {
    return `内存占用率 ${pressure.raw}%（<80 正常 / 80–91 警告 / ≥92 紧张）`;
  }
  return `内核原始值 ${pressure.raw}（1 正常 / 2 警告 / 4 紧张）`;
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
        className="w-full rounded-md px-1 py-1.5 text-left transition-colors hover:bg-white/[0.04]"
      >
        <div className="flex w-full items-center gap-2">
          <span className="min-w-0 flex-1 truncate text-[11.5px] text-ink">{proc.name}</span>
          <span className="shrink-0 font-mono text-[11px] tabular-nums text-warn">
            {formatBytes(proc.swapped ?? 0)}
          </span>
          <span className="w-14 shrink-0 text-right text-[10px] text-ink-faint">
            {ratio != null ? formatPercent(ratio) : "—"}
          </span>
        </div>
        {/* UI v4（§6.3）：换出占比红橙渐变条——「大部分在磁盘上」一眼可见 */}
        {ratio != null && ratio > 0 ? (
          <div className="mt-1 h-[3px] w-full overflow-hidden rounded-full bg-white/[0.06]" aria-hidden>
            <div
              className="prune-anim h-full rounded-full transition-[width] duration-500"
              style={{
                width: `${Math.min(100, ratio * 100)}%`,
                background: "linear-gradient(90deg, var(--color-warn), var(--color-danger))",
              }}
            />
          </div>
        ) : null}
      </button>
    </li>
  );
}

function ProcessInspector({ proc, onBack }: { proc: ProcMem; onBack: () => void }) {
  const points = useMemHistory(proc.pid, 240);
  const snapshot = useMemStore((s) => s.snapshot);
  const ports = usePortStore((s) => s.snapshot);
  const listenPorts = useMemo(
    () => (ports ? portsOfPid(ports.entries, proc.pid) : []),
    [ports, proc.pid],
  );
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
      <div>
        <button
          type="button"
          onClick={onBack}
          className="text-[11px] text-accent transition-colors hover:text-ink"
        >
          ← 返回概况
        </button>
      </div>
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

      {/* 构成堆叠条（UX v3 §7.6）：footprint = RSS + 换出，一眼看出「多少在磁盘上」。
          未深探时没有可靠的换出拆分，整条不渲染——不画假数据。 */}
      {footprint != null && footprint > 0 ? (
        <section>
          <div className="mb-2 flex items-baseline justify-between">
            <span className="text-[11px] font-medium text-ink-dim">内存构成一览</span>
            <span className="text-[10px] text-ink-faint">
              {ratio != null ? `${formatPercent(ratio)} 在磁盘上` : ""}
            </span>
          </div>
          <StackBar
            height={6}
            ariaLabel={`内存构成：RSS ${formatBytes(rss)}，已换出到磁盘 ${formatBytes(swapped)}，footprint 合计 ${formatBytes(footprint)}`}
            segments={[
              { key: "rss", label: "RSS（物理常驻）", value: rss, tone: "neutral" },
              { key: "swapped", label: "已换出到磁盘", value: swapped, tone: "warn" },
            ]}
            total={footprint}
          />
        </section>
      ) : null}

      <section className="border-b border-hairline-soft pb-4">
        <div className="mb-2 flex items-baseline justify-between">
          <span className="text-[11px] font-medium text-ink-dim">监听端口</span>
          <span className="text-[10px] text-ink-faint">
            {ports ? `端口快照 ${formatClock(ports.at)}` : "尚未扫描端口"}
          </span>
        </div>
        {!ports ? (
          <p className="text-[10.5px] leading-relaxed text-ink-faint">
            还没扫过端口。这里为空不代表该进程没有监听——打开「端口」tab 扫一轮再看。
          </p>
        ) : listenPorts.length === 0 ? (
          <p className="text-[10.5px] leading-relaxed text-ink-faint">
            最近一次扫描里没有该进程的监听端口。
          </p>
        ) : (
          <ul className="flex flex-wrap gap-1.5">
            {listenPorts.map((e) => {
              const exposure = exposureMeta(e.exposure);
              return (
                <li key={`${e.port}-${e.proto}`}>
                  <Badge tone={exposure.tone} title={`${exposure.label} · ${exposure.hint}`}>
                    <span className="font-mono tabular-nums">{e.port}</span>
                    <span className="text-[9px] uppercase opacity-70">{e.proto}</span>
                  </Badge>
                </li>
              );
            })}
          </ul>
        )}
      </section>

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
            value={memSourceLabel(proc.source)}
            mono
          />
        </dl>
        {snapshot?.deepAvailable === false ? (
          <p className="mt-2.5 rounded bg-warn/[0.07] px-2 py-1.5 text-[10px] leading-relaxed text-warn">
            深探不可用，当前为 RSS 口径。
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
  const refreshPorts = usePortStore((s) => s.refresh);

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
      // 从端口 tab 结束进程后，端口必须重扫——否则用户会看到一个已死的 pid 还占着 3000
      await refreshPorts();
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
