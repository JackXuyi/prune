/**
 * StackBar：构成关系堆叠条（UX v3 §7.2 / §7.6）。
 *
 * 用于「合计有意义」的构成：可回收 L1–L4、footprint = RSS + 换出、磁盘三层。
 * 规范（§7.4）：段间不留缝（容器裁剪即分隔）；某段 < 1.2% 宽时**不画色块但
 * 保留数字**（数字由调用方的图例承担，不为可见性虚增比例）；muted 段用斜纹
 * 表示「不计入主数字」（如 L4 Docker）。
 * 总量为 0 / 不可读时整体不渲染——不画一条空轨道冒充数据。
 */
import type { Tone } from "../../lib/statsReadout";
import { TONE_FILL } from "./tone";

export interface StackSegment {
  key: string;
  /** 图例与 aria 文本用 */
  label: string;
  value: number;
  tone?: Tone;
  /** true = 斜纹段，语义「不计入合计」（如 L4 只给命令） */
  muted?: boolean;
}

export function StackBar({
  segments,
  total,
  height = 6,
  ariaLabel,
  className = "",
}: {
  segments: StackSegment[];
  /** 缺省 = 各段之和 */
  total?: number;
  height?: number;
  ariaLabel?: string;
  className?: string;
}) {
  const sum = total ?? segments.reduce((s, x) => s + (x.value > 0 ? x.value : 0), 0);
  if (!(sum > 0) || !Number.isFinite(sum)) return null;
  const legend = segments
    .filter((s) => s.value > 0)
    .map((s) => `${s.label} ${Math.round((s.value / sum) * 100)}%${s.muted ? "（不计入）" : ""}`)
    .join("，");
  return (
    <div
      role="img"
      aria-label={ariaLabel ?? legend}
      className={`flex w-full overflow-hidden rounded-full bg-white/[0.05] ${className}`}
      style={{ height }}
    >
      {segments.map((seg) => {
        if (!(seg.value > 0)) return null;
        const pct = (seg.value / sum) * 100;
        // < 1.2% 的段不画色块：宁可缺一段色，也不虚增比例
        if (pct < 1.2) return null;
        return (
          <div
            key={seg.key}
            title={seg.muted ? `${seg.label}（不计入）` : seg.label}
            className={seg.muted ? "" : TONE_FILL[seg.tone ?? "neutral"]}
            style={{
              width: `${pct}%`,
              ...(seg.muted
                ? {
                    background:
                      "repeating-linear-gradient(45deg, rgba(255,255,255,0.22) 0 2px, transparent 2px 4px)",
                  }
                : {}),
            }}
          />
        );
      })}
    </div>
  );
}
