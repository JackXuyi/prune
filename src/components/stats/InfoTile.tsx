import type { ReactNode } from "react";

import type { Tone } from "../../lib/statsReadout";
import { ChevronRightIcon } from "../icons";
import { MiniGauge } from "./MiniGauge";

/** 主读数行（InfoTile）：数值 + 弱化单位 + 共用分母；由调用方从 Readout 拆出 */
export interface TileMain {
  value: string;
  unit?: string;
  tail?: string;
  tone?: Tone;
}

/**
 * InfoTile（eul 式图标优先 · 首页重设计）：总览磁贴统一形态。
 *
 * 三层克制文字（用户定稿）：标题 11px dim（仪表/图标旁）+ 大读数 +
 * 一行口径 10px faint；完整口径仍走 tooltip（少文字原则的兜底层）。
 * 图标位二态：百分比域传 `gaugeRatio` 渲染 MiniGauge，非百分比域
 * 传 plain 图标芯片——「环=占比」语义不被稀释。
 */
export function InfoTile({
  title,
  caption,
  color,
  icon,
  gaugeRatio,
  main,
  note,
  sub,
  viz,
  onOpen,
}: {
  title: string;
  /** hover tooltip（完整口径） */
  caption: string;
  /** 域色（CSS 颜色，推荐传 var(--color-*)） */
  color: string;
  /** 图标（环心或芯片内容，调用方定好尺寸：环心约 12，芯片 13） */
  icon: ReactNode;
  /** 传了就渲染环形仪表；不传渲染图标芯片 */
  gaugeRatio?: number | null;
  /** 主读数；缺省时显示 `note`（读不到的原因，绝不显示 0） */
  main?: TileMain;
  /** main 缺省时的状态说明 */
  note?: string;
  /** 读数下方的一行口径（10px faint，单行截断） */
  sub?: ReactNode;
  /** 卡底微图表位 */
  viz?: ReactNode;
  /** 存在时整卡可点（只导航，不做任何清理动作——红线不变） */
  onOpen?: () => void;
}) {
  const toneText =
    main?.tone === "danger"
      ? "text-danger"
      : main?.tone === "warn"
        ? "text-warn"
        : main?.tone === "info"
          ? "text-info"
          : main?.tone === "accent"
            ? "text-accent"
            : "text-ink";

  const head = (
    <div className="relative flex w-full items-center">
      {gaugeRatio != null ? (
        <MiniGauge ratio={gaugeRatio} color={color} size={24} stroke={2.4} icon={icon} />
      ) : (
        <span
          className="grid size-[24px] shrink-0 place-items-center rounded-[7px]"
          style={{
            background: `color-mix(in srgb, ${color} 15%, transparent)`,
            color,
            boxShadow: `0 0 10px color-mix(in srgb, ${color} 20%, transparent)`,
          }}
          aria-hidden
        >
          {icon}
        </span>
      )}
      <span className="ml-2 min-w-0 flex-1 truncate text-left text-[11px] font-semibold text-ink-dim">
        {title}
      </span>
      {onOpen ? (
        <ChevronRightIcon
          size={11}
          className="shrink-0 text-ink-faint opacity-0 transition-[opacity,transform] group-hover:translate-x-0.5 group-hover:opacity-100"
        />
      ) : null}
    </div>
  );

  const body = (
    <>
      {head}
      <div className="relative mt-1.5 w-full">
        {main ? (
          <span className="flex items-baseline gap-1">
            <span
              className={`font-mono text-[23px] leading-none font-semibold tabular-nums ${toneText}`}
            >
              {main.value}
            </span>
            {main.unit ? <span className="text-[11px] text-ink-faint">{main.unit}</span> : null}
            {main.tail ? <span className="text-[10px] text-ink-faint">{main.tail}</span> : null}
          </span>
        ) : (
          // 假事实红线：读不到 / 未就绪显示原因，绝不显示 0
          <p className="truncate text-[11px] leading-tight text-ink-faint" title={note}>
            {note ?? "—"}
          </p>
        )}
      </div>
      {sub ? (
        <div
          className="relative mt-1.5 w-full shrink-0 truncate text-[10px] leading-tight text-ink-faint"
          title={typeof sub === "string" ? sub : undefined}
        >
          {sub}
        </div>
      ) : null}
      {viz ? <div className="relative mt-auto w-full shrink-0 pt-1.5">{viz}</div> : null}
    </>
  );

  // 顶缘域色洗光 + 底缘发丝线：与设计稿一致，正文底色仍是中性 raised
  const wash = (
    <>
      <span
        aria-hidden
        className="pointer-events-none absolute inset-0"
        style={{
          backgroundImage: `linear-gradient(180deg, color-mix(in srgb, ${color} 6%, transparent), transparent 60%)`,
        }}
      />
      <span
        aria-hidden
        className="pointer-events-none absolute inset-x-0 bottom-0 h-[2px]"
        style={{
          background: `linear-gradient(90deg, transparent, color-mix(in srgb, ${color} 40%, transparent) 30%, color-mix(in srgb, ${color} 40%, transparent) 70%, transparent)`,
        }}
      />
    </>
  );

  const shell =
    "group relative flex h-full min-h-[104px] w-full flex-col overflow-hidden rounded-card border border-hairline-soft bg-surface-raised px-3.5 py-2.5 text-left transition-[border-color,transform,box-shadow] duration-150 hover:-translate-y-px hover:border-hairline hover:shadow-lg hover:shadow-black/10";

  if (!onOpen) {
    return (
      <div className={shell} title={caption}>
        {wash}
        {body}
      </div>
    );
  }
  return (
    <button type="button" onClick={onOpen} title={caption} className={shell}>
      {wash}
      {body}
    </button>
  );
}
