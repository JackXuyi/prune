import type { ReactNode } from "react";

import { formatPercent } from "../../lib/format";
import type { NetReadout, Readout } from "../../lib/statsReadout";
import { formatSpeed } from "../../types/net";
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
            <Meter ratio={readout.ratio} tone={readout.tone} height={4} />
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
 * 面板网络行（M16）：64 px，是 48 px 规则的唯一例外。
 *
 * 压到 48 就只剩 4 px 走势，等于没有；所以单独放宽 16 px，换来一条 18 px
 * 合并走势（下行 + 上行）与一行「近 5 min / 今日」次读数。
 */
export function NetStatsRow({
  readout,
  onOpen,
}: {
  readout: NetReadout;
  onOpen?: () => void;
}) {
  const ok = readout.status === "ok";
  const sub = readout.sub.slice(0, 2).join(" · ");

  return (
    <button
      type="button"
      onClick={onOpen}
      disabled={!onOpen}
      title={readout.sub.join(" · ")}
      className={`flex h-16 w-full flex-col justify-center gap-0.5 rounded-md px-1 text-left transition-colors ${
        onOpen ? "hover:bg-white/[0.04]" : "cursor-default"
      }`}
    >
      <div className="flex w-full items-center gap-2">
        <span className="flex shrink-0 items-center gap-1.5 text-[11px] text-ink-dim">
          <span className="text-accent">⇅</span>
          <span className="font-medium">网络</span>
        </span>
        <span className="flex-1" />
        {ok ? (
          <>
            <span className="shrink-0 font-mono text-[13px] leading-none font-medium tabular-nums text-accent">
              ↓ {formatSpeed(readout.downBps)}
            </span>
            <span className="shrink-0 font-mono text-[13px] leading-none font-medium tabular-nums text-info">
              ↑ {formatSpeed(readout.upBps)}
            </span>
          </>
        ) : (
          <span className="max-w-[62%] shrink-0 truncate text-[10px] text-ink-faint">
            {readout.note ?? "—"}
          </span>
        )}
      </div>

      {ok ? (
        <>
          <div className="h-[18px] w-full">
            {readout.totalSeries.length >= 2 ? (
              <Sparkline values={readout.totalSeries} height={18} baselineZero />
            ) : (
              <div className="flex h-[18px] items-center text-[9.5px] text-ink-faint/70">
                走势积累中（每秒一个点）
              </div>
            )}
          </div>
          <div className="w-full truncate text-[9.5px] leading-none text-ink-faint">{sub}</div>
        </>
      ) : null}
    </button>
  );
}
