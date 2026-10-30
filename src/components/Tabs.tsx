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
    <div role="tablist" className={`inline-flex items-center gap-0.5 ${className}`}>
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
            className={`inline-flex items-center gap-1 rounded-md font-medium transition-colors ${pad} ${
              active ? "bg-white/[0.08] text-ink" : "text-ink-faint hover:text-ink-dim"
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
