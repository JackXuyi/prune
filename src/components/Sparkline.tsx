import { useId } from "react";

import { useI18n } from "../i18n/store";

/**
 * 自绘 SVG sparkline（不引图表库，符合方案 §9 的自耗预算）。
 * 只负责画线，不带坐标轴——数值由外层以文字给出。
 *
 * `tone="info"` 是 M14 加的：网络 tab 的上下行必须是两条可区分的线，
 * 而且**各自归一化**（量级常差 10 倍，共享纵轴会把上传压成直线）。
 */
export function Sparkline({
  values,
  height = 64,
  tone = "accent",
  baselineZero = false,
}: {
  values: number[];
  height?: number;
  /** `info` 是 M14 加的：上下行两条线必须可区分 */
  tone?: "accent" | "warn" | "info";
  /**
   * 纵轴从 0 起（M16）。
   *
   * 默认按自身 min/max 归一化，适合「形态」比「绝对值」重要的曲线；但网速在
   * 空闲时会在 2–8 KB/s 之间抖动，min/max 归一化会把这层噪声画成满屏尖峰，
   * 看起来像在跑满带宽。统计页的迷你走势必须从 0 起。
   */
  baselineZero?: boolean;
}) {
  const { t } = useI18n();
  const gradientId = useId().replace(/:/g, "");

  if (values.length < 2) {
    return (
      <div
        className="flex items-center justify-center rounded-lg border border-dashed border-hairline-soft text-[11px] text-ink-faint"
        style={{ height: 48 }}
      >
        {values.length === 0 ? t("chart.sparkline.emptySeries") : t("chart.sparkline.accumulating")}
      </div>
    );
  }

  const W = 100;
  const H = 32;
  const max = Math.max(...values);
  const min = baselineZero ? 0 : Math.min(...values);
  const span = max - min || Math.max(1, max * 0.02);


  const coords = values.map((v, i) => {
    const x = (i / (values.length - 1)) * W;
    const y = H - ((v - min) / span) * (H - 4) - 2;
    return [x, y] as const;
  });

  const line = coords
    .map(([x, y], i) => `${i === 0 ? "M" : "L"}${x.toFixed(2)} ${y.toFixed(2)}`)
    .join(" ");
  const area = `${line} L${W} ${H} L0 ${H} Z`;

  const stroke = tone === "warn" ? "#f5b74a" : tone === "info" ? "#6aa9ff" : "#34d399";

  return (
    <svg
      viewBox={`0 0 ${W} ${H}`}
      preserveAspectRatio="none"
      style={{ height }}
      className="w-full"
      role="img"
      aria-label={t("chart.sparkline.aria")}
    >
      <defs>
        <linearGradient id={gradientId} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stopColor={stroke} stopOpacity="0.28" />
          <stop offset="100%" stopColor={stroke} stopOpacity="0" />
        </linearGradient>
      </defs>
      <path d={area} fill={`url(#${gradientId})`} />
      <path
        d={line}
        fill="none"
        stroke={stroke}
        strokeWidth={1.5}
        strokeLinejoin="round"
        strokeLinecap="round"
        vectorEffect="non-scaling-stroke"
      />
    </svg>
  );
}
