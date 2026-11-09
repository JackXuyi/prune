/**
 * ThresholdBar：单个数值 vs 固定阈值的横向条（UX v3 §7.2 / §7.6）。
 *
 * 用于「数值要和已知阈值比」的场景：网络诊断三层耗时（50 / 30 / 100 ms）。
 * 超阈值自动 danger 色，阈值位置画虚线刻度——用户不用记阈值数字。
 */
export function ThresholdBar({
  label,
  value,
  threshold,
  unit,
  decimals,
  className = "",
}: {
  label: string;
  value: number | null;
  threshold: number;
  unit: string;
  /** 小数位；缺省 ≥1 取整、<1 保留一位 */
  decimals?: number;
  className?: string;
}) {
  if (value == null || !Number.isFinite(value)) {
    return (
      <div className={className}>
        <div className="flex items-baseline justify-between text-[11px]">
          <span className="text-ink-dim">{label}</span>
          <span className="text-ink-faint">—</span>
        </div>
      </div>
    );
  }
  const max = Math.max(threshold * 1.5, value * 1.05, 1);
  const pct = Math.max(0, Math.min(100, (value / max) * 100));
  const over = value > threshold;
  const text =
    decimals != null
      ? value.toFixed(decimals)
      : value >= 1
        ? String(Math.round(value))
        : value.toFixed(1);
  return (
    <div className={className}>
      <div className="flex items-baseline justify-between text-[11px]">
        <span className="text-ink-dim">{label}</span>
        <span className={`font-mono tabular-nums ${over ? "text-danger" : "text-ink"}`}>
          {text} {unit}
        </span>
      </div>
      <div
        role="img"
        aria-label={`${label} ${text} ${unit}，阈值 ${threshold} ${unit}${over ? "，已超阈" : "，正常"}`}
        className="relative mt-1 h-1.5 w-full rounded-full bg-white/[0.05]"
      >
        <div
          className={`h-full rounded-full transition-[width] duration-500 ease-out ${
            over ? "bg-danger" : "bg-accent"
          }`}
          style={{ width: `${pct}%` }}
        />
        <div
          aria-hidden
          className="pointer-events-none absolute -bottom-[3px] -top-[3px] border-l border-dashed border-white/35"
          style={{ left: `${(threshold / max) * 100}%` }}
        />
      </div>
    </div>
  );
}
