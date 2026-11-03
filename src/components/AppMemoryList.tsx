import { useMemo } from "react";

import { formatBytes, formatPercent } from "../lib/format";
import {
  isExact,
  leaksOfFamily,
  swappedRatio,
  totalGrowth,
  totalOf,
  type FamilyMem,
  type LeakReport,
  type ProcMem,
  type WorkspaceMem,
  type WorkspaceSource,
} from "../types/mem";
import { Meter } from "./primitives";
import type { PortSnapshot } from "../types/net";

/** 口径说明：精确 / 部分深探 / 纯 RSS */
function coverageLabel(f: FamilyMem): string {
  if (f.footprint == null) return "RSS 口径";
  if (isExact(f)) return "精确";
  return `深探 ${f.deepCovered}/${f.processCount} · ≈ 下界`;
}

/** 归属来源的中文说明（鼠标悬停看，不占版面） */
const SOURCE_HINT: Record<WorkspaceSource, string> = {
  handle: "依据打开的文件句柄判定（最可靠）",
  label: "依据编辑器窗口标签 + 本地工作区记录判定",
  inherited: "子进程继承了所属窗口的工作区",
};

/** 一行项目：名称 + 绝对占用。面板窄，只显示前几条 */
function WorkspaceRow({
  ws,
  compact,
}: {
  ws: WorkspaceMem;
  compact: boolean;
}) {
  return (
    <div className="flex items-baseline gap-1.5">
      <span
        className={`min-w-0 flex-1 truncate ${compact ? "text-[10px]" : "text-[11px]"} text-ink-dim`}
        title={`${ws.path}\n${SOURCE_HINT[ws.source]}`}
      >
        {ws.label}
      </span>
      <span className="shrink-0 font-mono text-[10px] tabular-nums text-ink-faint">
        {ws.footprint == null ? "≈" : ""}
        {formatBytes(totalOf(ws))}
      </span>
    </div>
  );
}

/**
 * 每个应用的绝对内存排行。
 *
 * 数值口径：`total()` = footprint（含被换出到磁盘的部分）；未深探的子进程按
 * RSS 计入，因此带 `≈` 的行是**下界**，不会虚高。`compact` 供 380px 面板用，
 * 完整版供主窗口用。
 *
 * 有工作区概念的家族（Cursor / VS Code 等）会在行下列出各项目的占用，
 * 以及「未归属」余量——渲染进程这类拿不到项目线索的进程不会被摊进任何项目。
 */
export function AppMemoryList({
  families,
  processes,
  leaks,
  ports,
  limit,
  compact = false,
  onOpen,
  onOpenPorts,
}: {
  families: FamilyMem[];
  processes: ProcMem[];
  /** B5 判定结果：命中的家族行加「疑似泄漏」标记（措辞见 LeakCard 顶部注释） */
  leaks?: LeakReport;
  /** 端口快照（M7）：给「这个应用占着 3000」这类问题一个入口。null = 还没扫 */
  ports?: PortSnapshot | null;
  limit?: number;
  compact?: boolean;
  onOpen?: (pid: number) => void;
  /** 点端口徽标 → 跳到端口 tab 并定位该端口 */
  onOpenPorts?: (port: number) => void;
}) {
  const rows = useMemo(
    () => (limit ? families.slice(0, limit) : families),
    [families, limit],
  );
  const max = useMemo(() => Math.max(1, ...rows.map((f) => totalOf(f))), [rows]);

  /** 家族里最重的进程：点一行 → 直接看它 */
  const heaviest = useMemo(() => {
    const map = new Map<string, { pid: number; total: number }>();
    for (const p of processes) {
      const total = totalOf(p);
      const cur = map.get(p.family);
      if (!cur || total > cur.total) map.set(p.family, { pid: p.pid, total });
    }
    return map;
  }, [processes]);

  const hasEstimate = rows.some((f) => f.footprint != null && !isExact(f));

  /**
   * 家族 → 关注层端口。只认 watched：把 Electron 的内部 IPC 端口挂在应用行上
   * 等于给每一行都加噪音，反而让「谁占着 3000」更难找。
   */
  const portsByFamily = useMemo(() => {
    const map = new Map<string, number[]>();
    if (!ports) return map;
    const familyOf = new Map<number, string>();
    for (const p of processes) familyOf.set(p.pid, p.family);
    for (const e of ports.entries) {
      if (!e.watched) continue;
      for (const b of e.listeners) {
        if (b.pid == null) continue;
        const family = familyOf.get(b.pid);
        if (!family) continue;
        const list = map.get(family) ?? [];
        if (!list.includes(e.port)) list.push(e.port);
        map.set(family, list);
      }
    }
    for (const list of map.values()) list.sort((a, b) => a - b);
    return map;
  }, [ports, processes]);

  if (rows.length === 0) {
    return <p className="px-1 py-3 text-[11px] text-ink-faint">暂无应用数据</p>;
  }

  return (
    <div>
      <ul className="space-y-px">
        {rows.map((f, i) => {
          const total = totalOf(f);
          const exact = isExact(f);
          const pid = heaviest.get(f.name)?.pid;
          const ratio = swappedRatio(f);
          const familyLeaks = leaksOfFamily(leaks, f.name);
          const leakGrowth = totalGrowth(familyLeaks);
          const familyPorts = portsByFamily.get(f.name) ?? [];
          const tone =
            f.footprint == null ? "neutral" : exact ? "accent" : "warn";

          return (
            <li key={f.name}>
              <button
                type="button"
                disabled={!onOpen || pid == null}
                onClick={() => pid != null && onOpen?.(pid)}
                title={`${f.name} · ${coverageLabel(f)}`}
                className={`w-full rounded-md text-left transition-colors ${
                  compact ? "px-1.5 py-1.5" : "px-2 py-2"
                } ${onOpen ? "hover:bg-white/[0.045]" : ""}`}
              >
                {compact ? (
                  <>
                    <div className="flex items-baseline justify-between gap-2">
                      <span className="flex min-w-0 flex-1 items-baseline gap-1.5">
                        <span className="truncate text-[12px] leading-tight text-ink">
                          {f.name}
                        </span>
                        {familyLeaks.length > 0 ? (
                          <span
                            className="shrink-0 text-[9px] leading-tight text-warn"
                            title={`疑似内存泄漏：${familyLeaks.length} 个进程近 ${
                              familyLeaks[0].windowSecs / 60
                            } min 合计增长 ${formatBytes(leakGrowth)}（按曲线判断，不是结论）`}
                          >
                            疑似泄漏
                          </span>
                        ) : null}
                      </span>
                      <span className="shrink-0 font-mono text-[12px] leading-tight tabular-nums text-ink">
                        {exact ? "" : "≈"}
                        {formatBytes(total)}
                      </span>
                    </div>
                    <div className="mt-1 flex items-center gap-2">
                      <Meter
                        ratio={total / max}
                        tone={tone}
                        height={3}
                        className="min-w-0 flex-1"
                      />
                      <span className="shrink-0 text-[10px] text-ink-faint">
                        {f.processCount} 进程
                        {exact ? "" : f.footprint == null ? " · RSS" : " · ≈"}
                        {familyPorts.length > 0 ? (
                          <span
                            role={onOpenPorts ? "button" : undefined}
                            onClick={(e) => {
                              if (!onOpenPorts) return;
                              e.stopPropagation();
                              onOpenPorts(familyPorts[0]);
                            }}
                            title={familyPorts.map((p) => `:${p}`).join(" ")}
                            className={onOpenPorts ? "text-accent hover:underline" : ""}
                          >
                            {" · "}
                            {familyPorts.length} 端口
                          </span>
                        ) : null}
                      </span>
                    </div>
                  </>
                ) : (
                  <div className="grid grid-cols-[26px_minmax(0,1fr)_112px_86px] items-center gap-2.5">
                    <span className="font-mono text-[10px] tabular-nums text-ink-faint">
                      {String(i + 1).padStart(2, "0")}
                    </span>
                    <span className="min-w-0">
                      <span className="block truncate text-[13px] leading-tight text-ink">
                        {f.name}
                        {familyLeaks.length > 0 ? (
                          <span
                            className="ml-1.5 align-[1px] text-[10px] text-warn"
                            title={`疑似内存泄漏：${familyLeaks.length} 个进程近 ${
                              familyLeaks[0].windowSecs / 60
                            } min 合计增长 ${formatBytes(leakGrowth)}（按曲线判断，不是结论）`}
                          >
                            疑似泄漏 +{formatBytes(leakGrowth)}
                          </span>
                        ) : null}
                      </span>
                      <span className="mt-0.5 block truncate text-[10px] leading-tight text-ink-faint">
                        {f.processCount} 个进程 · {coverageLabel(f)}
                        {f.workspaces.length > 0
                          ? ` · ${f.workspaces.length} 个项目`
                          : ""}
                        {ratio != null ? ` · ${formatPercent(ratio)} 在磁盘` : ""}
                        {familyPorts.length > 0 ? (
                          <span
                            role={onOpenPorts ? "button" : undefined}
                            onClick={(e) => {
                              if (!onOpenPorts) return;
                              e.stopPropagation();
                              onOpenPorts(familyPorts[0]);
                            }}
                            title={`监听端口 ${familyPorts.map((p) => `:${p}`).join(" ")}`}
                            className={onOpenPorts ? "text-accent hover:underline" : ""}
                          >
                            {" · "}
                            {familyPorts.slice(0, 2).map((p) => `:${p}`).join(" ")}
                            {familyPorts.length > 2 ? ` +${familyPorts.length - 2}` : ""}
                          </span>
                        ) : null}
                      </span>
                    </span>
                    <Meter ratio={total / max} tone={tone} height={4} />
                    <span className="text-right font-mono text-[13px] leading-tight tabular-nums text-ink">
                      {exact ? "" : "≈"}
                      {formatBytes(total)}
                    </span>
                  </div>
                )}

                {f.workspaces.length > 0 ? (
                  <div
                    className={
                      compact
                        ? "mt-1 space-y-0.5 pl-1"
                        : "mt-1.5 space-y-0.5 pl-[36px]"
                    }
                  >
                    {(compact ? f.workspaces.slice(0, 3) : f.workspaces).map((w) => (
                      <WorkspaceRow key={w.path} ws={w} compact={compact} />
                    ))}
                    {compact && f.workspaces.length > 3 ? (
                      <div className="text-[10px] text-ink-faint">
                        还有 {f.workspaces.length - 3} 个项目
                      </div>
                    ) : null}
                    {f.wsUnattributedCount > 0 && f.wsUnattributedFootprint != null ? (
                      <div className="flex items-baseline gap-1.5">
                        <span
                          className="min-w-0 flex-1 truncate text-[10px] text-ink-faint"
                          title="这些进程没有任何可归属的项目线索（如 Electron 渲染进程），不摊进任何项目"
                        >
                          未归属 · {f.wsUnattributedCount} 进程
                        </span>
                        <span className="shrink-0 font-mono text-[10px] tabular-nums text-ink-faint">
                          {formatBytes(f.wsUnattributedFootprint)}
                        </span>
                      </div>
                    ) : null}
                  </div>
                ) : null}
              </button>
            </li>
          );
        })}
      </ul>

      {hasEstimate || rows.some((f) => f.footprint == null) ? (
        <p className="mt-2 px-1 text-[10px] leading-relaxed text-ink-faint">
          占用含被换出到磁盘的部分。<span className="text-warn">≈</span> = 部分子进程未深探，
          未探部分按 RSS 计入（下界，不会虚高）。
        </p>
      ) : null}
    </div>
  );
}

/** 顶部汇总：应用数 + 深探覆盖，避免用户把「≈」当成精确值 */
export function AppMemorySummary({ families }: { families: FamilyMem[] }) {
  const deep = families.filter((f) => f.footprint != null).length;
  return (
    <span className="text-[10px] text-ink-faint">
      {families.length} 个应用 · {deep} 个已深探
    </span>
  );
}
