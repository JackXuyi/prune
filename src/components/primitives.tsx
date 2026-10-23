import type { ReactNode } from "react";

import type { PressureLevel } from "../types/mem";

export type Tone = "accent" | "warn" | "danger" | "info" | "neutral";

const badgeTone: Record<Tone, string> = {
  accent: "border-accent/30 bg-accent/10 text-accent",
  warn: "border-warn/30 bg-warn/10 text-warn",
  danger: "border-danger/30 bg-danger/10 text-danger",
  info: "border-info/30 bg-info/10 text-info",
  neutral: "border-hairline bg-white/[0.03] text-ink-dim",
};

const barTone: Record<Tone, string> = {
  accent: "bg-accent",
  warn: "bg-warn",
  danger: "bg-danger",
  info: "bg-info",
  neutral: "bg-ink-faint",
};

/** 小徽标：用于口径标注、占比、状态 */
export function Badge({
  tone = "neutral",
  title,
  children,
}: {
  tone?: Tone;
  title?: string;
  children: ReactNode;
}) {
  return (
    <span
      title={title}
      className={`inline-flex shrink-0 items-center gap-1 rounded-[5px] border px-1.5 py-[1px] text-[10px] leading-4 font-medium ${badgeTone[tone]}`}
    >
      {children}
    </span>
  );
}

/** 细进度条（swap 仪表 / 占比） */
export function Meter({
  ratio,
  tone = "accent",
  height = 6,
  className = "",
}: {
  ratio: number;
  tone?: Tone;
  height?: number;
  className?: string;
}) {
  const pct = Math.max(0, Math.min(1, Number.isFinite(ratio) ? ratio : 0)) * 100;
  return (
    <div
      className={`w-full overflow-hidden rounded-full bg-white/[0.06] ${className}`}
      style={{ height }}
    >
      <div
        className={`h-full rounded-full transition-[width] duration-500 ease-out ${barTone[tone]}`}
        style={{ width: `${pct}%` }}
      />
    </div>
  );
}

const pressureMeta: Record<
  PressureLevel,
  { label: string; dot: string; text: string }
> = {
  normal: { label: "正常", dot: "bg-accent", text: "text-accent" },
  warning: { label: "警告", dot: "bg-warn", text: "text-warn" },
  critical: { label: "紧张", dot: "bg-danger", text: "text-danger" },
};

export function PressureDot({ level }: { level: PressureLevel }) {
  const meta = pressureMeta[level];
  return (
    <span className="inline-flex items-center gap-1.5">
      <span className={`size-1.5 rounded-full ${meta.dot}`} />
      <span className={`text-xs font-medium ${meta.text}`}>{meta.label}</span>
    </span>
  );
}

/** 指标块：主数值 + 单位 + 说明 */
export function Stat({
  label,
  value,
  unit,
  hint,
  tone = "neutral",
}: {
  label: string;
  value: string;
  unit?: string;
  hint?: string;
  tone?: Tone;
}) {
  const valueTone =
    tone === "warn"
      ? "text-warn"
      : tone === "danger"
        ? "text-danger"
        : tone === "accent"
          ? "text-accent"
          : "text-ink";
  return (
    <div className="rounded-lg border border-hairline-soft bg-surface-raised/60 px-3 py-2.5">
      <div className="text-[11px] text-ink-faint">{label}</div>
      <div className="mt-1 flex items-baseline gap-1">
        <span className={`font-mono text-lg leading-none tabular-nums ${valueTone}`}>
          {value}
        </span>
        {unit ? <span className="text-[11px] text-ink-faint">{unit}</span> : null}
      </div>
      {hint ? <div className="mt-1 text-[10px] text-ink-faint">{hint}</div> : null}
    </div>
  );
}

/** 对照条：footprint vs RSS 这类同量纲比较 */
export function CompareBar({
  label,
  value,
  ratio,
  tone = "neutral",
  sub,
}: {
  label: string;
  value: string;
  ratio: number;
  tone?: Tone;
  sub?: string;
}) {
  return (
    <div>
      <div className="flex items-baseline justify-between gap-2">
        <span className="text-[11px] text-ink-dim">{label}</span>
        <span className="font-mono text-xs tabular-nums text-ink">{value}</span>
      </div>
      <Meter ratio={ratio} tone={tone} height={5} className="mt-1.5" />
      {sub ? <div className="mt-1 text-[10px] text-ink-faint">{sub}</div> : null}
    </div>
  );
}
