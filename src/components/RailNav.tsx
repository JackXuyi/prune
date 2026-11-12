/**
 * 图标导航栏（UI v4 §4.1「Icon Rail」）：顶部文字 tab → 左侧 56px 图标栏。
 *
 * - 图标先于文字被识别；文字全部降级为 hover tooltip（title）
 * - 选中态：左侧 2.5px 域色发光竖条 + 域色图标配色（颜色即语义 §3.2）
 * - ⌘1–6 快捷键仍在 MainWindow（rail 只负责呈现），底部固定设置齿轮
 * - 刷新 / 口径 Badge / 返回按钮留在内容列标题栏——它们是「当前页」的属性，
 *   不是「全局导航」的属性
 *
 * i18n：`ITEMS` 是模块级常量，所以存的是 **key** 而不是文案 —— 写死文案的话
 * 切语言不会跟着变（常量不会重新求值）。
 */
import type { ReactNode } from "react";

import { useI18n } from "../i18n/store";
import type { NoParamKey } from "../i18n/types";
import {
  AppsIcon,
  DatabaseIcon,
  GearIcon,
  GlobeIcon,
  MemoryCloudIcon,
  OverviewIcon,
  PortJackIcon,
  ScissorsIcon,
} from "./icons";

export type RailTab = "stats" | "resources" | "memory" | "disk" | "net" | "ports";

interface RailItem {
  id: RailTab;
  labelKey: NoParamKey;
  hintKey: NoParamKey;
  icon: (props: { size?: number; className?: string }) => ReactNode;
  /** 域色（CSS 颜色，选中态竖条与图标 hover 用） */
  color: string;
}

const ITEMS: RailItem[] = [
  { id: "stats", labelKey: "nav.stats", hintKey: "nav.statsHint", icon: OverviewIcon, color: "var(--color-accent)" },
  { id: "resources", labelKey: "nav.resources", hintKey: "nav.resourcesHint", icon: AppsIcon, color: "var(--color-domain-cpu)" },
  { id: "memory", labelKey: "nav.memory", hintKey: "nav.memoryHint", icon: MemoryCloudIcon, color: "var(--color-info)" },
  { id: "disk", labelKey: "nav.disk", hintKey: "nav.diskHint", icon: DatabaseIcon, color: "var(--color-accent)" },
  { id: "net", labelKey: "nav.net", hintKey: "nav.netHint", icon: GlobeIcon, color: "var(--color-domain-net)" },
  { id: "ports", labelKey: "nav.ports", hintKey: "nav.portsHint", icon: PortJackIcon, color: "var(--color-domain-port)" },
];

export function RailNav({
  value,
  onChange,
  onOpenSettings,
}: {
  value: RailTab;
  onChange: (t: RailTab) => void;
  onOpenSettings: () => void;
}) {
  const { t } = useI18n();
  return (
    <nav
      className="flex w-[58px] shrink-0 flex-col items-center border-r border-hairline-soft bg-black/[0.18] py-3"
      aria-label={t("nav.mainAria")}
    >
      {/* Logo：无交互（点击无动作，沿用 UX v3 §5.1） */}
      <span className="mb-3 grid size-10 place-items-center" title="Prune">
        <ScissorsIcon size={17} className="text-accent" />
      </span>

      {ITEMS.map((it) => {
        const active = it.id === value;
        const Icon = it.icon;
        return (
          <button
            key={it.id}
            type="button"
            role="tab"
            aria-selected={active}
            title={`${t(it.labelKey)} · ${t(it.hintKey)}`}
            onClick={() => onChange(it.id)}
            className={`prune-anim relative grid size-10 place-items-center rounded-[10px] transition-colors ${
              active ? "bg-chip-hover text-ink" : "text-ink-faint hover:bg-chip hover:text-ink-dim"
            }`}
            style={active ? { color: it.color } : undefined}
          >
            {/* 选中态发光竖条：域色即语义 */}
            {active ? (
              <span
                className="absolute -left-[7px] top-[9px] bottom-[9px] w-[2.5px] rounded-full"
                style={{ background: it.color, boxShadow: `0 0 10px ${it.color}` }}
              />
            ) : null}
            <Icon size={19} />
          </button>
        );
      })}

      <span className="flex-1" />

      <button
        type="button"
        onClick={onOpenSettings}
        title={t("nav.settingsHint")}
        aria-label={t("nav.settings")}
        className="grid size-10 place-items-center rounded-[10px] text-ink-faint transition-colors hover:bg-chip hover:text-ink-dim"
      >
        <GearIcon size={17} />
      </button>
    </nav>
  );
}
