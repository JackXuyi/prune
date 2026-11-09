/**
 * 面积走势图（UI v4 §5.1）：CPU 10 min / 网络双方向共用。
 *
 * 与 Sparkline（纯描边）的分工：AreaChart 带「渐变面积填充」，科技感更强；
 * 点数不足 2 不渲染——一条线段画不出「尖峰还是持续」（沿用 StatsBoard 既有红线）。
 * 纵轴按 max 归一化（走势只看形状，不看绝对值——绝对值在主读数上）。
 */
export function AreaChart({
  values,
  color,
  height = 26,
  fillOpacity = 0.22,
  strokeWidth = 1.6,
  className = "",
  ariaLabel,
}: {
  values: number[];
  /** 任意 CSS 颜色，推荐传 var(--color-*) */
  color: string;
  height?: number;
  fillOpacity?: number;
  strokeWidth?: number;
  className?: string;
  ariaLabel?: string;
}) {
  const w = 100;
  const h = height;
  if (values.length < 2) return null;

  const max = Math.max(...values, 1e-9);
  const step = w / (values.length - 1);
  const y = (v: number) => h - 1.5 - (Math.max(0, v) / max) * (h - 3);

  const pts = values.map((v, i) => [i * step, y(v)] as const);
  const line = pts.map(([x, py], i) => `${i === 0 ? "M" : "L"}${x.toFixed(2)} ${py.toFixed(2)}`).join(" ");
  const area = `${line} L${w} ${h} L0 ${h} Z`;
  const gid = `area-${Math.abs(hash(values.join(",")))}`;

  return (
    <svg
      className={className}
      width="100%"
      height={h}
      viewBox={`0 0 ${w} ${h}`}
      preserveAspectRatio="none"
      role="img"
      aria-label={ariaLabel}
    >
      <defs>
        <linearGradient id={gid} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stopColor={color} stopOpacity={fillOpacity} />
          <stop offset="100%" stopColor={color} stopOpacity={0.02} />
        </linearGradient>
      </defs>
      <path d={area} fill={`url(#${gid})`} />
      <path d={line} fill="none" stroke={color} strokeWidth={strokeWidth} vectorEffect="non-scaling-stroke" />
    </svg>
  );
}

/** 稳定 hash：避免每帧生成新 gradient id 导致 SVG 重建 */
function hash(s: string): number {
  let h = 5381;
  for (let i = 0; i < s.length; i++) h = (h * 33) ^ s.charCodeAt(i);
  return h;
}
