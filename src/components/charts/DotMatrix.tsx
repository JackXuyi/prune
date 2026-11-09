/**
 * 点阵图（UI v4 §5.6）：端口仪表盘的「一眼可见」形态。
 *
 * 每个监听端口一颗点，颜色按 exposure 分组（琥珀=对外 / 绿=关注层 / 灰=其他），
 * hover 出 tooltip。max 截断防止端口数爆炸时把卡片撑爆——超出部分用「+N」收尾。
 */
import type { Tone } from "../../lib/statsReadout";

export interface DotItem {
  tone: Tone;
  /** hover tooltip（端口 + 进程）；省略则不可交互 */
  title?: string;
}

const TONE_COLOR: Record<Tone, string> = {
  accent: "var(--color-accent)",
  warn: "var(--color-warn)",
  danger: "var(--color-danger)",
  info: "var(--color-info)",
  neutral: "#2c333b",
};

export function DotMatrix({
  items,
  dot = 9,
  gap = 5,
  max = 40,
  className = "",
  ariaLabel,
}: {
  items: DotItem[];
  dot?: number;
  gap?: number;
  max?: number;
  className?: string;
  ariaLabel?: string;
}) {
  const shown = items.slice(0, max);
  const rest = items.length - shown.length;

  return (
    <div
      className={`flex flex-wrap ${className}`}
      style={{ gap, maxWidth: (dot + gap) * 18 }}
      role="img"
      aria-label={ariaLabel}
    >
      {shown.map((d, i) => (
        <span
          key={i}
          title={d.title}
          className="inline-block shrink-0 rounded-full"
          style={{
            width: dot,
            height: dot,
            background: TONE_COLOR[d.tone],
            boxShadow:
              d.tone === "warn" || d.tone === "danger"
                ? `0 0 6px color-mix(in srgb, ${TONE_COLOR[d.tone]} 40%, transparent)`
                : undefined,
          }}
        />
      ))}
      {rest > 0 ? (
        <span
          className="inline-grid shrink-0 place-items-center rounded-full font-mono text-[8px] text-ink-faint"
          style={{ width: dot * 2 + gap, height: dot, fontSize: 8 }}
        >
          +{rest}
        </span>
      ) : null}
    </div>
  );
}
