/**
 * MeterBar：单值占比条（UX v3 §7.2 / §7.6）。
 *
 * 与 primitives.Meter 的差别：① `value/max` 输入而非现成 ratio；② 支持阈值
 * 虚线刻度（如 swap 的 75 / 90）；③ **空值不渲染填充也不画 0 长度条**——
 * 「读不到」与「为 0」是两件事（M14 口径红线在图表层的落点）。
 * 数字永远由调用方放在条上方的文本位，本组件不重复渲染数值。
 */
import { useI18n } from "../../i18n/store";
import type { Tone } from "../../lib/statsReadout";
import { TONE_FILL } from "./tone";

export function MeterBar({
  value,
  max,
  tone = "accent",
  thresholds = [],
  height = 6,
  label,
  className = "",
}: {
  /** null = 读不到：不渲染填充 */
  value: number | null;
  max: number;
  tone?: Tone;
  /** 0–1 比例位置，如 swap 的 [0.75, 0.9]；≥0.9 的刻度画 danger 色 */
  thresholds?: number[];
  height?: number;
  /** 无障碍标签；缺省用占比描述 */
  label?: string;
  className?: string;
}) {
  const { t } = useI18n();
  const ratio =
    value != null && max > 0 && Number.isFinite(value)
      ? Math.max(0, Math.min(1, value / max))
      : null;
  return (
    <div
      role="img"
      aria-label={
        label ??
        (ratio != null
          ? t("chart.meter.ratio", { pct: Math.round(ratio * 100) })
          : t("chart.meter.unavailable"))
      }
      className={`relative w-full rounded-full bg-chip ${className}`}
      style={{ height }}
    >
      {ratio != null ? (
        <div
          className={`h-full rounded-full transition-[width] duration-500 ease-out ${TONE_FILL[tone]}`}
          style={{ width: `${ratio * 100}%` }}
        />
      ) : null}
      {thresholds.map((t) => (
        <div
          key={t}
          aria-hidden
          className={`pointer-events-none absolute -bottom-[3px] -top-[3px] border-l border-dashed ${
            t >= 0.9 ? "border-danger/70" : "border-white/35"
          }`}
          style={{ left: `${Math.max(0, Math.min(1, t)) * 100}%` }}
        />
      ))}
    </div>
  );
}
