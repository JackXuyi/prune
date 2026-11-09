import type { ReactNode } from "react";

import { formatPercent } from "../../lib/format";
import type { NetDirectionReadout, Readout } from "../../lib/statsReadout";
import { Meter } from "../primitives";
import { Sparkline } from "../Sparkline";
import { Reading, TagDot } from "./StatsCard";

/**
 * 面板统计行（M16）：高度**锁定 48 px**。
 *
 * 36 px 内容 + 12 px 内边距，行内两段：
 * - 第一行 18：标签 + 状态点 / 主读数（含单位与共用分母）
 * - 第二行 14：Meter 4 px + 副读数（无占比语义的域整行给文字）
 *
 * 用 `h-12` 而不是 `min-h`：面板 400 高的预算是一行一行加出来的（五行 256
 * 是唯一的弹性来源，其余都固定），副读数一旦换成两行就会把整块内容推到滚动区。
 */
export function StatsRow({
  icon,
  title,
  readout,
  onOpen,
}: {
  icon?: ReactNode;
  title: string;
  readout: Readout;
  onOpen?: () => void;
}) {
  const ok = readout.status === "ok";
  // 有占比条时文字只剩约 180 px，取 2 条；没有占比条就整行给文字，取 3 条
  const subLimit = readout.ratio != null ? 2 : 3;
  const subText = ok
    ? [
        readout.ratio != null ? formatPercent(readout.ratio) : null,
        ...readout.sub.slice(0, subLimit),
      ]
        .filter(Boolean)
        .join(" · ")
    : "";
  const fullText = ok
    ? [readout.ratio != null ? formatPercent(readout.ratio) : null, ...readout.sub]
        .filter(Boolean)
        .join(" · ")
    : (readout.note ?? "");

  return (
    <button
      type="button"
      onClick={onOpen}
      disabled={!onOpen}
      title={fullText}
      className={`flex h-12 w-full flex-col justify-center gap-0.5 rounded-md px-1 text-left transition-colors ${
        onOpen ? "hover:bg-white/[0.04]" : "cursor-default"
      }`}
    >
      <div className="flex w-full items-center gap-2">
        <span className="flex min-w-0 items-center gap-1.5 text-[11px] text-ink-dim">
          {icon ? <span className="shrink-0">{icon}</span> : null}
          <span className="truncate font-medium">{title}</span>
          {ok && readout.tag ? <TagDot tag={readout.tag} /> : null}
        </span>
        <span className="flex-1" />
        {ok ? (
          <Reading readout={readout} size="sm" />
        ) : (
          <span className="max-w-[58%] shrink-0 truncate text-[10px] text-ink-faint">
            {readout.note ?? "—"}
          </span>
        )}
      </div>
      <div className="flex w-full items-center gap-2">
        {ok && readout.ratio != null ? (
          <span className="w-[44%] shrink-0">
            {/* 3 px 面板规格（§7.6）：行高 48 px 预算下 4 px 会挤到副读数 */}
            <Meter ratio={readout.ratio} tone={readout.tone} height={3} />
          </span>
        ) : null}
        <span
          className={`min-w-0 flex-1 truncate text-[10px] leading-none ${
            ok ? "text-ink-faint" : "text-ink-faint/70"
          }`}
        >
          {ok ? subText : "点刷新可重扫一轮"}
        </span>
      </div>
    </button>
  );
}

/**
 * 面板方向行（M16 v6）：网络从一条 64 px 例外行拆成两条 48 px 标准行。
 *
 * 下载 / 上传各占一行，方向在对齐的位置各有一份读数与迷你走势——原来两个
 * 读数挤一行右侧、合并走势分不出方向。走势仍各自归一化（量级常差 10 倍），
 * `baselineZero` 让空闲噪声不被放大。
 *
 * 面板是「瞄一眼」场景：副读数只留近 5 min 均值一条，今日 / 峰值 / 会话
 * 累计进 `title`（全量脚注），明细在主窗口网络 tab。
 */
export function NetDirectionRow({
  readout,
  onOpen,
}: {
  readout: NetDirectionReadout;
  onOpen?: () => void;
}) {
  const ok = readout.status === "ok";
  const down = readout.dir === "down";
  const title = down ? "下载" : "上传";
  const arrow = down ? "↓" : "↑";
  const arrowText = down ? "text-accent" : "text-info";
  const valueText = down ? "text-accent" : "text-info";
  const fullText = [
    ok ? `近 5 min ${readout.avgText}` : null,
    readout.todayText ? `今日 ${readout.todayText}` : null,
    readout.peakText ? `峰值 ${readout.peakText}` : null,
    readout.sessionText ? `会话累计 ${readout.sessionText}` : null,
    !ok ? (readout.note ?? "—") : null,
  ]
    .filter(Boolean)
    .join(" · ");

  return (
    <button
      type="button"
      onClick={onOpen}
      disabled={!onOpen}
      title={fullText}
      className={`flex h-12 w-full flex-col justify-center gap-0.5 rounded-md px-1 text-left transition-colors ${
        onOpen ? "hover:bg-white/[0.04]" : "cursor-default"
      }`}
    >
      <div className="flex w-full items-center gap-2">
        <span className="flex min-w-0 items-center gap-1.5 text-[11px] text-ink-dim">
          <span className={`shrink-0 ${arrowText}`}>{arrow}</span>
          <span className="truncate font-medium">{title}</span>
        </span>
        <span className="flex-1" />
        {ok ? (
          <span className="flex shrink-0 items-baseline gap-1">
            <span
              className={`font-mono text-[13.5px] leading-none font-medium tabular-nums ${valueText}`}
            >
              {readout.main}
            </span>
            <span className="text-[10px] text-ink-faint">{readout.unit}</span>
          </span>
        ) : (
          <span className="max-w-[58%] shrink-0 truncate text-[10px] text-ink-faint">
            {readout.note ?? "—"}
          </span>
        )}
      </div>
      <div className="flex w-full items-center gap-2">
        {ok ? (
          <>
            <span className="w-[44%] shrink-0">
              {readout.series.length >= 2 ? (
                <Sparkline
                  values={readout.series}
                  height={14}
                  tone={down ? "accent" : "info"}
                  baselineZero
                />
              ) : (
                <span className="block h-[14px] text-[9px] leading-[14px] text-ink-faint/70">
                  走势积累中
                </span>
              )}
            </span>
            <span className="min-w-0 flex-1 truncate text-[10px] leading-none text-ink-faint">
              近 5 min {readout.avgText}
            </span>
          </>
        ) : null}
      </div>
    </button>
  );
}
