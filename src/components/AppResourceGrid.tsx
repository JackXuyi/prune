import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";

import { CellBar } from "./CellBar";
import { EmptyState } from "./EmptyArt";
import { Btn } from "./primitives";
import { AppIcon } from "./AppIcon";
import {
  ChipIcon,
  ChevronRightIcon,
  DatabaseIcon,
  DownloadIcon,
  LinkIcon,
  MemoryCloudIcon,
  PortJackIcon,
  SearchIcon,
  StackIcon,
} from "./icons";
import { AppSummaryRibbon } from "./resources/AppSummaryRibbon";
import { useI18n } from "../i18n/store";
import type { NoParamKey } from "../i18n/types";
import { devBundles } from "../lib/devFixture";
import { formatBytes } from "../lib/format";
import { appBundles, isTauri } from "../lib/ipc";
import { useAppIcons } from "../lib/useAppIcons";
import { type BundleMap, type BundleSize } from "../types/appdisk";
import type { SortKey } from "../types/appsort";
import { isExact, totalOf, type FamilyMem, type ProcMem } from "../types/mem";
import {
  connsOfPids,
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

/**
 * 排序维度（eul 式图标化）：每个 chip = 域色图标 + 短标签，
 * 颜色即语义（mem 蓝 / cpu 紫 / 端口琥珀 / 网速青），与卡内指标同一套配色。
 * 同时作为页面「透镜」：卡片顶缘洗光跟随当前排序的域色。
 */
const SORTS: { key: SortKey; labelKey: NoParamKey; icon: typeof MemoryCloudIcon; color: string }[] = [
  { key: "mem", labelKey: "resources.sort.mem", icon: MemoryCloudIcon, color: "var(--color-info)" },
  { key: "cpu", labelKey: "resources.sort.cpu", icon: ChipIcon, color: "var(--color-domain-cpu)" },
  { key: "procs", labelKey: "resources.sort.procs", icon: StackIcon, color: "var(--color-ink-dim)" },
  { key: "disk", labelKey: "resources.sort.disk", icon: DatabaseIcon, color: "var(--color-accent)" },
  { key: "ports", labelKey: "resources.sort.ports", icon: PortJackIcon, color: "var(--color-domain-port)" },
  { key: "down", labelKey: "resources.sort.down", icon: DownloadIcon, color: "var(--color-domain-net)" },
];

/** 大户 spotlight 的四个维度：顺序固定，点一下就切到对应排序 */
const SPOTS: { key: SortKey; labelKey: NoParamKey }[] = [
  { key: "mem", labelKey: "resources.sort.mem" },
  { key: "cpu", labelKey: "resources.sort.cpu" },
  { key: "procs", labelKey: "resources.sort.procs" },
  { key: "down", labelKey: "resources.sort.down" },
];

interface Row {
  family: FamilyMem;
  members: ProcMem[];
  pids: number[];
  /** 主进程 exe：图标提取用（拿不到就 null → 字母徽标） */
  exe: string | null;
  mem: number;
  cpu: number | null;
  procs: number;
  disk: BundleSize | null;
  ports: PortEntry[];
  exposed: number;
  conns: ConnStats | null;
  net: Speed | null;
  /**
   * 搜索用文本：应用名 + 成员进程名 + 可执行路径 + 端口进程名。
   * v5 纠偏：v4 只匹配家族名，搜 `chrome` 在中文界面里永远没结果 ——
   * 用户记得住进程名，记不住我们给的分类名。
   */
  hay: string;
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
    const hay = [
      f.name,
      ...members.map((m) => m.name),
      ...members.map((m) => m.exe ?? ""),
      ...watched.flatMap((e) => e.listeners.map((b) => b.comm ?? "")),
    ]
      .join(" ")
      .toLowerCase();
    return {
      family: f,
      members,
      pids,
      exe: members.find((m) => m.exe)?.exe ?? null,
      mem: totalOf(f),
      cpu: f.cpu ?? null,
      procs: f.processCount,
      disk: bundles ? (bundles[f.name] ?? null) : null,
      ports: watched,
      exposed: watched.filter((e) => e.exposure === "any").length,
      conns: ports ? connsOfPids(ports.connsByPid, pids) : null,
      net: speed,
      hay,
    };
  });
}

/** 排序 / spotlight 取值一律走这里：同一个 key 在「排序」和「大户」里必须取同一个数 */
function sortValue(key: SortKey, r: Row): number | null {
  switch (key) {
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
  onOpenApp,
}: {
  families: FamilyMem[];
  processes: ProcMem[];
  ports: PortSnapshot | null;
  net: NetSpeedSample | null;
  /** 父级刷新按钮的计数：变化时强制重测磁盘本体 */
  refreshNonce?: number;
  /** D2：整卡点击进入应用详情（取消卡内展开态） */
  onOpenApp?: (family: string) => void;
}) {
  const [sortKey, setSortKey] = useState<SortKey>("mem");
  const { t } = useI18n();
  const sortDef = SORTS.find((s) => s.key === sortKey) ?? SORTS[0];
  /** 透镜色：卡片洗光/底缘与激活指标跟随当前排序——一眼看出「现在按什么看」 */
  const lensColor = sortDef.color;
  const sortLabel = t(sortDef.labelKey);
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
  const [bundles, setBundles] = useState<BundleMap | null>(null);
  const [bundleBusy, setBundleBusy] = useState(false);

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

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    const base = q ? rows.filter((r) => r.hay.includes(q)) : rows;
    return [...base].sort((a, b) => {
      const va = sortValue(sortKey, a);
      const vb = sortValue(sortKey, b);
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

  /**
   * 摘要带口径：跟随当前筛选（搜索时只统计筛出来的），与列表计数一致。
   * 内存是各家族 footprint 求和——共享页会重复计入，tooltip 里已注明。
   */
  const agg = useMemo(() => {
    const sum = (pick: (r: Row) => number | null) => filtered.reduce((n, r) => n + (pick(r) ?? 0), 0);
    return {
      mem: sum((r) => r.mem),
      procs: sum((r) => r.procs),
      exposed: sum((r) => r.exposed),
      ports: ports ? sum((r) => r.ports.length) : null,
      conns: ports ? sum((r) => r.conns?.established ?? null) : null,
    };
  }, [filtered, ports]);

  const searching = query.trim().length > 0;
  const visible = searching || showAll ? filtered : filtered.slice(0, DEFAULT_VISIBLE);
  const remaining = filtered.length - DEFAULT_VISIBLE;
  const gated = !searching && filtered.length > DEFAULT_VISIBLE;

  // D1：首屏 + 一屏滚动余量的图标（60 个上限在 Rust 侧钳制，这里只挑要渲染的）
  const iconItems = useMemo(
    () => visible.slice(0, 40).map((r) => ({ family: r.family.name, exe: r.exe })),
    [visible],
  );
  const icons = useAppIcons(iconItems);

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
        return t("resources.procsTitle", { n: v });
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
  };

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      {/* ① 摘要带：与首页状态带同形——图标 + mono 读数，口径全进 tooltip */}
      <div className="shrink-0 px-5 pt-2">
        <AppSummaryRibbon
          apps={filtered.length}
          procs={agg.procs}
          memBytes={agg.mem}
          net={net}
          ports={agg.ports}
          exposed={agg.exposed}
          conns={agg.conns}
          searching={searching}
          onSort={pickSort}
        />

        {/* ② 工具条：搜索 + 排序（域色图标 + 短标签）+ 显示计数 */}
        <div className="mt-2 flex flex-wrap items-center gap-1.5">
          <div className="relative">
            <SearchIcon
              size={11}
              className="pointer-events-none absolute top-1/2 left-2 -translate-y-1/2 text-ink-faint"
            />
            <input
              ref={searchRef}
              value={query}
              onChange={(e) => {
                setQuery(e.target.value);
              }}
              placeholder={t("resources.searchPlaceholder")}
              className="w-[156px] rounded-md border border-hairline bg-chip-soft py-1 pr-2 pl-[26px] text-[11px] text-ink placeholder:text-ink-faint focus:border-accent/60 focus:outline-none"
            />
          </div>
          <div className="ml-1 flex items-center gap-0.5 rounded-lg border border-hairline-soft bg-surface-raised p-0.5">
            {SORTS.map((s) => {
              const active = s.key === sortKey;
              const label = t(s.labelKey);
              const Icon = s.icon;
              return (
                <button
                  key={s.key}
                  type="button"
                  onClick={() => pickSort(s.key)}
                  className={`flex items-center gap-1 rounded-[7px] px-1.5 py-1 text-[10.5px] transition-colors ${
                    active ? "bg-chip-hover text-ink" : "text-ink-dim hover:bg-chip hover:text-ink"
                  }`}
                  title={active ? t("resources.sortTipToggle", { label }) : t("resources.sortTip", { label })}
                >
                  <span
                    aria-hidden
                    className="inline-flex"
                    style={{ color: s.color, opacity: active ? 1 : 0.5 }}
                  >
                    <Icon size={11} />
                  </span>
                  {label}
                  {active ? <span className="ml-0.5 text-[9px]">{desc ? "▾" : "▴"}</span> : null}
                </button>
              );
            })}
          </div>
          <div className="flex-1" />
          <span className="font-mono text-[10px] tabular-nums text-ink-faint">
            {t("resources.showing", { shown: visible.length, total: filtered.length })}
          </span>
        </div>

        {/* ③ 口径行：条形基准用小图形代替文字；大户 = 域色图标 + 应用名 */}
        {benchText || spotlight ? (
          <div className="mt-1.5 flex flex-wrap items-center gap-x-2 gap-y-1 text-[10px] text-ink-faint">
            {benchText ? (
              <span
                className="flex items-center gap-1"
                title={t("resources.benchTip")}
                aria-label={t("resources.bench", { label: sortLabel })}
              >
                <BenchGlyph />
                <span className="font-mono tabular-nums text-ink-dim">{benchText}</span>
              </span>
            ) : null}
            {spotlight
              ? SPOTS.map(({ key, labelKey }) => {
                  const r = top((x) => sortValue(key, x));
                  const label = t(labelKey);
                  const def = SORTS.find((s) => s.key === key);
                  const Icon = def?.icon ?? MemoryCloudIcon;
                  return r ? (
                    <button
                      key={key}
                      type="button"
                      onClick={() => pickSort(key)}
                      className="flex items-center gap-1 rounded bg-chip px-1.5 py-0.5 text-ink-dim transition-colors hover:bg-chip-hover hover:text-ink"
                      title={t("resources.spotlightTip", { label })}
                    >
                      <span aria-hidden className="inline-flex" style={{ color: def?.color }}>
                        <Icon size={9} />
                      </span>
                      <span className="text-ink">{r.family.name}</span>
                    </button>
                  ) : null;
                })
              : null}
          </div>
        ) : null}
      </div>

      {net?.status === "unavailable" ? (
        <div className="mx-5 mt-2 rounded-lg bg-warn/[0.08] px-3 py-1.5 text-[10.5px] leading-relaxed text-warn">
          {t("resources.netWarn", { why: net.note ?? t("resources.netFallback") })}
        </div>
      ) : null}

      <div className="min-h-0 flex-1 overflow-y-auto px-5 pt-2.5 pb-3">
        {/* 自适应列数：卡片更窄、一屏更多；240px 是「图标 + 名称 + 三枚小簇」的下限 */}
        <div className="grid grid-cols-[repeat(auto-fill,minmax(240px,1fr))] gap-2.5">
          {visible.map((r) => (
            <ResourceCard
              key={r.family.name}
              row={r}
              icon={icons[r.family.name]}
              onOpen={onOpenApp ? () => onOpenApp(r.family.name) : undefined}
              denom={denom}
              netStatus={net?.status ?? null}
              bundleReady={bundles != null}
              portsReady={ports != null}
              lensKey={sortKey}
              lensColor={lensColor}
            />
          ))}
        </div>

        {gated ? (
          <button
            type="button"
            onClick={() => setShowAll((v) => !v)}
            className="mt-3 w-full rounded-lg bg-chip-soft py-2 text-[11px] text-ink-dim transition-colors hover:bg-chip-hover hover:text-ink"
          >
            {showAll
              ? t("resources.collapse")
              : t("resources.expand", {
                  n: remaining,
                  last: filtered.length,
                  label: sortLabel,
                  order: desc ? t("resources.sortDesc") : t("resources.sortAsc"),
                })}
          </button>
        ) : null}

        {/* 空态必须能分辨「没搜到」和「没数据」：前者给清除搜索，后者说明权限 */}
        {filtered.length === 0 ? (
          <EmptyState
            art="search"
            title={t("resources.noMatchTitle", { query })}
            description={t("resources.noMatchNote")}
            action={
              <Btn
                variant="ghost"
                onClick={() => {
                  setQuery("");
                  searchRef.current?.focus();
                }}
              >
                {t("resources.clearSearch")}
              </Btn>
            }
          />
        ) : null}
        {rows.length === 0 ? (
          <EmptyState
            art="no-record"
            title={t("resources.noProcTitle")}
            description={t("resources.noProcNote")}
          />
        ) : null}
      </div>
    </div>
  );
}

/**
 * 一张卡（eul 式）：应用图标 + 名称 + 三枚「图标 + 数字」小簇（进程 / 端口 / 连接）
 * + 2×2 指标网格（域色图标 + 比例条 + mono 读数）。
 *
 * 纪律：卡内零文字标签（口径全进 tooltip）；顶缘洗光与底缘发丝线跟随当前排序
 * 的域色（透镜），激活指标数值同色加粗——「现在按什么看」不用读字。
 */
function ResourceCard({
  row,
  icon,
  onOpen,
  denom,
  netStatus,
  bundleReady,
  portsReady,
  lensKey,
  lensColor,
}: {
  row: Row;
  /** 真实图标（data URL）；缺省落字母徽标 */
  icon?: string;
  /** 缺省时卡片不可点（面板等只读场景） */
  onOpen?: () => void;
  denom: { mem: number; cpu: number; disk: number; down: number; procs: number };
  netStatus: NetSpeedSample["status"] | null;
  bundleReady: boolean;
  portsReady: boolean;
  /** 当前排序维度：对应指标高亮 */
  lensKey: SortKey;
  /** 当前排序的域色：洗光 / 底缘 / 激活读数 */
  lensColor: string;
}) {
  const { family, mem, cpu, procs, disk, ports, exposed, conns, net } = row;
  const exact = isExact(family);
  const { t } = useI18n();

  // 读数只放数字与短单位；「为什么是 —」进 tooltip（零文字纪律 + 假事实红线）
  const netText =
    netStatus == null || netStatus === "warmup" || netStatus === "unavailable"
      ? "—"
      : net == null
        ? "—"
        : // 双向合并为一个短串：/s 只留一个尾缀（`↓3.8k ↑120/s`），完整读数在 hover 里
          `↓${compactSpeed(net.downBps)} ↑${compactSpeed(net.upBps)}/s`;

  const netTitle =
    netStatus == null
      ? t("resources.netShort.notSampled")
      : netStatus === "warmup"
        ? t("resources.netShort.sampling")
        : netStatus === "unavailable"
          ? t("resources.netShort.unreadable")
          : net
            ? t("resources.mesh.netHintWith", {
                down: formatSpeed(net.downBps),
                up: formatSpeed(net.upBps),
              })
            : t("resources.mesh.netHint");

  const diskText = disk
    ? `${disk.partial ? "≥ " : ""}${formatBytes(disk.bytes)}`
    : // 首次进入还在批量测量：说「测量中…」而不是「—」（— 的含义是「没有 .app」）
      !bundleReady
      ? t("common.progress.measuring")
      : "—";

  return (
    <button
      type="button"
      onClick={onOpen}
      disabled={!onOpen}
      aria-label={onOpen ? t("resources.ariaOpen", { name: family.name }) : undefined}
      className="group relative col-span-1 flex w-full flex-col overflow-hidden rounded-card border border-hairline-soft bg-surface-raised px-3 pt-2 pb-2.5 text-left transition-[border-color,transform,box-shadow] duration-150 hover:-translate-y-px hover:border-hairline hover:shadow-lg hover:shadow-black/10 disabled:cursor-default disabled:hover:translate-y-0 disabled:hover:border-hairline-soft disabled:hover:shadow-none"
    >
      {/* 透镜：顶缘域色洗光 + 底缘发丝线（跟当前排序），与首页磁贴同一套皮 */}
      <span
        aria-hidden
        className="pointer-events-none absolute inset-0"
        style={{
          backgroundImage: `linear-gradient(180deg, color-mix(in srgb, ${lensColor} 6%, transparent), transparent 60%)`,
        }}
      />
      <span
        aria-hidden
        className="pointer-events-none absolute inset-x-0 bottom-0 h-[2px]"
        style={{
          background: `linear-gradient(90deg, transparent, color-mix(in srgb, ${lensColor} 40%, transparent) 30%, color-mix(in srgb, ${lensColor} 40%, transparent) 70%, transparent)`,
        }}
      />

      {/* w-full：WKWebView 里 button 作 flex 容器时交叉轴 stretch 不生效（同 DomainCard） */}
      <div className="relative flex w-full items-center gap-2">
        <AppIcon
          family={family.name}
          icon={icon}
          size={28}
          className="shrink-0 transition-transform group-hover:scale-[1.03]"
        />
        <span className="min-w-0 flex-1 truncate text-[12.5px] leading-tight text-ink">
          {family.name}
        </span>
        {/* 三枚小簇：图标 + 数字，零文字（进程标警色只是提示，tooltip 里有明文） */}
        <Cluster
          icon={<StackIcon size={10} />}
          value={String(procs)}
          tone={procs >= PROC_WARN ? "warn" : undefined}
          title={
            procs >= PROC_WARN
              ? t("resources.procsTitleWarn", { n: procs })
              : t("resources.procsTitle", { n: procs })
          }
        />
        <Cluster
          icon={<PortJackIcon size={10} />}
          value={portsReady ? String(ports.length) : "—"}
          dot={exposed > 0}
          title={portsReady ? t("resources.portsTip") : t("resources.portsUnscannedTip")}
        />
        <Cluster
          icon={<LinkIcon size={10} />}
          value={conns == null ? "—" : String(conns.established)}
          title={portsReady ? t("resources.connsTip") : t("resources.connsUnscannedTip")}
        />
        <ChevronRightIcon
          size={11}
          className="shrink-0 text-ink-faint opacity-0 transition-[opacity,transform] group-hover:translate-x-0.5 group-hover:opacity-100"
        />
      </div>

      {/* 指标区：窄卡单列四行（图标 + 条 + 读数）——2×2 在 240px 卡里会把条压成 20px，
          量感全失；单列让条始终有足够长度，卡片宽度自适应时也不用改布局 */}
      <div className="relative mt-2 grid w-full grid-cols-1 gap-y-[5px]">
        <MetricCell
          icon={<ChipIcon size={11} />}
          color="var(--color-domain-cpu)"
          label={t("resources.metric.cpu")}
          hint={t("resources.mesh.cpuHint")}
          value={cpuShort(cpu)}
          danger={cpu != null && cpu >= 100}
          active={lensKey === "cpu"}
          lensColor={lensColor}
          bar={
            <CellBar
              ratio={denom.cpu > 0 && cpu != null ? cpu / denom.cpu : 0}
              tone="cpu"
              show={denom.cpu > 0 && cpu != null}
            />
          }
        />
        <MetricCell
          icon={<MemoryCloudIcon size={11} />}
          color="var(--color-info)"
          label={t("resources.metric.mem")}
          hint={t("resources.mesh.memHint")}
          value={`${exact ? "" : "≈"}${formatBytes(mem)}`}
          active={lensKey === "mem"}
          lensColor={lensColor}
          bar={<CellBar ratio={denom.mem > 0 ? mem / denom.mem : 0} tone="mem" show={denom.mem > 0} />}
        />
        <MetricCell
          icon={<DatabaseIcon size={11} />}
          color="var(--color-accent)"
          label={t("resources.metric.disk")}
          hint={t("resources.mesh.diskHint")}
          value={diskText}
          active={lensKey === "disk"}
          lensColor={lensColor}
          bar={
            <CellBar
              ratio={denom.disk > 0 && disk ? disk.bytes / denom.disk : 0}
              tone={disk?.partial ? "warn" : "neutral"}
              show={denom.disk > 0 && !!disk}
            />
          }
        />
        <MetricCell
          icon={<DownloadIcon size={11} />}
          color="var(--color-domain-net)"
          label={t("resources.metric.net")}
          hint={netTitle}
          value={netText}
          active={lensKey === "down"}
          lensColor={lensColor}
          bar={
            <CellBar
              ratio={denom.down > 0 && net ? net.downBps / denom.down : 0}
              tone="net"
              show={denom.down > 0 && !!net && net.downBps > 0}
            />
          }
        />
      </div>
    </button>
  );
}

/** 卡头小簇：域色淡图标 + mono 数字（语义全在图表与 tooltip 里，不占字） */
function Cluster({
  icon,
  value,
  title,
  tone,
  dot = false,
}: {
  icon: ReactNode;
  value: string;
  title: string;
  tone?: "warn";
  /** 对外暴露端口标记点（琥珀） */
  dot?: boolean;
}) {
  return (
    <span className="flex shrink-0 items-center gap-1" title={title}>
      <span aria-hidden className="inline-flex text-ink-faint">
        {icon}
      </span>
      <span
        className={`font-mono text-[10px] tabular-nums ${
          tone === "warn" ? "text-warn" : "text-ink-dim"
        }`}
      >
        {value}
      </span>
      {dot ? <i aria-hidden className="size-[3px] rounded-full bg-warn" /> : null}
    </span>
  );
}

/**
 * 卡内一格指标：域色图标（语义）+ 比例条（量感）+ mono 读数（精度）。
 * 激活维度（当前排序）读数同色加粗——透镜一眼可辨；超 100% CPU 用 danger 覆盖。
 */
function MetricCell({
  icon,
  color,
  label,
  hint,
  value,
  bar,
  active = false,
  lensColor,
  danger = false,
}: {
  icon: ReactNode;
  color: string;
  /** 无障碍名（图标无文字，读屏需要） */
  label: string;
  hint: string;
  value: string;
  bar: ReactNode;
  active?: boolean;
  lensColor: string;
  danger?: boolean;
}) {
  return (
    <div className="flex min-w-0 items-center gap-1.5" title={`${label} · ${hint}`}>
      <span aria-hidden className="inline-flex shrink-0" style={{ color }}>
        {icon}
      </span>
      <span className="min-w-0 flex-1" aria-hidden>
        {bar}
      </span>
      <span
        aria-label={`${label}: ${value}`}
        className={`shrink-0 font-mono text-[10.5px] tabular-nums ${
          danger ? "text-danger" : active ? "font-semibold" : "text-ink-dim"
        }`}
        style={!danger && active ? { color: lensColor } : undefined}
      >
        {value}
      </span>
    </div>
  );
}

/** 条形基准的小图形（两根等比横条）：代替「XX 条形基准」五个字，口径留在 tooltip */
function BenchGlyph() {
  return (
    <span aria-hidden className="flex h-[9px] w-[9px] flex-col justify-between">
      <i className="h-[2px] w-full rounded-full bg-current" />
      <i className="h-[2px] w-2/3 rounded-full bg-current" />
    </span>
  );
}
