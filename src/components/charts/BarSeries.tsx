import { useI18n } from "../../i18n/store";

/**
 * 柱形序列（D5）：应用详情的「近 7 天 / 24 h」三轨历史用。
 *
 * 三条硬规则：
 * 1. **缺数据不补 0**：`hasData=false` 画 1px 灰虚线，读起来是「没有记录」而不是「值为 0」。
 * 2. **峰值高亮**：最大值那根用满不透明色，其余降到 70%，一眼看出最高点。
 * 3. **纯 SVG/DOM，无依赖**：柱是 div，高度用百分比，容器高度由调用方定。
 */
export interface BarDatum {
  /** 展示用标签（tooltip 首行） */
  label: string;
  value: number;
  hasData: boolean;
}

export function BarSeries({
  bars,
  color,
  height = 30,
  peakHighlight = true,
  ariaLabel,
  formatValue = (v) => String(Math.round(v)),
}: {
  bars: BarDatum[];
  /** 任意 CSS 颜色（推荐 var(--color-*)） */
  color: string;
  height?: number;
  peakHighlight?: boolean;
  ariaLabel: string;
  formatValue?: (v: number) => string;
}) {
  const { t } = useI18n();
  const max = bars.reduce((m, b) => (b.hasData ? Math.max(m, b.value) : m), 0);
  const peak = bars.reduce((idx, b, i) => {
    if (!b.hasData) return idx;
    const cur = bars[idx];
    return !cur?.hasData || b.value > cur.value ? i : idx;
  }, 0);

  return (
    <div
      className="flex items-end gap-[3px]"
      style={{ height }}
      role="img"
      aria-label={ariaLabel}
    >
      {bars.map((b, i) => {
        const ratio = max > 0 && b.hasData ? Math.max(0.02, b.value / max) : 0;
        const isPeak = peakHighlight && i === peak && b.hasData && max > 0;
        return (
          <div
            key={`${b.label}-${i}`}
            className="flex min-w-0 flex-1 flex-col justify-end"
            title={
              b.hasData
                ? `${b.label} · ${formatValue(b.value)}`
                : t("chart.bars.noRecord", { label: b.label })
            }
          >
            {b.hasData && b.value > 0 ? (
              <i
                className="block w-full rounded-t-[2px]"
                style={{
                  height: `${ratio * 100}%`,
                  background: color,
                  opacity: isPeak ? 1 : 0.72,
                  boxShadow: isPeak ? `0 0 6px color-mix(in srgb, ${color} 55%, transparent)` : undefined,
                }}
              />
            ) : (
              <i
                className="block w-full"
                style={{
                  height: 1,
                  borderTop: b.hasData
                    ? `1px solid color-mix(in srgb, ${color} 55%, transparent)`
                    : "1px dashed rgba(255,255,255,0.16)",
                }}
              />
            )}
          </div>
        );
      })}
    </div>
  );
}
