import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";

import { CellBar } from "./CellBar";
import { Sparkline } from "./Sparkline";
import { StackBar } from "./charts/StackBar";
import { ChevronRightIcon } from "./icons";
import { devAppDisk, devBundles } from "../lib/devFixture";
import { formatBytes } from "../lib/format";
import { appBundles, appDisk, isTauri } from "../lib/ipc";
import {
  BUNDLE_READONLY_HINT,
  CACHE_CLEAN_HINT,
  DATA_READONLY_HINT,
  diskEmpty,
  diskTotal,
  type AppDisk,
  type BundleMap,
  type BundleSize,
} from "../types/appdisk";
import { isExact, totalOf, type FamilyMem, type ProcMem } from "../types/mem";
import {
  connsOfPids,
  exposureMeta,
  formatSpeed,
  netSpeedOfPids,
  type ConnStats,
  type NetSpeedSample,
  type PortEntry,
  type PortSnapshot,
  type Speed,
} from "../types/net";

/** 默认只渲染前 12 张卡：48 张要滚 8 屏，没人会看完（方案 §4.3） */
const DEFAULT_VISIBLE = 12;

/** 进程数到这个量级就标警示色：Chrome / Electron 家族的典型「进程爆炸」信号 */
const PROC_WARN = 20;

type SortKey = "mem" | "cpu" | "procs" | "disk" | "ports" | "down";

/** UI v4：排序 chip 带域色点——颜色即语义（§3.2），mem 蓝 / cpu 紫 / net 青 */
const SORTS: { key: SortKey; label: string; dot?: string }[] = [
  { key: "mem", label: "内存", dot: "var(--color-info)" },
  { key: "cpu", label: "CPU", dot: "var(--color-domain-cpu)" },
  { key: "procs", label: "进程" },
  { key: "disk", label: "磁盘" },
  { key: "ports", label: "端口" },
  { key: "down", label: "下载", dot: "var(--color-domain-net)" },
];

interface Row {
  family: FamilyMem;
  members: ProcMem[];
  pids: number[];
  mem: number;
  cpu: number | null;
  procs: number;
  disk: BundleSize | null;
  ports: PortEntry[];
  exposed: number;
  conns: ConnStats | null;
  net: Speed | null;
}

/** 行装配：家族 × 成员 × 端口 × 连接 × 网速。主窗口网格与面板卡片共用同一口径。 */
function buildRows(
  families: FamilyMem[],
  processes: ProcMem[],
  ports: PortSnapshot | null,
  bundles: BundleMap | null,
  net: NetSpeedSample | null,
): Row[] {
  const byFamily = new Map<string, ProcMem[]>();
  for (const p of processes) {
    const arr = byFamily.get(p.family);
    if (arr) arr.push(p);
    else byFamily.set(p.family, [p]);
  }

  return families.map((f) => {
    const members = byFamily.get(f.name) ?? [];
    const pids = members.map((m) => m.pid);
    const watched = ports
      ? ports.entries.filter(
          (e) => e.watched && e.listeners.some((b) => b.pid != null && pids.includes(b.pid)),
        )
      : [];
    const speed = net && net.status === "ok" ? netSpeedOfPids(net, pids) : null;
    return {
      family: f,
      members,
      pids,
      mem: totalOf(f),
      cpu: f.cpu ?? null,
      procs: f.processCount,
      disk: bundles ? (bundles[f.name] ?? null) : null,
      ports: watched,
      exposed: watched.filter((e) => e.exposure === "any").length,
      conns: ports ? connsOfPids(ports.connsByPid, pids) : null,
      net: speed,
    };
  });
}

/** 单核口径的短格式：卡片里放不下「188%（约 1.9 核）」 */
function cpuShort(v: number | null): string {
  if (v == null || !Number.isFinite(v)) return "—";
  if (v < 1) return "<1%";
  return `${Math.round(v)}%`;
}

/** 值列专用短格式：单位只留一个尾缀（`↓3.8k ↑120 B/s`），完整读数在 hover 里 */
function compactSpeed(bps: number): string {
  if (!Number.isFinite(bps) || bps < 1) return "0";
  const units = ["B", "k", "M", "G"];
  let v = bps;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i += 1;
  }
  return `${v >= 100 ? Math.round(v) : v.toFixed(1).replace(/\.0$/, "")}${units[i]}`;
}

/**
 * 应用资源板（M10）：每个应用一张卡，横向对比进程 / CPU / 内存 / 磁盘 / 端口 / 网速。
 *
 * 为什么是卡片而不是表格（方案 §1.2）：卡片能同时装下徽标、条、走势；
 * 代价是失去列对齐，所以所有卡强制同高、指标行序固定，且**条一律用
 * 「全表该列最大值」做分母**——否则每张卡各算各的刻度，条就成了装饰。
 *
 * 折叠不是过滤：`showAll` 只影响渲染几张，分母、合计、spotlight 始终按全部应用算。
 */
export function AppResourceBoard({
  families,
  processes,
  ports,
  net,
  refreshNonce = 0,
  onOpenPorts,
  onOpenProcess,
}: {
  families: FamilyMem[];
  processes: ProcMem[];
  ports: PortSnapshot | null;
  net: NetSpeedSample | null;
  /** 父级刷新按钮的计数：变化时强制重测磁盘本体 */
  refreshNonce?: number;
  onOpenPorts?: (port: number) => void;
  onOpenProcess?: (pid: number) => void;
}) {
  const [sortKey, setSortKey] = useState<SortKey>("mem");
  const [desc, setDesc] = useState(true);
  const [query, setQuery] = useState("");
  // ⌘F 聚焦搜索框（UX v3 §5.5）：主窗口全局派发 prune:focus-search
  const searchRef = useRef<HTMLInputElement>(null);
  useEffect(() => {
    const onFocus = () => searchRef.current?.focus();
    window.addEventListener("prune:focus-search", onFocus);
    return () => window.removeEventListener("prune:focus-search", onFocus);
  }, []);
  const [showAll, setShowAll] = useState(false);
  const [expanded, setExpanded] = useState<string | null>(null);
  const [bundles, setBundles] = useState<BundleMap | null>(null);
  const [bundleBusy, setBundleBusy] = useState(false);

  /** 每个家族的 60 s 网速走势（只存在内存里，切走 tab 由父级卸载组件自然清空） */
  const historyRef = useRef<Map<string, number[]>>(new Map());
  /** 上一次已入图的样本对象：rows 变化不该让同一个样本被重复 push */
  const lastSampleRef = useRef<NetSpeedSample | null>(null);

  const loadBundles = useCallback(
    async (force: boolean) => {
      if (bundleBusy) return;
      setBundleBusy(true);
      try {
        if (!isTauri()) {
          setBundles(devBundles());
          return;
        }
        const items = families.map((f) => ({
          family: f.name,
          exe: processes.find((p) => p.family === f.name && p.exe)?.exe ?? null,
        }));
        setBundles(await appBundles(items, force));
      } catch {
        // 测不出就是「—」：不编数字，也不弹错（磁盘是次要信息，不该打断对比）
        setBundles({});
      } finally {
        setBundleBusy(false);
      }
    },
    // families/processes 每轮快照都会换引用，不能进依赖，否则会反复触发 du
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [bundleBusy, refreshNonce],
  );

  // 首次出家族数据后测一次；之后只在「刷新按钮」或「家族数量变了」时再测。
  // 家族数量变化要重测的理由：新启动的应用在缓存里没有条目，不补测就永远是「—」。
  // 缓存命中的部分不会重新 du，所以这次调用很便宜。
  const measuredNonce = useRef<number | null>(null);
  const measuredCount = useRef(0);
  useEffect(() => {
    if (families.length === 0) return;
    if (measuredNonce.current === refreshNonce && measuredCount.current === families.length) return;
    const force = measuredNonce.current != null && measuredNonce.current !== refreshNonce;
    measuredNonce.current = refreshNonce;
    measuredCount.current = families.length;
    void loadBundles(force);
  }, [families.length, refreshNonce, loadBundles]);

  const rows = useMemo<Row[]>(
    () => buildRows(families, processes, ports, bundles, net),
    [families, processes, ports, bundles, net],
  );

  // 网速走势：只保留最近 60 个样本（60 s）。切走 tab 组件卸载即清空，不落盘。
  useEffect(() => {
    if (!net || net.status !== "ok") return;
    // rows 会随 bundles/ports 变化重算，这里按样本对象去重，避免同一秒 push 两次
    if (lastSampleRef.current === net) return;
    lastSampleRef.current = net;
    for (const r of rows) {
      const arr = historyRef.current.get(r.family.name) ?? [];
      arr.push(r.net?.downBps ?? 0);
      if (arr.length > 60) arr.shift();
      historyRef.current.set(r.family.name, arr);
    }
  }, [net, rows]);

  // 浏览器走查：没有真实采样，用一条假走势让 sparkline 的结构可见（仅 DEV + 非 Tauri）
  useEffect(() => {
    if (isTauri() || families.length === 0) return;
    for (const f of families) {
      if (historyRef.current.has(f.name)) continue;
      historyRef.current.set(
        f.name,
        Array.from({ length: 30 }, (_, i) => Math.abs(Math.sin(i / 3.2)) * 240_000),
      );
    }
  }, [families]);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    const base = q ? rows.filter((r) => r.family.name.toLowerCase().includes(q)) : rows;
    const value = (r: Row): number | null => {
      switch (sortKey) {
        case "mem":
          return r.mem;
        case "cpu":
          return r.cpu;
        case "procs":
          return r.procs;
        case "disk":
          return r.disk?.bytes ?? null;
        case "ports":
          return r.ports.length;
        case "down":
          return r.net?.downBps ?? null;
      }
    };
    return [...base].sort((a, b) => {
      const va = value(a);
      const vb = value(b);
      // 「没有数据」永远排在最后：不管升序还是降序，缺值都不该冒充最小值
      if (va == null && vb == null) return a.family.name.localeCompare(b.family.name);
      if (va == null) return 1;
      if (vb == null) return -1;
      if (va === vb) return a.family.name.localeCompare(b.family.name);
      return desc ? vb - va : va - vb;
    });
  }, [rows, query, sortKey, desc]);

  /**
   * 比例条分母：只看「用户过滤了什么」，不看「屏幕渲染了几张」。
   * 折叠状态下仍用全部应用的最大值——否则点开剩余时全屏条长会缩水，
   * 看起来像数据变了。
   */
  const denom = useMemo(() => {
    const base = query.trim() ? filtered : rows;
    const max = (pick: (r: Row) => number | null) =>
      base.reduce((m, r) => Math.max(m, pick(r) ?? 0), 0);
    return {
      mem: max((r) => r.mem),
      cpu: max((r) => r.cpu),
      disk: max((r) => r.disk?.bytes ?? null),
      down: max((r) => r.net?.downBps ?? null),
      procs: max((r) => r.procs),
    };
  }, [rows, filtered, query]);

  const totalProcs = useMemo(() => rows.reduce((n, r) => n + r.procs, 0), [rows]);
  const totalDown = net?.status === "ok" ? net.totalDownBps : null;
  const totalUp = net?.status === "ok" ? net.totalUpBps : null;

  const searching = query.trim().length > 0;
  const visible = searching || showAll ? filtered : filtered.slice(0, DEFAULT_VISIBLE);
  const remaining = filtered.length - DEFAULT_VISIBLE;
  const gated = !searching && filtered.length > DEFAULT_VISIBLE;

  /**
   * 条形基准标注（§7.6 拍板项）：denom 就是「该指标全体应用的最大值」——
   * 当前排序字段的 Top1 满条、其余按差距缩短，跨排序不可比（切排序基准跟着换）。
   * 端口是计数没有比例条，不标。
   */
  const benchText = (() => {
    const v = sortKey === "ports" ? null : denom[sortKey];
    if (v == null || v <= 0) return null;
    switch (sortKey) {
      case "mem":
      case "disk":
        return formatBytes(v);
      case "down":
        return formatSpeed(v);
      case "cpu":
        return `${Math.round(v)}%`;
      case "procs":
        return `${v} 个进程`;
      default:
        return null;
    }
  })();

  /** 大户 spotlight：只在「默认视图」出现，跟当前排序说的一致才不算自相矛盾 */
  const spotlight = !searching && sortKey === "mem" && desc && rows.length > 0;
  const top = (pick: (r: Row) => number | null) =>
    rows.reduce<Row | null>((best, r) => {
      const v = pick(r);
      if (v == null) return best;
      const b = best ? pick(best) : null;
      return b == null || v > b ? r : best;
    }, null);

  const pickSort = (key: SortKey) => {
    if (key === sortKey) {
      setDesc((d) => !d);
    } else {
      setSortKey(key);
      setDesc(true);
    }
    // 切排序时收起展开卡：它可能掉出前 12 名，展开态凭空消失更莫名其妙
    setExpanded(null);
  };

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="shrink-0 border-b border-hairline-soft px-5 py-2.5">
        <div className="flex flex-wrap items-center gap-2">
          <input
            ref={searchRef}
            value={query}
            onChange={(e) => {
              setQuery(e.target.value);
              setExpanded(null);
            }}
            placeholder="搜索应用…"
            className="w-[168px] rounded-md border border-hairline bg-white/[0.03] px-2 py-1 text-[11px] text-ink placeholder:text-ink-faint focus:border-accent/60 focus:outline-none"
          />
          <span className="ml-1 text-[10px] text-ink-faint">排序</span>
          {SORTS.map((s) => {
            const active = s.key === sortKey;
            return (
              <button
                key={s.key}
                type="button"
                onClick={() => pickSort(s.key)}
                className={`rounded-md px-2 py-1 text-[10.5px] transition-colors ${
                  active
                    ? "bg-white/[0.09] text-ink"
                    : "text-ink-dim hover:bg-white/[0.05] hover:text-ink"
                }`}
                title={`按${s.label}排序${active ? "（再点切换升降序）" : ""}`}
              >
                {s.dot ? (
                  <span
                    aria-hidden
                    className="mr-1 inline-block size-[6px] rounded-full align-middle"
                    style={{
                      background: s.dot,
                      opacity: active ? 1 : 0.35,
                      boxShadow: active ? `0 0 6px color-mix(in srgb, ${s.dot} 60%, transparent)` : undefined,
                    }}
                  />
                ) : null}
                {s.label}
                {active ? <span className="ml-0.5 text-[9px]">{desc ? "▾" : "▴"}</span> : null}
              </button>
            );
          })}
          <div className="flex-1" />
          <span className="font-mono text-[10px] tabular-nums text-ink-faint">
            显示 {visible.length} / {filtered.length}
          </span>
        </div>

        <div className="mt-1.5 flex flex-wrap items-center gap-x-3 gap-y-1 text-[10px] text-ink-faint">
          <span>
            {filtered.length} 个应用 ·{" "}
            <span className="font-mono tabular-nums text-ink-dim">{totalProcs}</span> 进程
          </span>
          <span className="font-mono tabular-nums">
            ↓ {totalDown == null ? "—" : formatSpeed(totalDown)} · ↑{" "}
            {totalUp == null ? "—" : formatSpeed(totalUp)}
          </span>
          <span>
            {net == null
              ? "网速尚未采样"
              : net.status === "warmup"
                ? "网速采样中…"
                : net.status === "unavailable"
                  ? "网速不可读"
                  : `网速 ${Math.round(net.intervalMs)} ms 差分 · 仅非回环`}
          </span>
          {benchText ? (
            <span
              title="卡内比例条的分母 = 该指标全体应用的最大值；Top1 满条，其余按差距缩短。切换排序后基准跟着换，跨排序的条长不可直接比较"
            >
              {SORTS.find((s) => s.key === sortKey)?.label}条形基准{" "}
              <span className="font-mono tabular-nums text-ink-dim">{benchText}</span>
            </span>
          ) : null}
          {spotlight ? (
            <>
              <span className="text-ink-faint/70">大户</span>
              {[
                { label: "内存", r: top((x) => x.mem) },
                { label: "CPU", r: top((x) => x.cpu) },
                { label: "进程", r: top((x) => x.procs) },
                { label: "下载", r: top((x) => x.net?.downBps ?? null) },
              ].map(({ label, r }) =>
                r ? (
                  <button
                    key={label}
                    type="button"
                    onClick={() => pickSort(label === "内存" ? "mem" : label === "CPU" ? "cpu" : label === "进程" ? "procs" : "down")}
                    className="rounded bg-white/[0.05] px-1.5 py-0.5 text-ink-dim transition-colors hover:bg-white/[0.09] hover:text-ink"
                    title={`切到${label}排序`}
                  >
                    {label} <span className="text-ink">{r.family.name}</span>
                  </button>
                ) : null,
              )}
            </>
          ) : null}
        </div>
      </div>

      {net?.status === "unavailable" ? (
        <div className="mx-5 mt-2 rounded-lg bg-warn/[0.08] px-3 py-1.5 text-[10.5px] leading-relaxed text-warn">
          ⚠ 网速不可读：{net.note ?? "nettop 不可用"}。卡片里的网速行保持空白，不估算、不显示 0。
        </div>
      ) : null}

      <div className="min-h-0 flex-1 overflow-y-auto px-5 py-3">
        <div className="grid grid-cols-2 gap-3">
          {visible.map((r) => (
            <ResourceCard
              key={r.family.name}
              row={r}
              expanded={expanded === r.family.name}
              onToggle={() =>
                setExpanded((cur) => (cur === r.family.name ? null : r.family.name))
              }
              denom={denom}
              netStatus={net?.status ?? null}
              bundleReady={bundles != null}
              portsReady={ports != null}
              history={historyRef.current.get(r.family.name) ?? []}
              onOpenPorts={onOpenPorts}
              onOpenProcess={onOpenProcess}
            />
          ))}
        </div>

        {gated ? (
          <button
            type="button"
            onClick={() => setShowAll((v) => !v)}
            className="mt-3 w-full rounded-lg bg-white/[0.03] py-2 text-[11px] text-ink-dim transition-colors hover:bg-white/[0.06] hover:text-ink"
          >
            {showAll
              ? "⌃ 收起，只看 Top 12"
              : `⌄ 展开剩余 ${remaining} 个（第 13–${filtered.length} 名，仍按${SORTS.find((s) => s.key === sortKey)?.label}${desc ? "降序" : "升序"}）`}
          </button>
        ) : null}

        {filtered.length === 0 ? (
          <p className="px-1 py-6 text-[11px] text-ink-faint">
            没有匹配「{query}」的应用。
          </p>
        ) : null}
      </div>
    </div>
  );
}

/** 一张卡：摘要（头 + 概览 + 三行指标）→ 点开跨两列长大，下半区放表格放不下的细节 */
function ResourceCard({
  row,
  expanded,
  onToggle,
  denom,
  netStatus,
  bundleReady,
  portsReady,
  history,
  showNet = true,
  dense = false,
  onOpenPorts,
  onOpenProcess,
}: {
  row: Row;
  expanded: boolean;
  onToggle: () => void;
  denom: { mem: number; cpu: number; disk: number; down: number; procs: number };
  netStatus: NetSpeedSample["status"] | null;
  bundleReady: boolean;
  portsReady: boolean;
  history: number[];
  /** 面板不采样网速：隐藏整行比显示「尚未采样」诚实（后者像坏了） */
  showNet?: boolean;
  /** 面板紧凑规格：收紧内边距与行距，3 张卡 + 入口必须在一屏内装下、不滚动 */
  dense?: boolean;
  onOpenPorts?: (port: number) => void;
  onOpenProcess?: (pid: number) => void;
}) {
  const { family, members, mem, cpu, procs, disk, ports, exposed, conns, net } = row;
  const exact = isExact(family);

  const netText =
    netStatus == null
      ? "尚未采样"
      : netStatus === "warmup"
        ? "采样中…"
        : netStatus === "unavailable"
          ? "不可读"
          : net == null
            ? "—"
            : // 双向合并为一个短串：/s 只留一个尾缀（`↓3.8k ↑120 B/s`），完整读数在 hover 里
              `↓${compactSpeed(net.downBps)} ↑${compactSpeed(net.upBps)}/s`;

  const diskText = disk
    ? `${disk.partial ? "≥ " : ""}${formatBytes(disk.bytes)}`
    : // 首次进入还在批量测量：说「测量中…」而不是「—」（— 的含义是「没有 .app」）
      !bundleReady
      ? "测量中…"
      : "—";

  return (
    <div
      className={`col-span-1 flex flex-col rounded-xl border transition-colors ${
        expanded
          ? "col-span-2 border-hairline bg-white/[0.045]"
          : "border-hairline-soft bg-white/[0.02] hover:border-hairline hover:bg-white/[0.035]"
      }`}
    >
      <div
        role="button"
        tabIndex={0}
        onClick={onToggle}
        onKeyDown={(e) => {
          if (e.key === "Enter" || e.key === " ") {
            e.preventDefault();
            onToggle();
          }
        }}
        aria-expanded={expanded}
        className={`cursor-pointer text-left ${dense ? "px-2.5 pt-1.5 pb-1.5" : "px-3 pt-2 pb-2"}`}
      >
        <div className="flex items-center gap-1.5">
          <ChevronRightIcon
            size={11}
            className={`shrink-0 text-ink-faint transition-transform ${expanded ? "rotate-90" : ""}`}
          />
          <span
            className={`min-w-0 flex-1 truncate leading-tight text-ink ${
              dense ? "text-[12px]" : "text-[12.5px]"
            }`}
          >
            {family.name}
          </span>
          <span
            className={`shrink-0 font-mono text-[10.5px] tabular-nums ${
              procs >= PROC_WARN ? "text-warn" : "text-ink-dim"
            }`}
            title={
              procs >= PROC_WARN
                ? `${procs} 个进程：Electron 家族常见，进程数多不等于占资源多`
                : `${procs} 个进程`
            }
          >
            {procs} 进程
          </span>
        </div>

        <div
          className={`flex items-center gap-x-2 text-ink-faint ${
            dense ? "mt-0.5 pl-[16px] text-[9.5px]" : "mt-0.5 flex-wrap pl-[18px] text-[10px]"
          }`}
        >
          {/* CPU% 已由下方紫色 mini-bar 行承担，副标题不再重复（去冗余） */}
          <span
            title={
              portsReady
                ? "只统计关注层端口；Electron 的内部 IPC 随机端口不计入"
                : "端口尚未扫描（「—」不代表没有占用）"
            }
          >
            {!portsReady ? "端口未扫" : ports.length === 0 ? "— 端口" : `${ports.length} 端口`}
            {exposed > 0 ? <span className="ml-0.5 text-warn">●</span> : null}
          </span>
          <span>·</span>
          <span title={portsReady ? "ESTABLISHED 连接数" : "端口尚未扫描，连接数未知"}>
            {conns == null ? "— 连接" : `${conns.established} 连接`}
          </span>
        </div>

        <div className={dense ? "mt-1 space-y-[3px]" : "mt-1.5 space-y-[5px]"}>
          {/* UI v4 排行榜条：CPU 紫 + 内存蓝双 mini-bar，域色即语义（§3.2） */}
          <MetricRow
            dense={dense}
            label="CPU"
            text={cpuShort(cpu)}
            hint="单核口径，可超过 100%"
            textClass={cpu != null && cpu >= 100 ? "text-danger" : ""}
            bar={
              <CellBar
                ratio={denom.cpu > 0 && cpu != null ? cpu / denom.cpu : 0}
                tone="cpu"
                show={denom.cpu > 0 && cpu != null}
              />
            }
          />
          <MetricRow
            dense={dense}
            label="内存"
            text={`${exact ? "" : "≈"}${formatBytes(mem)}`}
            hint="footprint 口径，含被换出到磁盘的部分；未深探的子进程按 RSS 计入（下界）"
            bar={<CellBar ratio={denom.mem > 0 ? mem / denom.mem : 0} tone="mem" show={denom.mem > 0} />}
          />
          <MetricRow
            dense={dense}
            label="磁盘"
            text={diskText}
            hint={`仅 .app 本体；${BUNDLE_READONLY_HINT}。缓存/数据在展开后按需测量`}
            bar={
              <CellBar
                ratio={denom.disk > 0 && disk ? disk.bytes / denom.disk : 0}
                tone={disk?.partial ? "warn" : "neutral"}
                show={denom.disk > 0 && !!disk}
              />
            }
          />
          {showNet ? (
            <MetricRow
              label="网速"
              text={netText ?? "—"}
              hint={
                net
                  ? `下载 ${formatSpeed(net.downBps)} · 上传 ${formatSpeed(net.upBps)} · 1 s 差分的瞬时速率，仅非回环接口（localhost 转发不计入）；连接多不等于流量大`
                  : "1 s 差分的瞬时速率，仅非回环接口（localhost 转发不计入）；连接多不等于流量大"
              }
              textClass="text-[10px]"
              bar={
                <CellBar
                  ratio={denom.down > 0 && net ? net.downBps / denom.down : 0}
                  tone="net"
                  show={denom.down > 0 && !!net && net.downBps > 0}
                />
              }
            />
          ) : null}
        </div>
      </div>

      {expanded ? (
        <ResourceDetail
          family={family}
          members={members}
          ports={ports}
          history={history}
          onOpenPorts={onOpenPorts}
          onOpenProcess={onOpenProcess}
        />
      ) : null}
    </div>
  );
}

function MetricRow({
  label,
  text,
  hint,
  bar,
  textClass = "",
  dense = false,
}: {
  label: string;
  text: string;
  hint: string;
  bar: ReactNode;
  textClass?: string;
  dense?: boolean;
}) {
  return (
    <div
      className={
        dense
          ? "grid grid-cols-[24px_84px_minmax(0,1fr)] items-center gap-1.5"
          : "grid grid-cols-[30px_92px_minmax(0,1fr)] items-center gap-2"
      }
      title={hint}
    >
      <span className={dense ? "text-[9.5px] text-ink-faint" : "text-[10px] text-ink-faint"}>
        {label}
      </span>
      <span
        className={`truncate text-right font-mono tabular-nums text-ink-dim ${textClass} ${
          dense ? "text-[11px]" : ""
        }`}
      >
        {text}
      </span>
      <span className="min-w-0">{bar}</span>
    </div>
  );
}

/** 展开区：只放卡片里没有的——磁盘三层按钮、端口清单、进程 Top 3、60 s 网速走势 */
function ResourceDetail({
  family,
  members,
  ports,
  history,
  onOpenPorts,
  onOpenProcess,
}: {
  family: FamilyMem;
  members: ProcMem[];
  ports: PortEntry[];
  history: number[];
  onOpenPorts?: (port: number) => void;
  onOpenProcess?: (pid: number) => void;
}) {
  const [disk, setDisk] = useState<AppDisk | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const exe = members.find((m) => m.exe)?.exe ?? null;
  const top3 = useMemo(
    () => [...members].sort((a, b) => totalOf(b) - totalOf(a)).slice(0, 3),
    [members],
  );

  async function measure(force: boolean) {
    setBusy(true);
    setError(null);
    try {
      if (!isTauri()) {
        setDisk(devAppDisk(family.name));
        return;
      }
      setDisk(await appDisk(family.name, exe, force));
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="border-t border-hairline-soft px-3 py-2.5">
      <div className="flex flex-wrap items-center gap-2">
        <button
          type="button"
          onClick={() => void measure(!!disk)}
          disabled={busy}
          className="rounded-md bg-white/[0.05] px-2 py-1 text-[10.5px] text-ink-dim transition-colors hover:bg-white/[0.09] hover:text-ink disabled:opacity-50"
          title="只读测量；应用越大越久，最多 4 秒/层"
        >
          {busy ? "测量中…" : disk ? "重新测量磁盘三层" : "测量磁盘三层"}
        </button>
        <span className="text-[10px] text-ink-faint">
          本体 / 缓存 / 数据 · 不产生删除动作
        </span>
      </div>

      {disk ? (
        <div className="mt-2 space-y-1">
          {/* 三层构成条（§7.6）：缓存占多大比重一眼看出；数字文本仍逐行保留（V-01） */}
          {!diskEmpty(disk) ? (
            <StackBar
              height={5}
              ariaLabel={`磁盘三层构成：本体 ${formatBytes(disk.bundle?.bytes ?? 0)}，缓存 ${formatBytes(disk.cache?.bytes ?? 0)}，数据 ${formatBytes(disk.data?.bytes ?? 0)}`}
              segments={[
                { key: "bundle", label: "本体", value: disk.bundle?.bytes ?? 0, tone: "accent" },
                { key: "cache", label: "缓存", value: disk.cache?.bytes ?? 0, tone: "info" },
                { key: "data", label: "数据", value: disk.data?.bytes ?? 0, tone: "neutral" },
              ]}
            />
          ) : null}
          {[
            { label: "本体", leaf: disk.bundle, hint: BUNDLE_READONLY_HINT },
            { label: "缓存", leaf: disk.cache, hint: CACHE_CLEAN_HINT },
            { label: "数据", leaf: disk.data, hint: DATA_READONLY_HINT },
          ].map(({ label, leaf, hint }) => (
            <div key={label} className="flex items-baseline gap-2" title={hint}>
              <span className="w-7 shrink-0 text-[10px] text-ink-faint">{label}</span>
              <span className="min-w-0 flex-1 truncate font-mono text-[9.5px] text-ink-faint/70">
                {leaf ? leaf.path : "—"}
              </span>
              <span className="shrink-0 font-mono text-[10.5px] tabular-nums text-ink-dim">
                {leaf ? `${leaf.partial ? "≥ " : ""}${formatBytes(leaf.bytes)}` : "—"}
              </span>
            </div>
          ))}
          {diskEmpty(disk) ? (
            <p className="text-[10px] text-ink-faint">{disk.note ?? "没有可归属的磁盘目录"}</p>
          ) : (
            <p className="text-[9.5px] leading-snug text-ink-faint">
              合计 {formatBytes(diskTotal(disk))} · 按文件实占块统计，含 APFS 克隆共享部分，
              不等于可释放空间
            </p>
          )}
        </div>
      ) : null}

      {error ? <p className="mt-1.5 text-[10px] text-warn">{error}</p> : null}

      <div className="mt-2.5 flex flex-wrap items-center gap-1.5">
        <span className="text-[10px] text-ink-faint">监听端口</span>
        {ports.length === 0 ? (
          <span className="text-[10px] text-ink-faint">—</span>
        ) : (
          ports.map((e) => {
            const meta = exposureMeta(e.exposure);
            return (
              <button
                key={`${e.port}-${e.proto}`}
                type="button"
                onClick={() => onOpenPorts?.(e.port)}
                title={`${meta.label} · ${meta.hint}；点击去「端口」tab 看持有进程`}
                className="rounded bg-white/[0.05] px-1.5 py-0.5 font-mono text-[10px] tabular-nums text-ink-dim transition-colors hover:bg-white/[0.09] hover:text-ink"
              >
                {e.port}
                {e.exposure === "any" ? <span className="ml-0.5 text-warn">●</span> : null}
              </button>
            );
          })
        )}
      </div>

      <div className="mt-1.5 flex items-baseline gap-2">
        <span className="shrink-0 text-[10px] text-ink-faint">进程 Top</span>
        <span className="min-w-0 flex-1 truncate text-[10px] text-ink-dim">
          {top3.length === 0
            ? "—"
            : top3.map((p) => `${p.name} ${formatBytes(totalOf(p))}`).join(" · ")}
          {members.length > 3 ? ` · 共 ${members.length} 个` : ""}
        </span>
        {top3[0] ? (
          <button
            type="button"
            onClick={() => onOpenProcess?.(top3[0].pid)}
            className="shrink-0 text-[9.5px] text-accent hover:underline"
          >
            看明细
          </button>
        ) : null}
      </div>

      <div className="mt-1.5 flex items-center gap-2">
        <span className="shrink-0 text-[10px] text-ink-faint">网速 60s</span>
        <span className="min-w-0 flex-1">
          {history.length >= 2 ? (
            <Sparkline values={history} height={26} tone="accent" />
          ) : (
            <span className="text-[10px] text-ink-faint">采样积累中…</span>
          )}
        </span>
      </div>

      <p className="mt-1 text-[9.5px] leading-snug text-ink-faint">
        单核 CPU 口径可超过 100%；连接多不等于流量大（本机没有 per-process 字节 API，
        网速来自 nettop 差分）。
      </p>
    </div>
  );
}

/**
 * 面板用的轻量版已随 M16 撤除：菜单栏面板不再展示应用卡，改为统计行 +
 * 一行「查看全部 N 个应用」入口。`ResourceCard` 的 `dense` 档位仍保留，
 * 供将来需要单列紧凑卡时复用。
 */

