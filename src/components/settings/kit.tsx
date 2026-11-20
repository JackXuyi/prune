/**
 * 设置弹层共享基元（settings-eul-mockup.html W2 落码）。
 *
 * 设计稿契约：
 * - Group：图形锚点（chip 底描边图标）+ 组标题 + 右注；组体是描边卡。
 * - ToggleRow：11.5px 主文字 + 右贴开关；说明文字降级为 title tooltip
 *   （文字三层红线：标题 / 读数 / 一行口径，说明不占行高）。
 * - Slider：label 88px + 视觉轨道 + mono 值单行 26px；真实 <input type=range>
 *   透明铺在轨道上（保留原生键盘 / 无障碍交互），hint 同样进 tooltip。
 * - StatusDot：点徽（ plist 已注册 / CLI v0.16 / 就绪·刘海），文字恒 mono。
 *
 * 诚实纪律：`—` ≠ 0；读不到的状态点不渲染（隐藏 = 未就绪语义，不是占位）。
 */
import type { ReactNode } from "react";

/** 组容器：图标锚点 + 标题 + 右注 + 行容器 */
export function SettingsGroup({
  icon,
  title,
  note,
  titleTooltip,
  children,
}: {
  icon: ReactNode;
  title: string;
  /** 右侧口径短注（一行，截断） */
  note?: string;
  /** 整组口径说明（进 tooltip，不占版面） */
  titleTooltip?: string;
  children: ReactNode;
}) {
  return (
    /* 不加 overflow-hidden：语言下拉菜单要从组内弹出（会溢出组边界），
       圆角裁切交给内容自身的圆角语义 */
    <section className="rounded-xl border border-hairline bg-surface-raised">
      <div
        className="flex items-center gap-2 px-3 pb-1.5 pt-2"
        title={titleTooltip}
      >
        <span className="grid h-[22px] w-[22px] shrink-0 place-items-center rounded-md bg-chip text-ink-dim">
          {icon}
        </span>
        <span className="text-[11px] font-semibold text-ink-dim">{title}</span>
        {note ? (
          <span className="ml-auto truncate text-[9px] text-ink-faint">{note}</span>
        ) : null}
      </div>
      <div className="px-3 pb-2">{children}</div>
    </section>
  );
}

/** 开关（38×22，与旧实现同规格；aria-label 必传） */
export function Switch({
  checked,
  onChange,
  label,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
  label: string;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      onClick={() => onChange(!checked)}
      className={`relative h-[22px] w-[38px] shrink-0 rounded-full transition-colors ${
        checked ? "bg-accent/80" : "bg-chip-hover"
      }`}
    >
      <span
        className="absolute top-[3px] h-4 w-4 rounded-full bg-white shadow-[0_1px_3px_rgba(0,0,0,0.3)] transition-all"
        style={{ left: checked ? 20 : 3 }}
      />
    </button>
  );
}

/** 开关行：主文字 + 可选 tag + 右贴开关；说明进 tooltip；dim = 随主开关变灰 */
export function ToggleRow({
  label,
  tag,
  desc,
  checked,
  onChange,
  ariaLabel,
  dim,
}: {
  label: string;
  tag?: string;
  desc?: string;
  checked: boolean;
  onChange: (v: boolean) => void;
  ariaLabel: string;
  dim?: boolean;
}) {
  return (
    <div
      className={`flex items-center justify-between gap-2.5 py-1 transition-opacity ${
        dim ? "opacity-40" : ""
      }`}
      title={desc}
    >
      <span className="min-w-0 truncate text-[11.5px] text-ink">
        {label}
        {tag ? (
          <span className="ml-1.5 shrink-0 text-[9px] text-ink-faint">{tag}</span>
        ) : null}
      </span>
      <Switch checked={checked} onChange={onChange} label={ariaLabel} />
    </div>
  );
}

/** 点徽：实色状态点 + mono 文字。读不到就整颗不渲染（调用方判断） */
export function StatusDot({
  tone = "accent",
  title,
  children,
}: {
  tone?: "accent" | "warn" | "faint";
  title?: string;
  children: ReactNode;
}) {
  const dot =
    tone === "accent" ? "bg-accent" : tone === "warn" ? "bg-warn" : "bg-ink-faint";
  return (
    <span
      className="inline-flex max-w-[180px] items-center gap-1.5"
      title={title}
    >
      <span className={`h-1.5 w-1.5 shrink-0 rounded-full ${dot}`} />
      <span className="truncate font-mono text-[10px] tabular-nums text-ink-dim">
        {children}
      </span>
    </span>
  );
}

/**
 * 紧凑滑杆行：label 88px + 视觉轨道 + mono 值。
 * 真实 input[type=range] 透明铺满轨道区——拖动 / 键盘 / 读屏全走原生控件，
 * 视觉层只表达当前值位置。hint 进行级 tooltip（设计稿：hint 不占行高）。
 */
export function Slider({
  label,
  hint,
  value,
  min,
  max,
  step,
  suffix,
  disabled,
  onChange,
}: {
  label: string;
  hint: string;
  value: number;
  min: number;
  max: number;
  step: number;
  suffix: string;
  disabled?: boolean;
  onChange: (v: number) => void;
}) {
  const pct = max > min ? ((value - min) / (max - min)) * 100 : 0;
  return (
    <div
      className={`flex items-center gap-2.5 py-1 transition-opacity ${
        disabled ? "opacity-40" : ""
      }`}
      title={hint}
    >
      <span className="w-[88px] shrink-0 truncate text-[10.5px] text-ink-dim">
        {label}
      </span>
      <div className="relative h-4 flex-1">
        <div className="absolute top-1/2 h-1 w-full -translate-y-1/2 rounded-full bg-chip">
          <div
            className="absolute inset-y-0 left-0 rounded-full bg-accent/70"
            style={{ width: `${pct}%` }}
          />
        </div>
        <div
          className="absolute top-1/2 h-2.5 w-2.5 -translate-x-1/2 -translate-y-1/2 rounded-full bg-accent shadow-[0_1px_3px_rgba(0,0,0,0.3)]"
          style={{ left: `${pct}%` }}
        />
        <input
          type="range"
          min={min}
          max={max}
          step={step}
          value={value}
          disabled={disabled}
          onChange={(e) => onChange(Number(e.target.value))}
          aria-label={label}
          className="absolute inset-0 w-full cursor-pointer opacity-0 disabled:cursor-not-allowed"
        />
      </div>
      <span className="w-[52px] shrink-0 text-right font-mono text-[12px] tabular-nums text-ink">
        {value}
        <span className="ml-0.5 text-[9px] font-medium text-ink-faint">{suffix}</span>
      </span>
    </div>
  );
}

/** 口径句（CPU / 内存规则）：chip 底一行，文字即口径，不伪装图表 */
export function RuleLine({ children }: { children: ReactNode }) {
  return (
    <p className="my-1 rounded-lg bg-chip-soft px-2.5 py-1.5 text-[9.5px] leading-4 text-ink-faint">
      {children}
    </p>
  );
}
