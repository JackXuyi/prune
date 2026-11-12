/**
 * BarList：多实体同一指标的排序条列表（UX v3 §7.2 / §7.6）。
 *
 * 用于实体数 ≥ 4、文字表已能排序但看不出量级差的场景：网卡分布、按应用流量。
 * 规范（§7.4）：条宽上限 40%（保留文字呼吸空间）；数值右对齐等宽字体；
 * value 为 null（读不到）时不画填充，display 由调用方给「— / 不可读」。
 */
import type { Tone } from "../../lib/statsReadout";
import { TONE_FILL } from "./tone";

export interface BarRow {
  key: string;
  label: string;
  /** null = 读不到：不画填充 */
  value: number | null;
  /** 已格式化的数值文本（条右侧） */
  display: string;
  tone?: Tone;
  onClick?: () => void;
  title?: string;
}

const BAR_CAP = 40; // 条最长占行宽 40%

export function BarList({
  rows,
  max,
  className = "",
}: {
  rows: BarRow[];
  /** 归一化基准；缺省 = 行内最大值 */
  max?: number;
  className?: string;
}) {
  const maxV = max ?? rows.reduce((m, r) => Math.max(m, r.value ?? 0), 0);
  return (
    <div className={className}>
      {rows.map((r) => {
        const pct =
          r.value != null && maxV > 0 && Number.isFinite(r.value)
            ? Math.min(BAR_CAP, (r.value / maxV) * BAR_CAP)
            : null;
        const inner = (
          <>
            <span className="w-24 shrink-0 truncate text-[11px] text-ink-dim">{r.label}</span>
            <span className="relative h-1.5 min-w-0 flex-1 rounded-full bg-chip">
              {pct != null && pct > 0 ? (
                <span
                  aria-hidden
                  className={`absolute inset-y-0 left-0 rounded-full ${TONE_FILL[r.tone ?? "accent"]}`}
                  style={{ width: `${pct}%` }}
                />
              ) : null}
            </span>
            <span className="w-20 shrink-0 text-right font-mono text-[11px] whitespace-nowrap tabular-nums text-ink">
              {r.display}
            </span>
          </>
        );
        return r.onClick ? (
          <button
            key={r.key}
            type="button"
            onClick={r.onClick}
            title={r.title}
            className="flex w-full items-center gap-2 rounded-md px-1 py-1 text-left transition-colors hover:bg-white/[0.04]"
          >
            {inner}
          </button>
        ) : (
          <div key={r.key} title={r.title} className="flex items-center gap-2 px-1 py-1">
            {inner}
          </div>
        );
      })}
    </div>
  );
}
