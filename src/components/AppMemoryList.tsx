import { useMemo } from "react";

import { formatBytes, formatPercent } from "../lib/format";
import { isExact, swappedRatio, totalOf, type FamilyMem, type ProcMem } from "../types/mem";
import { Meter } from "./primitives";

/** 口径说明：精确 / 部分深探 / 纯 RSS */
function coverageLabel(f: FamilyMem): string {
  if (f.footprint == null) return "RSS 口径";
  if (isExact(f)) return "精确";
  return `深探 ${f.deepCovered}/${f.processCount} · ≈ 下界`;
}

/**
 * 每个应用的绝对内存排行。
 *
 * 数值口径：`total()` = footprint（含被换出到磁盘的部分）；未深探的子进程按
 * RSS 计入，因此带 `≈` 的行是**下界**，不会虚高。`compact` 供 380px 面板用，
 * 完整版供主窗口用。
 */
export function AppMemoryList({
  families,
  processes,
  limit,
  compact = false,
  onOpen,
}: {
  families: FamilyMem[];
  processes: ProcMem[];
  limit?: number;
  compact?: boolean;
  onOpen?: (pid: number) => void;
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
                      <span className="min-w-0 flex-1 truncate text-[12px] leading-tight text-ink">
                        {f.name}
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
                      </span>
                      <span className="mt-0.5 block truncate text-[10px] leading-tight text-ink-faint">
                        {f.processCount} 个进程 · {coverageLabel(f)}
                        {ratio != null ? ` · ${formatPercent(ratio)} 在磁盘` : ""}
                      </span>
                    </span>
                    <Meter ratio={total / max} tone={tone} height={4} />
                    <span className="text-right font-mono text-[13px] leading-tight tabular-nums text-ink">
                      {exact ? "" : "≈"}
                      {formatBytes(total)}
                    </span>
                  </div>
                )}
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
