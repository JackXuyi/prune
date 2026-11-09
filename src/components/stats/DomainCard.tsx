/**
 * 域卡（UI v4 §5.1）：总览六张卡的统一形态——「图标 + 大数字 + 微图表」。
 *
 * 与 StatsCard 的分工：StatsCard 面向「读数排版」（caption 平铺、副读数成行），
 * DomainCard 面向「驾驶舱密度」——caption 全部降级为 hover tooltip（少文字原则），
 * 域色图标 chip 先于文字被识别，微图表占卡片下半。
 * 读数仍来自同一份 `Readout`（statsReadout 同源层，两块屏不各说各话）。
 */
import type { ReactNode } from "react";

import type { Readout } from "../../lib/statsReadout";
import { ChevronRightIcon } from "../icons";
import { Reading } from "./StatsCard";

export function DomainCard({
  title,
  caption,
  icon,
  color,
  readout,
  onOpen,
  chart,
  body,
}: {
  title: string;
  /** hover tooltip（少文字原则：caption 不再平铺） */
  caption: string;
  icon: ReactNode;
  /** 域色（CSS 颜色，推荐 var(--color-*)） */
  color: string;
  readout: Readout;
  /** 存在时整卡可点（只导航，不做任何清理动作——红线不变） */
  onOpen?: () => void;
  /** 卡片下半的微图表位（面积走势 / 阈值条 / 点阵） */
  chart?: ReactNode;
  /** 自定义主体（网络方向卡用） */
  body?: ReactNode;
}) {
  const shell =
    "group flex min-h-[140px] flex-col rounded-[14px] border border-white/[0.07] bg-white/[0.035] px-4 py-3.5 text-left transition-all hover:-translate-y-px hover:border-white/[0.12] hover:bg-white/[0.05] hover:shadow-[0_0_24px_rgba(255,255,255,0.06)]";

  const content = body ?? (
    <>
      <div className="flex items-baseline gap-1.5">
        <Reading readout={readout} />
        {readout.ratio != null ? (
          <span className="ml-auto font-mono text-[10px] tabular-nums text-ink-faint">
            {Math.round(readout.ratio * 100)}%
          </span>
        ) : null}
      </div>
      {chart ? <div className="mt-auto pt-2.5">{chart}</div> : null}
    </>
  );

  const inner = (
    <>
      {/* w-full：WKWebView 里 button 作 flex 容器时交叉轴 stretch 不生效，
          子元素会收缩成内容宽（右对齐/撑满全失效），必须显式给宽度 */}
      <div className="flex w-full items-center gap-2">
        <span
          className="grid size-[26px] shrink-0 place-items-center rounded-lg"
          style={{
            background: `color-mix(in srgb, ${color} 13%, transparent)`,
            color,
          }}
          aria-hidden
        >
          {icon}
        </span>
        <span className="text-[11px] font-medium tracking-wide text-ink-dim">{title}</span>
        {onOpen ? (
          <ChevronRightIcon
            size={12}
            className="ml-auto shrink-0 text-ink-faint transition-transform group-hover:translate-x-0.5"
          />
        ) : null}
      </div>
      <div className="mt-3 flex w-full flex-1 flex-col">
        {readout.status !== "ok" && !body ? (
          // 假事实红线：读不到 / 未就绪显示原因，绝不显示 0
          <p className="text-[11px] leading-relaxed text-ink-faint">{readout.note ?? "—"}</p>
        ) : (
          content
        )}
      </div>
    </>
  );

  if (!onOpen) {
    return (
      <div className={shell} title={caption}>
        {inner}
      </div>
    );
  }
  return (
    <button type="button" onClick={onOpen} title={caption} className={shell}>
      {inner}
    </button>
  );
}
