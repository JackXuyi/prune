import type { ReactNode } from "react";

export interface TabItem<T extends string> {
  id: T;
  label: string;
  /** 悬浮说明，避免在窄面板里堆副标题 */
  hint?: string;
  badge?: ReactNode;
}

/** 分段式 tab：面板与主窗口共用，保持同一套视觉与交互 */
export function Tabs<T extends string>({
  value,
  onChange,
  items,
  className = "",
  size = "md",
}: {
  value: T;
  onChange: (v: T) => void;
  items: TabItem<T>[];
  className?: string;
  size?: "sm" | "md";
}) {
  const pad = size === "sm" ? "px-2 py-[3px] text-[11px]" : "px-2.5 py-1 text-[11.5px]";
  return (
    <div
      role="tablist"
      className={`inline-flex items-center gap-0.5 rounded-md bg-white/[0.03] p-0.5 ring-1 ring-white/[0.04] ${className}`}
    >
      {items.map((it) => {
        const active = it.id === value;
        return (
          <button
            key={it.id}
            type="button"
            role="tab"
            aria-selected={active}
            title={it.hint}
            onClick={() => onChange(it.id)}
            className={`inline-flex items-center gap-1 rounded-[5px] font-medium transition-colors ${pad} ${
              active
                ? "bg-surface-overlay text-ink shadow-[0_1px_2px_rgba(0,0,0,0.4)]"
                : "text-ink-faint hover:text-ink-dim"
            }`}
          >
            {it.label}
            {it.badge}
          </button>
        );
      })}
    </div>
  );
}
