import type { ReactNode } from "react";

/**
 * MiniGauge（eul 式图标优先 · 首页重设计）：图标位升级为环形仪表。
 *
 * 三通道分工（设计稿 stats-eul-mockup.html §④）：
 * - 环形进度弧 → 量感（离满还有多远，不读数字也有差距感）
 * - 环心域色图标 → 语义（这是哪个域）
 * - 旁侧 mono 数字 → 精确值（由调用方排版，这里只管环）
 *
 * 边界：仅百分比类读数套环（CPU / 内存 / 磁盘 / 电池）；速率、端口计数
 * 这类非百分比保持图标位——环只能表达「占比」，硬套会稀释「环=占比」语义。
 * `ratio` 为 null（读不到）时只画空轨道，绝不画 0 弧（假事实红线）。
 */
export function MiniGauge({
  ratio,
  color,
  size = 26,
  stroke = 2.2,
  icon,
  ariaLabel,
}: {
  /** 0–1 占比；null = 未就绪（空轨道 + 图标，不画弧） */
  ratio: number | null;
  /** 域色（CSS 颜色，推荐传 var(--color-*)），环弧与环心图标共用 */
  color: string;
  /** 渲染尺寸（px），viewBox 固定 24，整体等比缩放 */
  size?: number;
  stroke?: number;
  /** 环心图标（调用方负责传好尺寸，约 size×0.45） */
  icon: ReactNode;
  ariaLabel?: string;
}) {
  const R = 10.5;
  const C = 2 * Math.PI * R;
  const frac = ratio != null ? Math.max(0, Math.min(1, ratio)) : 0;
  return (
    <span
      className="relative grid shrink-0 place-items-center"
      style={{ width: size, height: size }}
      role={ariaLabel ? "img" : undefined}
      aria-label={ariaLabel}
    >
      <svg width={size} height={size} viewBox="0 0 24 24" className="absolute inset-0" aria-hidden="true">
        <circle cx="12" cy="12" r={R} fill="none" stroke="var(--color-chip)" strokeWidth={stroke} />
        {frac > 0 ? (
          <circle
            cx="12"
            cy="12"
            r={R}
            fill="none"
            stroke={color}
            strokeWidth={stroke}
            strokeLinecap="round"
            strokeDasharray={`${frac * C} ${C - frac * C}`}
            transform="rotate(-90 12 12)"
          />
        ) : null}
      </svg>
      <span className="relative grid place-items-center" style={{ color }} aria-hidden="true">
        {icon}
      </span>
    </span>
  );
}
