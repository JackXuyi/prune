import type { ReactNode } from "react";

import { useI18n } from "../../i18n/store";
import { formatPercent } from "../../lib/format";
import type { Readout, Tone } from "../../lib/statsReadout";
import { ChevronRightIcon } from "../icons";
import { Meter } from "../primitives";

/** 主读数颜色：只有告警态才染色，正常占用保持中性，不制造焦虑 */
export function toneText(tone: Tone): string {
  if (tone === "danger") return "text-danger";
  if (tone === "warn") return "text-warn";
  if (tone === "accent") return "text-ink";
  return "text-ink";
}

/**
 * 大号读数（M16）：数值 + 单位 + 共用分母。
 *
 * 主窗口 24 px、面板 13.5 px，同一份 `Readout` 只是字号不同 —— 这是「两边一致」
 * 在视觉层的落点：数字的来源只有一个。
 */
export function Reading({
  readout,
  size = "lg",
}: {
  readout: Readout;
  size?: "lg" | "md" | "sm";
}) {
  const valueCls = size === "lg" ? "text-[24px]" : size === "md" ? "text-[19px]" : "text-[13.5px]";
  const unitCls = size === "lg" ? "text-[11px]" : size === "md" ? "text-[10px]" : "text-[10px]";
  return (
    <span className="flex items-baseline gap-1">
      <span className={`font-mono ${valueCls} leading-none tabular-nums ${toneText(readout.tone)}`}>
        {readout.main}
      </span>
      {readout.unit ? <span className={`${unitCls} text-ink-faint`}>{readout.unit}</span> : null}
      {readout.mainTail ? (
        <span className="text-[10px] text-ink-faint">{readout.mainTail}</span>
      ) : null}
    </span>
  );
}

/** 状态点（内存压力）：与 `PressureDot` 同一套视觉，但吃 `Readout.tag` */
export function TagDot({ tag }: { tag: NonNullable<Readout["tag"]> }) {
  const dot =
    tag.tone === "danger" ? "bg-danger" : tag.tone === "warn" ? "bg-warn" : "bg-accent";
  const text =
    tag.tone === "danger" ? "text-danger" : tag.tone === "warn" ? "text-warn" : "text-accent";
  return (
    <span className="inline-flex shrink-0 items-center gap-1.5">
      <span className={`size-1.5 rounded-full ${dot}`} />
      <span className={`text-[10px] font-medium ${text}`}>{tag.text}</span>
    </span>
  );
}

/**
 * 统计卡（主窗口 960）。`min-h-[140px]` 是算出来的：三行卡 + 标题块 + 页脚
 * 合计 574 px，刚好放进 592 px 内容区（算式见方案 §5.1）。
 */
export function StatsCard({
  title,
  caption,
  readout,
  onOpen,
  body,
  footnote,
}: {
  title: string;
  caption: string;
  readout: Readout;
  /** 存在时整卡可点（只做导航，不做任何清理动作） */
  onOpen?: () => void;
  /** 自定义主体（网络卡用）：存在时替换「主读数 + 占比 + 副读数」默认排版 */
  body?: ReactNode;
  /** 卡底图表位（总览 v3.1）：swap 阈值条 / 端口分组堆叠条等，不影响主读数排版 */
  footnote?: ReactNode;
}) {
  const { t } = useI18n();
  const shell =
    "flex min-h-[140px] flex-col rounded-xl border border-hairline-soft bg-chip-soft px-4 py-3.5";
  const content = body ?? (
    <>
      <div className="flex items-baseline justify-between gap-2">
        <Reading readout={readout} />
      </div>
      {readout.ratio != null ? (
        <div className="mt-2.5">
          <Meter ratio={readout.ratio} tone={readout.tone} />
        </div>
      ) : null}
      <div className="mt-2 flex flex-wrap items-center gap-x-3 gap-y-1 text-[10px] text-ink-faint">
        {readout.ratio != null ? (
          <span className="font-mono tabular-nums text-ink-dim">
            {formatPercent(readout.ratio)}
          </span>
        ) : null}
        {readout.tag ? <TagDot tag={readout.tag} /> : null}
        {readout.sub.map((s) => (
          <span key={s}>{s}</span>
        ))}
      </div>
    </>
  );

  const inner = (
    <>
      <div className="flex items-baseline justify-between gap-2">
        <span className="text-[11px] font-medium tracking-wide text-ink-dim">{title}</span>
        {onOpen ? (
          <ChevronRightIcon
            size={12}
            className="shrink-0 text-ink-faint transition-transform group-hover:translate-x-0.5"
          />
        ) : null}
      </div>
      <span className="mt-0.5 block text-[10px] leading-snug text-ink-faint">{caption}</span>
      <div className="mt-3 flex flex-1 flex-col">
        {readout.status !== "ok" && !body ? (
          <p className="text-[11px] leading-relaxed text-ink-faint">{readout.note ?? "—"}</p>
        ) : (
          content
        )}
      </div>
      {footnote ? <div className="mt-2 shrink-0">{footnote}</div> : null}
    </>
  );

  if (!onOpen) return <div className={shell}>{inner}</div>;
  return (
    <button
      type="button"
      onClick={onOpen}
      title={t("stats.readout.openDetail", { title })}
      className={`group text-left transition-colors hover:border-hairline hover:bg-chip-hover ${shell}`}
    >
      {inner}
    </button>
  );
}
