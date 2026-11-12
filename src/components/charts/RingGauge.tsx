/**
 * 环形仪表（UI v4 §5.1）：可回收 Hero 环 / 内存压力环 / 卷容量环共用。
 *
 * 纯 SVG，零依赖。segments 按数值比例分弧，段间留 2° 缺口让「四段构成」一眼可分；
 * empty=true 时只画虚线空轨——「尚未扫描」绝不画 0（假事实红线）。
 * children 渲染在环心（主数字 + 标签）。
 */
import type { ReactNode } from "react";

export interface RingSegment {
  value: number;
  /** 任意 CSS 颜色，推荐传 var(--color-*) 引用 token */
  color: string;
  /** true 时降透明度（如 L4「只给命令不计入」） */
  dim?: boolean;
}

export function RingGauge({
  segments,
  size = 132,
  stroke = 12,
  gapDeg = 2.5,
  empty = false,
  trackColor = "var(--color-chip)",
  className = "",
  children,
  ariaLabel,
}: {
  segments: RingSegment[];
  size?: number;
  stroke?: number;
  /** 段间缺口角度（度） */
  gapDeg?: number;
  /** 未扫描态：只画虚线空轨 */
  empty?: boolean;
  trackColor?: string;
  className?: string;
  children?: ReactNode;
  ariaLabel?: string;
}) {
  const r = (size - stroke) / 2;
  const c = 2 * Math.PI * r;
  const cx = size / 2;
  const total = segments.reduce((s, seg) => s + Math.max(0, seg.value), 0);
  const gap = (gapDeg / 360) * c;

  // 累计偏移（沿顺时针，从 12 点方向起）
  let acc = 0;
  const arcs = segments.map((seg, i) => {
    const frac = total > 0 ? Math.max(0, seg.value) / total : 0;
    const len = Math.max(0, frac * c - (total > 0 ? gap : 0));
    const offset = acc;
    acc += frac * c;
    return { key: i, len, offset, color: seg.color, dim: seg.dim };
  });

  return (
    <div className={`relative inline-grid place-items-center ${className}`} style={{ width: size, height: size }}>
      <svg width={size} height={size} viewBox={`0 0 ${size} ${size}`} role="img" aria-label={ariaLabel}>
        <circle
          cx={cx}
          cy={cx}
          r={r}
          fill="none"
          stroke={trackColor}
          strokeWidth={stroke}
          strokeDasharray={empty ? "3 5" : undefined}
        />
        {!empty
          ? arcs.map((a) =>
              a.len > 0 ? (
                <circle
                  key={a.key}
                  cx={cx}
                  cy={cx}
                  r={r}
                  fill="none"
                  stroke={a.color}
                  strokeWidth={stroke}
                  strokeLinecap="butt"
                  opacity={a.dim ? 0.45 : 1}
                  strokeDasharray={`${a.len} ${c - a.len}`}
                  strokeDashoffset={-a.offset}
                  transform={`rotate(-90 ${cx} ${cx})`}
                />
              ) : null,
            )
          : null}
      </svg>
      {children ? <div className="absolute inset-0 grid place-items-center">{children}</div> : null}
    </div>
  );
}
