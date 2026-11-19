import { useI18n } from "../../i18n/store";
import { formatBytes, formatPercent, formatSignedBytes } from "../../lib/format";
import { swappedRatio, type MemSnapshot, type ProcMem, type SwapRateReport } from "../../types/mem";
import { CellBar } from "../CellBar";
import { useMemStore } from "../../stores/memStore";

/**
 * 换出大户表（eul 重设计 · memory-eul-mockup.html ⑤）：
 * 列头 + 网格行——[进程 | 换出比例条 | 已换出 | Δ24h | 占比]。
 *
 * F24 语义不变：Δ 列只在有增速数据时出现（Windows 无每进程换出口径 →
 * 恒无数据不画空列，列模板随之收窄）；Δ 负值 = 换回内存/释放，不是脏数据。
 * 行 hover 右键结束进程的交互保留。
 */
export function VictimsBoard({
  snapshot,
  swapRates,
  onKillMenu,
}: {
  snapshot: MemSnapshot;
  swapRates: SwapRateReport | null;
  onKillMenu: (proc: ProcMem, x: number, y: number) => void;
}) {
  const { t } = useI18n();
  const { topSwapped } = snapshot;
  const hasDelta = (swapRates?.rates.length ?? 0) > 0;
  const deltaByPid = new Map((swapRates?.rates ?? []).map((r) => [r.pid, r.deltaBytes]));
  // Tailwind 任意值：类名必须是静态可扫描的完整 token（裸 "58px" 不会被生成）
  const cols = hasDelta
    ? "grid-cols-[minmax(0,1fr)_58px_62px_66px_46px]"
    : "grid-cols-[minmax(0,1fr)_58px_62px_46px]";

  if (topSwapped.length === 0) {
    return (
      <p className="py-1 text-[11px] text-ink-faint">{t("common.empty.swapped")}</p>
    );
  }

  return (
    <section className="flex min-h-0 flex-1 flex-col">
      <div className="flex shrink-0 items-baseline gap-2">
        <span className="text-[11px] font-semibold tracking-wide text-ink-dim">
          {t("memory.victims.title")}
        </span>
        <span className="min-w-0 flex-1 truncate text-[9.5px] text-ink-faint">
          {t("memory.victims.subtitle")}
        </span>
      </div>

      {/* 列头：与行共用一套网格模板 */}
      <div
        className={`mt-1 grid shrink-0 gap-2 px-1.5 text-right text-[9px] text-ink-faint ${cols}`}
      >
        <span className="text-left">{t("memory.victims.colProc")}</span>
        <span title={t("memory.swap.bandTip")}>{t("memory.victims.colSwapRatio")}</span>
        <span>{t("memory.victims.colSwapped")}</span>
        {hasDelta ? (
          <span title={t("memory.victims.deltaTip", { h: swapRates?.windowHours ?? 24 })}>
            {t("memory.victims.delta24h")}
          </span>
        ) : null}
        <span>{t("memory.victims.ratioCol")}</span>
      </div>

      <ul className="min-h-0 flex-1 overflow-y-auto">
        {topSwapped.map((p) => (
          <VictimRow
            key={`${p.pid}-${p.startTime}`}
            proc={p}
            delta={deltaByPid.get(p.pid) ?? null}
            deltaHours={swapRates?.windowHours ?? null}
            showDelta={hasDelta}
            onKillMenu={onKillMenu}
          />
        ))}
      </ul>
    </section>
  );
}

function VictimRow({
  proc,
  delta,
  deltaHours,
  showDelta,
  onKillMenu,
}: {
  proc: ProcMem;
  /** F24：窗口首末样本差分（字节）；null = 不在增速榜（样本不足/pid 复用剔除），显示「—」 */
  delta: number | null;
  deltaHours: number | null;
  /** Windows 无口径 → 整列不画（列头与行同步收窄） */
  showDelta: boolean;
  onKillMenu: (proc: ProcMem, x: number, y: number) => void;
}) {
  const { t } = useI18n();
  const select = useMemStore((s) => s.select);
  const ratio = swappedRatio(proc);
  const swapped = proc.swapped ?? null;
  // 与列头同一套网格模板（列头/行必须共用，否则列对不上）
  const cols = showDelta
    ? "grid-cols-[minmax(0,1fr)_58px_62px_66px_46px]"
    : "grid-cols-[minmax(0,1fr)_58px_62px_46px]";

  return (
    <li>
      <button
        type="button"
        onClick={() => select(proc.pid)}
        onContextMenu={(e) => {
          e.preventDefault();
          onKillMenu(proc, e.clientX, e.clientY);
        }}
        title={t("memory.rowHint")}
        className={`grid w-full items-center gap-2 rounded-[7px] px-1.5 py-[5.5px] text-right transition-colors hover:bg-chip ${cols}`}
      >
        <span className="flex min-w-0 items-center gap-1.5 text-left">
          <b className="min-w-0 truncate text-[11.5px] font-medium text-ink">{proc.name}</b>
          {proc.family !== proc.name ? (
            <span className="shrink-0 text-[9px] text-ink-faint">{proc.family}</span>
          ) : null}
        </span>
        <span className="min-w-0">
          {ratio != null && ratio > 0 ? (
            <CellBar ratio={Math.min(1, ratio)} tone="warn" show />
          ) : (
            <span className="block h-[3px] w-full rounded-[2px] bg-chip" aria-hidden />
          )}
        </span>
        <span className="font-mono text-[10.5px] tabular-nums text-ink-dim">
          {swapped != null ? formatBytes(swapped) : "—"}
        </span>
        {showDelta ? (
          <span
            className={`font-mono text-[10.5px] tabular-nums ${
              (delta ?? 0) > 0 ? "text-warn" : "text-ink-faint"
            }`}
            title={deltaHours != null ? t("memory.victims.deltaTip", { h: deltaHours }) : undefined}
          >
            {formatSignedBytes(delta) ?? "—"}
          </span>
        ) : null}
        <span className="font-mono text-[10.5px] tabular-nums text-ink-faint">
          {ratio != null ? formatPercent(ratio) : "—"}
        </span>
      </button>
    </li>
  );
}
