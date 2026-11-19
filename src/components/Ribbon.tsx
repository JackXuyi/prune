import type { ReactNode } from "react";

/**
 * 状态带基元（eul 形态）：一段圆角长条内多段 hairline 分隔，
 * 每段 = 图标位 + mono 读数，零文字标题——首页 StatusRibbon 与应用页
 * AppSummaryRibbon 共用同一套皮，避免两页各画一条「看起来差不多」的带子。
 *
 * 「环=占比」纪律由调用方决定图标位形态（MiniGauge 或 RibbonChip）；
 * 这里只管皮，不管语义。
 */

/** 长条外壳：stretch 等分 + 段间发丝线；圆角裁切由 overflow-hidden 兜底 */
export function RibbonShell({
  ariaLabel,
  children,
}: {
  ariaLabel: string;
  children: ReactNode;
}) {
  return (
    <div
      className="flex h-[62px] shrink-0 items-stretch divide-x divide-hairline-soft overflow-hidden rounded-[14px] border border-hairline-soft bg-gradient-to-b from-chip-soft to-transparent bg-surface-raised"
      role="group"
      aria-label={ariaLabel}
    >
      {children}
    </div>
  );
}

const SEG =
  "group/seg flex min-w-0 flex-1 items-center justify-center gap-2.5 rounded-none px-2 transition-colors first:rounded-l-[14px] last:rounded-r-[14px]";

/** 一段：可点（导航/切排序）就渲染 button，纯读数就渲染 div */
export function RibbonSeg({
  title,
  onClick,
  children,
}: {
  title: string;
  onClick?: () => void;
  children: ReactNode;
}) {
  if (!onClick) {
    return (
      <div className={SEG} title={title}>
        {children}
      </div>
    );
  }
  return (
    <button type="button" className={`${SEG} cursor-pointer hover:bg-chip`} title={title} onClick={onClick}>
      {children}
    </button>
  );
}

/** 段内图标芯片（非百分比位）：26px 底衬 + 域色图标 */
export function RibbonChip({
  color,
  icon,
  glow = false,
}: {
  color: string;
  icon: ReactNode;
  /** 域色微光晕（应用页排序透镜段用，轻一点） */
  glow?: boolean;
}) {
  return (
    <span
      className="grid size-[26px] shrink-0 place-items-center rounded-[8px]"
      style={{
        background: `color-mix(in srgb, ${color} 14%, transparent)`,
        color,
        boxShadow: glow ? `0 0 10px color-mix(in srgb, ${color} 18%, transparent)` : undefined,
      }}
      aria-hidden
    >
      {icon}
    </span>
  );
}

/** 段内读数：mono 17px + 弱化单位；warn/danger 只改颜色，tooltip 里始终有明文 */
export function RibbonValue({
  value,
  unit,
  tone,
}: {
  value: string;
  unit?: string;
  tone?: "warn" | "danger";
}) {
  return (
    <span className="min-w-0 whitespace-nowrap">
      <span
        className={`font-mono text-[17px] leading-none font-semibold tabular-nums ${
          tone === "warn" ? "text-warn" : tone === "danger" ? "text-danger" : "text-ink"
        }`}
      >
        {value}
      </span>
      {unit ? <span className="ml-0.5 text-[10.5px] font-medium text-ink-faint">{unit}</span> : null}
    </span>
  );
}
