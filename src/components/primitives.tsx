import { useCallback, useEffect, useLayoutEffect, useRef, useState, useSyncExternalStore } from "react";
import { createPortal } from "react-dom";
import type { ReactNode } from "react";

import { useI18n } from "../i18n/store";
import type { PressureLevel } from "../types/mem";

/**
 * UI v5 基础组件库（spec §3）。
 *
 * 这一层是「先建地基再铺页面」的产物：六个 board 只做组合，不写一次性样式。
 * 所有旧组件（Badge / Meter / PressureDot / Stat / CompareBar）的 **props 签名
 * 一律不改**，只允许扩展 —— 它们被 9 个文件引用，签名一动手就要跟着改 9 处。
 */

/** 语义色。cpu / port 是 v5 新增的域色档：端口不再借 warn，避免与真实告警撞色 */
export type Tone = "accent" | "warn" | "danger" | "info" | "neutral" | "cpu" | "port";

const badgeTone: Record<Tone, string> = {
  accent: "bg-accent/12 text-accent",
  warn: "bg-warn/12 text-warn",
  danger: "bg-danger/12 text-danger",
  info: "bg-info/12 text-info",
  neutral: "bg-chip text-ink-dim",
  cpu: "bg-cpu-soft text-domain-cpu",
  port: "bg-port-soft text-domain-port",
};

const barTone: Record<Tone, string> = {
  accent: "bg-accent",
  warn: "bg-warn",
  danger: "bg-danger",
  info: "bg-info",
  neutral: "bg-ink-faint",
  cpu: "bg-domain-cpu",
  port: "bg-domain-port",
};

/** 小徽标：用于口径标注、占比、状态。无边框，只有一层淡底色 */
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
      className={`inline-flex shrink-0 items-center gap-1 rounded-full px-[7px] py-[2px] text-[9.5px] leading-[13px] font-medium whitespace-nowrap ${badgeTone[tone]}`}
    >
      {children}
    </span>
  );
}

/**
 * 细进度条（swap 仪表 / 占比 / 分级构成）。
 *
 * tone 完全由调用方决定 —— 这里**不做**「超过某比例自动变红」。
 * 本项目的条有两种语义：占用率（可自动升档）和相对长度（bytes/max，
 * 最长那条 ratio 恒为 1）。后者一旦自动升档，最长的分类会被误标成危险色。
 * 语义判断只有调用方知道，组件不该猜。
 */
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
    <div className={`w-full overflow-hidden rounded-full bg-chip ${className}`} style={{ height }}>
      <div
        className={`h-full rounded-full transition-[width] duration-500 ease-out ${barTone[tone]}`}
        style={{ width: `${pct}%` }}
      />
    </div>
  );
}

/**
 * 分段进度条（存储 L1–L4 构成 / 任意多段占比）。
 *
 * 段间用 **flex gap** 而不是 border 分隔 —— 加 border 会让各段实际宽度
 * 多出 1px，四段累加后整条溢出容器（v5 原型里 L1 满条就是这么来的）。
 */
export function TrackProgress({
  segments,
  height = 6,
  gap = 2,
  className = "",
  title,
}: {
  segments: { value: number; color: string; label?: string }[];
  height?: number;
  gap?: number;
  className?: string;
  title?: string;
}) {
  const total = segments.reduce((s, x) => s + (Number.isFinite(x.value) ? Math.max(0, x.value) : 0), 0);
  return (
    <div
      title={title}
      className={`flex w-full overflow-hidden rounded-full ${className}`}
      style={{ height, gap }}
      role="img"
      aria-label={segments.map((s) => s.label).filter(Boolean).join(" / ") || undefined}
    >
      {segments.map((seg, i) => {
        const pct = total > 0 ? (Math.max(0, seg.value) / total) * 100 : 0;
        if (pct <= 0) return null;
        return (
          <div
            key={`${seg.label ?? "seg"}-${i}`}
            className="h-full shrink-0 transition-[width] duration-500 ease-out"
            style={{ width: `calc(${pct}% - ${(gap * (segments.length - 1)) / Math.max(1, segments.length)}px)`, background: seg.color }}
          />
        );
      })}
    </div>
  );
}

const pressureMeta: Record<
  PressureLevel,
  { labelKey: "common.pressure.normal" | "common.pressure.warning" | "common.pressure.critical"; dot: string; text: string }
> = {
  normal: { labelKey: "common.pressure.normal", dot: "bg-accent", text: "text-accent" },
  warning: { labelKey: "common.pressure.warning", dot: "bg-warn", text: "text-warn" },
  critical: { labelKey: "common.pressure.critical", dot: "bg-danger", text: "text-danger" },
};

export function PressureDot({ level }: { level: PressureLevel }) {
  const { t } = useI18n();
  const meta = pressureMeta[level];
  return (
    <span className="inline-flex items-center gap-1.5">
      <span className={`size-1.5 rounded-full ${meta.dot}`} />
      <span className={`text-[11px] font-medium ${meta.text}`}>{t(meta.labelKey)}</span>
    </span>
  );
}

/** 指标块：主数值 + 单位 + 说明。无卡片边框，靠排版与留白分组 */
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
          : tone === "cpu"
            ? "text-domain-cpu"
            : tone === "port"
              ? "text-domain-port"
              : "text-ink";
  return (
    <div>
      <div className="text-[10px] tracking-wide text-ink-faint">{label}</div>
      <div className="mt-1.5 flex items-baseline gap-1">
        <span className={`font-mono text-[19px] leading-none tabular-nums ${valueTone}`}>
          {value}
        </span>
        {unit ? <span className="text-[11px] text-ink-faint">{unit}</span> : null}
      </div>
      {hint ? <div className="mt-1.5 text-[10px] leading-snug text-ink-faint">{hint}</div> : null}
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
      <Meter ratio={ratio} tone={tone} height={4} className="mt-1.5" />
      {sub ? <div className="mt-1 text-[10px] text-ink-faint">{sub}</div> : null}
    </div>
  );
}

/* ==========================================================================
   卡片
   ========================================================================== */

type CardVariant = "base" | "interactive" | "hero" | "insets";

const cardVariant: Record<CardVariant, string> = {
  base: "rounded-card border-hairline-soft bg-surface-raised",
  interactive:
    "rounded-card border-hairline-soft bg-surface-raised transition-[border-color,transform] duration-150 hover:-translate-y-px hover:border-hairline",
  hero: "rounded-hero border-hairline-soft bg-surface-raised",
  insets: "rounded-[9px] border-hairline bg-surface-sunk",
};

export function Card({
  variant = "base",
  className = "",
  children,
  as = "div",
  onClick,
  title,
  "aria-label": ariaLabel,
}: {
  variant?: CardVariant;
  className?: string;
  children: ReactNode;
  as?: "div" | "section" | "article";
  onClick?: () => void;
  title?: string;
  "aria-label"?: string;
}) {
  const Tag = as;
  const pad = variant === "hero" ? "px-5 py-[18px]" : variant === "insets" ? "px-3 py-2.5" : "px-4 py-[14px]";
  const interactive = typeof onClick === "function";
  return (
    <Tag
      title={title}
      aria-label={ariaLabel}
      onClick={onClick}
      className={`border ${cardVariant[interactive && variant === "base" ? "interactive" : variant]} ${pad} ${className}`}
    >
      {children}
    </Tag>
  );
}

/* ==========================================================================
   按钮族（spec §3.5）
   ========================================================================== */

type BtnVariant = "primary" | "ghost" | "danger" | "text";
type BtnSize = "sm" | "md";

const btnVariant: Record<BtnVariant, string> = {
  primary: "bg-accent text-accent-ink hover:brightness-105",
  ghost: "border border-hairline bg-chip text-ink-dim hover:border-ink-faint hover:text-ink",
  danger: "bg-danger text-danger-ink hover:brightness-105",
  text: "text-ink-dim hover:bg-chip-hover hover:text-ink",
};

const btnSize: Record<BtnSize, string> = {
  sm: "h-[26px] px-[7px] text-[11px]",
  md: "h-7 px-3 text-[11.5px]",
};

export function Btn({
  variant = "ghost",
  size = "sm",
  icon,
  children,
  onClick,
  disabled,
  loading,
  title,
  className = "",
  type = "button",
}: {
  variant?: BtnVariant;
  size?: BtnSize;
  icon?: ReactNode;
  children?: ReactNode;
  onClick?: () => void;
  disabled?: boolean;
  loading?: boolean;
  title?: string;
  className?: string;
  type?: "button" | "submit";
}) {
  return (
    <button
      type={type}
      title={title}
      onClick={onClick}
      disabled={disabled || loading}
      className={`inline-flex shrink-0 items-center justify-center gap-1.5 rounded-ctl font-medium whitespace-nowrap transition-all duration-150 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-accent active:scale-[0.98] disabled:pointer-events-none disabled:opacity-45 ${btnSize[size]} ${btnVariant[variant]} ${className}`}
    >
      {loading ? <Spinner /> : icon}
      {children}
    </button>
  );
}

function Spinner() {
  return (
    <span
      aria-hidden
      className="inline-block size-3 shrink-0 animate-spin rounded-full border-[1.5px] border-current border-t-transparent opacity-70"
    />
  );
}

/* ==========================================================================
   语义标记
   ========================================================================== */

/**
 * 口径徽标（spec §3.2）—— Prune 的可信度全靠它。
 * 「这个数字是怎么算的」必须随手可查，尤其是两个能被相减的数。
 * label / tip 由调用方从 i18n 取（白名单见 spec §8.1）。
 */
export function ScopeBadge({
  label,
  tip,
  className = "",
}: {
  label: string;
  tip?: string;
  className?: string;
}) {
  return (
    <span
      title={tip}
      className={`inline-flex shrink-0 cursor-help items-center gap-1 rounded-full border border-info/25 bg-info/10 px-[7px] py-px text-[9.5px] leading-[13px] whitespace-nowrap text-info ${className}`}
    >
      <InfoGlyph />
      {label}
    </span>
  );
}

function InfoGlyph() {
  return (
    <svg width="9" height="9" viewBox="0 0 12 12" fill="none" aria-hidden>
      <circle cx="6" cy="6" r="4.6" stroke="currentColor" strokeWidth="1.1" />
      <path d="M6 5.3v3" stroke="currentColor" strokeWidth="1.1" strokeLinecap="round" />
      <circle cx="6" cy="3.6" r="0.65" fill="currentColor" />
    </svg>
  );
}

/**
 * 新增提案标记（spec §3.3）。
 * 只贴在「设计稿有、实现没有」的独立卡上 —— 不得贴到既有功能上模糊边界。
 * 文案走 i18n：check-i18n 脚本会拦下组件里的中文字面量（spec §8.3）。
 */
export function ProposalTag({ children }: { children?: ReactNode }) {
  const { t } = useI18n();
  return (
    <span className="inline-flex shrink-0 items-center rounded-full border border-dashed border-ink-faint px-1.5 py-px text-[9px] leading-[13px] whitespace-nowrap text-ink-faint">
      {children ?? t("common.hint.proposalNew")}
    </span>
  );
}

/* ==========================================================================
   Toast（spec §3.8）—— 模块级单例，任何组件可直接 toast()
   ========================================================================== */

type ToastTone = "info" | "ok" | "error";

interface ToastItem {
  id: number;
  message: string;
  tone: ToastTone;
}

let toastItem: ToastItem | null = null;
let toastSeq = 0;
let toastTimer: ReturnType<typeof setTimeout> | null = null;
const toastSubs = new Set<() => void>();

function emitToast() {
  toastSubs.forEach((fn) => fn());
}

/**
 * 同一时刻只保留一条：新消息直接替换旧的并重置计时。
 * 排队会让用户看到 3 秒前那次操作的反馈，反而干扰判断。
 */
export function toast(message: string, tone: ToastTone = "info"): void {
  toastItem = { id: ++toastSeq, message, tone };
  if (toastTimer) clearTimeout(toastTimer);
  toastTimer = setTimeout(() => {
    toastItem = null;
    toastTimer = null;
    emitToast();
  }, 2400);
  emitToast();
}

function subscribeToast(fn: () => void) {
  toastSubs.add(fn);
  return () => {
    toastSubs.delete(fn);
  };
}

/**
 * DEV 自检钩子：HMR 之后模块 URL 会带 `?t=` 时间戳，页面里跑的实例和
 * 调试时 `import('/src/...')` 拿到的裸路径实例可能是两份，状态不共享。
 * 这里把「页面内那个实例」的 toast 挂到 window，用 ??= 保证先注册的（页面的）赢。
 * 生产构建里 import.meta.env.DEV 是常量 false，整段会被摇掉。
 */
if (import.meta.env.DEV && typeof window !== "undefined") {
  (window as unknown as { __pruneToast?: typeof toast }).__pruneToast ??= toast;
}

const toastToneBar: Record<ToastTone, string> = {
  info: "bg-info",
  ok: "bg-accent",
  error: "bg-danger",
};

/** 挂在三个窗口的根节点上；各自独立，互不影响 */
export function ToastHost() {
  const item = useSyncExternalStore(
    subscribeToast,
    () => toastItem,
    () => null,
  );
  if (!item) return null;
  return createPortal(
    <div
      role="status"
      aria-live="polite"
      key={item.id}
      className="pointer-events-none fixed bottom-[18px] left-1/2 z-[100] flex -translate-x-1/2 items-center gap-2 overflow-hidden rounded-[9px] border border-hairline bg-surface-overlay py-2 pr-3.5 pl-0 text-[11px] text-ink shadow-[0_10px_30px_rgba(0,0,0,0.5)]"
    >
      <span className={`h-4 w-[3px] shrink-0 rounded-r ${toastToneBar[item.tone]}`} />
      <span className="pr-1">{item.message}</span>
    </div>,
    document.body,
  );
}

/* ==========================================================================
   PopMenu（spec §3.10 / §7.4）—— 右键菜单
   ========================================================================== */

export interface PopMenuItem {
  label: string;
  icon?: ReactNode;
  kbd?: string;
  danger?: boolean;
  disabled?: boolean;
  onSelect: () => void;
}

/**
 * 独立命名空间：类名/样式都不复用「结束进程」那套 .ctx。
 * 曾经复用导致继承了 flex-direction:column，菜单高度直接翻倍。
 */
export function PopMenu({
  x,
  y,
  items,
  footer,
  onClose,
  width = 184,
}: {
  x: number;
  y: number;
  items: PopMenuItem[];
  footer?: ReactNode;
  onClose: () => void;
  width?: number;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ x, y });
  const [active, setActive] = useState(0);

  // 边缘翻转：先量再定位，避免菜单被窗口边缘裁掉
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const r = el.getBoundingClientRect();
    const pad = 8;
    let nx = x;
    let ny = y;
    if (x + r.width > window.innerWidth - pad) nx = Math.max(pad, x - r.width);
    if (y + r.height > window.innerHeight - pad) ny = Math.max(pad, y - r.height);
    setPos({ x: nx, y: ny });
  }, [x, y]);

  const run = useCallback(
    (i: number) => {
      const it = items[i];
      if (!it || it.disabled) return;
      onClose();
      it.onSelect();
    },
    [items, onClose],
  );

  useEffect(() => {
    const el = ref.current;
    el?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        onClose();
      } else if (e.key === "ArrowDown") {
        e.preventDefault();
        setActive((i) => (i + 1) % items.length);
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        setActive((i) => (i - 1 + items.length) % items.length);
      } else if (e.key === "Enter") {
        e.preventDefault();
        run(active);
      }
    };
    const onDown = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) onClose();
    };
    window.addEventListener("keydown", onKey);
    // 捕获阶段：让菜单先于行点击关闭，避免「点了菜单外面还触发了行下钻」
    window.addEventListener("mousedown", onDown, true);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("mousedown", onDown, true);
    };
  }, [active, items.length, onClose, run]);

  return createPortal(
    <div
      ref={ref}
      role="menu"
      tabIndex={-1}
      className="pmenu fixed z-[92] rounded-pop border border-hairline bg-surface-overlay py-[3px] shadow-[0_16px_40px_rgba(0,0,0,0.55)] focus:outline-none"
      style={{ left: pos.x, top: pos.y, width }}
      onContextMenu={(e) => e.preventDefault()}
    >
      {items.map((it, i) => (
        <button
          key={it.label}
          type="button"
          role="menuitem"
          disabled={it.disabled}
          onMouseEnter={() => setActive(i)}
          onClick={() => run(i)}
          className={`flex w-full items-center gap-2 px-2.5 py-[6px] text-left text-[11.5px] transition-colors disabled:opacity-40 ${
            it.danger ? "text-danger" : "text-ink"
          } ${i === active && !it.disabled ? "bg-chip-hover" : "bg-transparent"}`}
        >
          {it.icon ? <span className="grid size-[13px] shrink-0 place-items-center opacity-80">{it.icon}</span> : null}
          <span className="min-w-0 flex-1 truncate">{it.label}</span>
          {it.kbd ? <kbd className="shrink-0 text-[9.5px] text-ink-faint">{it.kbd}</kbd> : null}
        </button>
      ))}
      {footer ? <div className="mt-1 border-t border-hairline-soft px-2.5 py-1.5">{footer}</div> : null}
    </div>,
    document.body,
  );
}
