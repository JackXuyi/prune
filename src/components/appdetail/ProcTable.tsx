/**
 * D6：应用详情的进程明细表。
 *
 * - 真 `<table>` + `th[scope]`：读屏能报出列名，而不是一串 div；
 * - 排序只在本地做（45 行以内，不需要 IPC）；
 * - 右键菜单与「内存」tab 共用 `ProcKillMenu`（两步确认在那边实现，这里只抛事件）。
 */
import { useMemo, useState } from "react";

import { useI18n } from "../../i18n/store";
import type { NoParamKey } from "../../i18n/types";
import { formatBytes } from "../../lib/format";
import type { ProcMem } from "../../types/mem";

type SortKey = "mem" | "cpu" | "swapped" | "name";

const COLUMNS: { key: SortKey; labelKey: NoParamKey; className: string }[] = [
  { key: "name", labelKey: "resources.proc.colName", className: "text-left" },
  { key: "cpu", labelKey: "resources.proc.colCpu", className: "text-right" },
  { key: "mem", labelKey: "resources.proc.colMem", className: "text-right" },
  { key: "swapped", labelKey: "resources.proc.colSwapped", className: "text-right" },
];

export function ProcTable({
  members,
  onOpenProcess,
  onKillMenu,
}: {
  members: ProcMem[];
  onOpenProcess?: (pid: number) => void;
  onKillMenu?: (proc: ProcMem, x: number, y: number) => void;
}) {
  const [sortKey, setSortKey] = useState<SortKey>("mem");
  const [desc, setDesc] = useState(true);
  const { t } = useI18n();

  const sorted = useMemo(() => {
    const val = (p: ProcMem): number | null => {
      switch (sortKey) {
        case "cpu":
          return p.cpu ?? null;
        case "swapped":
          return p.swapped ?? null;
        case "mem":
          return p.footprint ?? p.rss;
        default:
          return null;
      }
    };
    return [...members].sort((a, b) => {
      if (sortKey === "name") {
        return desc ? b.name.localeCompare(a.name) : a.name.localeCompare(b.name);
      }
      const va = val(a);
      const vb = val(b);
      // 缺值永远排最后（不当 0 参与排序）
      if (va == null && vb == null) return a.pid - b.pid;
      if (va == null) return 1;
      if (vb == null) return -1;
      if (va === vb) return a.pid - b.pid;
      return desc ? vb - va : va - vb;
    });
  }, [members, sortKey, desc]);

  const pick = (key: SortKey) => {
    if (key === sortKey) setDesc((d) => !d);
    else {
      setSortKey(key);
      setDesc(true);
    }
  };

  return (
    <section className="rounded-xl border border-hairline-soft bg-white/[0.02]">
      <div className="flex items-center gap-2 border-b border-hairline-soft px-3 py-2">
        <span className="text-[11px] font-medium text-ink">{t("resources.proc.title")}</span>
        <span className="font-mono text-[10px] tabular-nums text-ink-faint">
          {members.length}
        </span>
        <div className="grow" />
        <span className="text-[9px] text-ink-faint">{t("resources.proc.hint")}</span>
      </div>

      <table className="w-full table-fixed border-collapse text-[10.5px]">
        <thead>
          <tr className="text-[9px] text-ink-faint">
            {COLUMNS.map((c) => {
              const label = t(c.labelKey);
              return (
                <th
                  key={c.key}
                  scope="col"
                  className={`px-2 py-1 font-normal ${c.className} ${
                    c.key === "name" ? "w-[minmax(0,1fr)]" : "w-[68px]"
                  }`}
                >
                  <button
                    type="button"
                    onClick={() => pick(c.key)}
                    className="transition-colors hover:text-ink-dim"
                    title={
                      c.key === sortKey
                        ? t("resources.sortTipToggle", { label })
                        : t("resources.sortTip", { label })
                    }
                  >
                    {label}
                    {c.key === sortKey ? <span className="ml-0.5">{desc ? "▾" : "▴"}</span> : null}
                  </button>
                </th>
              );
            })}
            <th scope="col" className="w-[minmax(0,1fr)] px-2 py-1 text-left font-normal">
              {t("resources.proc.colWorkspace")}
            </th>
          </tr>
        </thead>
        <tbody>
          {sorted.map((p) => (
            <tr
              key={p.pid}
              tabIndex={0}
              onClick={() => onOpenProcess?.(p.pid)}
              onKeyDown={(e) => {
                if (e.key === "Enter") onOpenProcess?.(p.pid);
              }}
              onContextMenu={(e) => {
                if (!onKillMenu) return;
                e.preventDefault();
                onKillMenu(p, e.clientX, e.clientY);
              }}
              className="cursor-pointer border-t border-hairline-soft/60 transition-colors hover:bg-white/[0.04] focus:bg-white/[0.06] focus:outline-none"
              title={`${p.name} · pid ${p.pid}${p.exe ? `\n${p.exe}` : ""}`}
            >
              <td className="truncate px-2 py-[5px] text-ink">{p.name}</td>
              <td
                className={`px-2 py-[5px] text-right font-mono tabular-nums ${
                  p.cpu != null && p.cpu >= 100 ? "text-danger" : "text-domain-cpu"
                }`}
              >
                {p.cpu == null ? "—" : p.cpu < 1 ? "<1%" : `${Math.round(p.cpu)}%`}
              </td>
              <td className="px-2 py-[5px] text-right font-mono tabular-nums text-ink-dim">
                {formatBytes(p.footprint ?? p.rss)}
                {p.footprint == null ? "*" : ""}
              </td>
              <td className="px-2 py-[5px] text-right font-mono tabular-nums text-warn">
                {p.swapped == null ? "—" : formatBytes(p.swapped)}
              </td>
              <td className="truncate px-2 py-[5px] text-[9.5px] text-ink-faint">
                {p.workspace?.path ?? "—"}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      {sorted.some((p) => p.footprint == null) ? (
        <p className="px-3 pb-2 pt-1 text-[9px] text-ink-faint">{t("resources.proc.rssNote")}</p>
      ) : null}
    </section>
  );
}
