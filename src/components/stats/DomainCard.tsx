/**
 * 域卡（UI v5 §4.1）：总览六张卡的统一形态 —— 「图标 + 大数字 + 微图表 + 一行口径」。
 *
 * v5 与 v4 的差别只有一个：**固定 96 高、只留一行脚注**。
 * 六张卡从 3 行 × 140 压到 2 行 × 96，省下的 240 px 让「剪除成绩单」不用滚动
 * 就能出现在首屏 —— 这是「总览一屏读完」的前提。
 *
 * 口径正文（caption）仍然保留，但只在 hover tooltip 里；卡面只留一条最关键的
 * 读数（`footnote`）。数字仍来自同一份 `Readout`（statsReadout 同源层）。
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
  footnote,
}: {
  title: string;
  /** hover tooltip（少文字原则：caption 不再平铺） */
  caption: string;
  icon: ReactNode;
  /** 域色（CSS 颜色，推荐传 var(--color-*)） */
  color: string;
  readout: Readout;
  /** 存在时整卡可点（只导航，不做任何清理动作——红线不变） */
  onOpen?: () => void;
  /** 卡片下半的微图表位（面积走势 / 阈值条 / 点阵） */
  chart?: ReactNode;
  /** 自定义主体（网络方向卡用）：替换「大数字」那一行 */
  body?: ReactNode;
  /** 卡底唯一一行关键口径（9.5 faint，单行截断） */
  footnote?: ReactNode;
}) {
  const shell =
    "group flex h-[96px] w-full flex-col rounded-card border border-hairline-soft bg-surface-raised px-3.5 py-2.5 text-left transition-[border-color,transform] duration-150 hover:-translate-y-px hover:border-hairline";

  const inner = (
    <>
      {/* w-full：WKWebView 里 button 作 flex 容器时交叉轴 stretch 不生效，
          子元素会收缩成内容宽（右对齐/撑满全失效），必须显式给宽度 */}
      <div className="flex w-full items-center gap-1.5">
        <span
          className="grid size-[18px] shrink-0 place-items-center rounded-[5px]"
          style={{
            background: `color-mix(in srgb, ${color} 14%, transparent)`,
            color,
          }}
          aria-hidden
        >
          {icon}
        </span>
        <span className="min-w-0 flex-1 truncate text-[10.5px] font-medium tracking-wide text-ink-dim">
          {title}
        </span>
        {onOpen ? (
          <ChevronRightIcon
            size={11}
            className="shrink-0 text-ink-faint opacity-0 transition-[opacity,transform] group-hover:translate-x-0.5 group-hover:opacity-100"
          />
        ) : null}
      </div>

      <div className="mt-1 w-full">
        {readout.status !== "ok" && !body ? (
          // 假事实红线：读不到 / 未就绪显示原因，绝不显示 0
          <p className="truncate text-[11px] leading-tight text-ink-faint" title={readout.note}>
            {readout.note ?? "—"}
          </p>
        ) : (
          (body ?? <Reading size="md" readout={readout} />)
        )}
      </div>

      {chart ? <div className="mt-auto w-full">{chart}</div> : null}
      {footnote ? (
        <div className="mt-1 w-full truncate text-[9.5px] leading-none text-ink-faint">{footnote}</div>
      ) : null}
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
