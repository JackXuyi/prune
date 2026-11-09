/**
 * 图标导航栏（UI v4 §4.1「Icon Rail」）：顶部文字 tab → 左侧 56px 图标栏。
 *
 * - 图标先于文字被识别；文字全部降级为 hover tooltip（title）
 * - 选中态：左侧 2.5px 域色发光竖条 + 域色图标配色（颜色即语义 §3.2）
 * - ⌘1–6 快捷键仍在 MainWindow（rail 只负责呈现），底部固定设置齿轮
 * - 刷新 / 口径 Badge / 返回按钮留在内容列标题栏——它们是「当前页」的属性，
 *   不是「全局导航」的属性
 */
import type { ReactNode } from "react";

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
  label: string;
  hint: string;
  icon: (props: { size?: number; className?: string }) => ReactNode;
  /** 域色（CSS 颜色，选中态竖条与图标 hover 用） */
  color: string;
}

const ITEMS: RailItem[] = [
  { id: "stats", label: "总览", hint: "可回收 + 内存 / CPU / 硬盘 / 端口 / 网速：本机资源总览", icon: OverviewIcon, color: "var(--color-accent)" },
  { id: "resources", label: "应用", hint: "每个应用一张卡：进程 / CPU / 内存 / 磁盘 / 端口 / 网速", icon: AppsIcon, color: "var(--color-domain-cpu)" },
  { id: "memory", label: "内存", hint: "内存概况、swap 与换出受害者 · 进程家族与明细", icon: MemoryCloudIcon, color: "var(--color-info)" },
  { id: "disk", label: "存储", hint: "卷容量、可回收空间与清理", icon: DatabaseIcon, color: "var(--color-accent)" },
  { id: "net", label: "网络", hint: "本机下载 / 上传速率、走势、网卡分布与今日流量", icon: GlobeIcon, color: "var(--color-domain-net)" },
  { id: "ports", label: "端口", hint: "谁占着 3000：按端口反查持有进程", icon: PortJackIcon, color: "var(--color-warn)" },
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
  return (
    <nav
      className="flex w-14 shrink-0 flex-col items-center border-r border-hairline-soft bg-black/[0.18] py-3"
      aria-label="主导航"
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
            title={`${it.label} · ${it.hint}`}
            onClick={() => onChange(it.id)}
            className={`prune-anim relative grid size-11 place-items-center rounded-[10px] transition-colors ${
              active ? "bg-white/[0.06] text-ink" : "text-ink-faint hover:bg-white/[0.04] hover:text-ink-dim"
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
        title="设置 · 灵动岛开关与阈值"
        aria-label="设置"
        className="grid size-11 place-items-center rounded-[10px] text-ink-faint transition-colors hover:bg-white/[0.04] hover:text-ink-dim"
      >
        <GearIcon size={17} />
      </button>
    </nav>
  );
}
