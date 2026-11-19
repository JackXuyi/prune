import { useI18n } from "../../i18n/store";
import { formatBytes, formatRelative } from "../../lib/format";
import type { Totals } from "../../types/disk";
import { RefreshIcon } from "../icons";
import { StackBar } from "../charts/StackBar";

/**
 * 存储构成 Hero（eul 重设计 · storage-eul-mockup.html ②）：
 * 可回收大数字（全页唯一 25px+ 读数）+ L1–L4 构成堆叠条 + 图例。
 *
 * 诚实红线：可回收 = L1+L2+L3 **不含 L4**——L4 走 muted 斜纹段，
 * 图例注明「不计入」；四段之和 ≠ 可回收，视觉上不允许混淆。
 * 右侧是扫描口径块（何时 / 耗时 / 增量 / 重扫），重扫不改勾选。
 */
export function StHero({
  totals,
  scannedAt,
  scanElapsedMs,
  incremental,
  groupCount,
  scanning,
  onRescan,
}: {
  totals: Totals;
  scannedAt: number;
  scanElapsedMs: number;
  incremental: boolean;
  groupCount: number;
  scanning: boolean;
  onRescan: () => void;
}) {
  const { t, locale } = useI18n();
  const hatch =
    "repeating-linear-gradient(45deg, var(--color-hatch) 0 2px, transparent 2px 4px)";

  return (
    <section
      className="relative flex h-[118px] shrink-0 items-center gap-4 overflow-hidden rounded-[16px] border px-[18px] py-3"
      style={{
        borderColor: "color-mix(in srgb, var(--color-accent) 26%, transparent)",
        backgroundImage:
          "linear-gradient(120deg, color-mix(in srgb, var(--color-accent) 9%, transparent), color-mix(in srgb, var(--color-accent) 2%, transparent) 46%, transparent 76%)",
        backgroundColor: "var(--color-surface-raised)",
      }}
      title={t("disk.hero.tip")}
    >
      <div className="min-w-0 flex-1">
        {/* 主读数行：只有一个大数字 */}
        <div className="flex items-baseline gap-2.5">
          <span className="font-mono text-[25px] leading-none font-semibold tabular-nums text-accent">
            {formatBytes(totals.reclaimable)}
          </span>
          <span className="min-w-0 truncate text-[10px] text-ink-faint">
            {t("disk.hero.lead")}
          </span>
        </div>

        {/* 构成堆叠条：L1 accent / L2 info / L3 warn / L4 muted 斜纹 */}
        <StackBar
          className="mt-2.5"
          height={11}
          ariaLabel={t("disk.stack.aria", {
            l1: formatBytes(totals.deadweight),
            l2: formatBytes(totals.l2),
            l3: formatBytes(totals.l3),
            l4: formatBytes(totals.l4),
          })}
          segments={[
            { key: "l1", label: `L1 ${t("disk.tier.l1")}`, value: totals.deadweight, tone: "accent" },
            { key: "l2", label: `L2 ${t("disk.tier.l2")}`, value: totals.l2, tone: "info" },
            { key: "l3", label: `L3 ${t("disk.tier.l3")}`, value: totals.l3, tone: "warn" },
            { key: "l4", label: `L4 ${t("disk.tier.l4")}`, value: totals.l4, muted: true },
          ]}
        />

        {/* 图例：色块 + 值；L4 斜纹 + 「不计入」 */}
        <div className="mt-2 flex flex-wrap gap-x-3 gap-y-1">
          <LegendDot swatch="var(--color-accent)" label={`L1 ${t("disk.level.l1")}`} value={formatBytes(totals.deadweight)} tip={t("disk.tier.l1Hint")} />
          <LegendDot swatch="var(--color-info)" label={`L2 ${t("disk.level.l2")}`} value={formatBytes(totals.l2)} tip={t("disk.tier.l2Hint")} />
          <LegendDot swatch="var(--color-warn)" label={`L3 ${t("disk.level.l3")}`} value={formatBytes(totals.l3)} tip={t("disk.tier.l3Hint")} />
          {totals.l4 > 0 ? (
            <LegendDot swatch={hatch} label={`L4 ${t("disk.level.l4")}`} value={formatBytes(totals.l4)} tip={t("disk.hero.l4Tip")} />
          ) : null}
        </div>
      </div>

      {/* 扫描口径块：何时 / 耗时 / 增量 / 组数 / 重扫（重扫不改勾选，picked 由 scannedAt 变化统一重置） */}
      <div className="flex w-[168px] shrink-0 flex-col gap-1.5">
        <div className="font-mono text-[13px] leading-none tabular-nums text-ink">
          {formatRelative(scannedAt, locale)}
          <span className="ml-1.5 font-sans text-[9.5px] font-normal text-ink-faint">
            {t("disk.hero.scanWhen", { secs: (scanElapsedMs / 1000).toFixed(1) })}
          </span>
        </div>
        <div className="flex items-center gap-1.5 text-[9.5px] text-ink-faint">
          {incremental ? (
            <span className="rounded-full bg-info/12 px-[7px] py-px text-[9px] text-info">
              {t("disk.hero.incrTag")}
            </span>
          ) : null}
          <span className="min-w-0 truncate">{t("disk.hero.scanMeta", { n: groupCount })}</span>
        </div>
        <button
          type="button"
          onClick={onRescan}
          disabled={scanning}
          className="mt-0.5 flex h-[26px] items-center justify-center gap-1.5 rounded-[8px] border border-hairline bg-chip text-[11px] text-ink-dim transition-colors hover:text-ink disabled:opacity-40"
        >
          <RefreshIcon size={11} className={scanning ? "animate-spin" : ""} />
          {scanning ? t("common.progress.scanning") : t("common.action.rescan")}
        </button>
      </div>
    </section>
  );
}

function LegendDot({
  swatch,
  label,
  value,
  tip,
}: {
  swatch: string;
  label: string;
  value: string;
  tip: string;
}) {
  return (
    <span className="flex items-center gap-1.5 text-[9.5px] text-ink-faint" title={tip}>
      <em className="size-[7px] shrink-0 rounded-[2px]" style={{ background: swatch }} aria-hidden />
      {label}
      <b className="font-mono text-[10.5px] font-medium tabular-nums text-ink-dim">{value}</b>
    </span>
  );
}
