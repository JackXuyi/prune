import { useEffect, useRef, useState } from "react";

import { useI18n } from "../../i18n/store";
import { formatBytes, formatRate, formatSignedBytes } from "../../lib/format";
import type { MemSnapshot } from "../../types/mem";
import type { SwapRateReport } from "../../types/mem";
import { SwapIcon, ThrashIcon } from "../icons";
import { InfoTile } from "../stats/InfoTile";
import { Sparkline } from "../Sparkline";

/**
 * 内存页双磁贴（eul 重设计 · memory-eul-mockup.html ③）：
 * - 交换空间：主读数占用比 + tail「已用/总量」+ 阈值带（75/90 刻度画在轨道上）
 * - 换页颠簸：主读数换入速率 + tail「出 x/s」+ 换入走势线
 *
 * 换页走势是**前端累积**的：memStore 的快照事件（后台常采，≈10 s 一轮）每次
 * 到达 push 一个点，只在内存页打开时积累——零新增 IPC 采样。不足 2 个点不画线
 * （诚实红线：不画假走势）。swap.total=0 / thrash 缺口径 → 整块磁贴消失。
 */
export function MemTiles({
  snapshot,
  swapRates,
}: {
  snapshot: MemSnapshot;
  /** F24 换出增速（读序列库历史）；null = 还没拉到 */
  swapRates: SwapRateReport | null;
}) {
  const { swap, thrash } = snapshot;
  const hasSwap = swap.total > 0;
  const showSwapTile = hasSwap && swapRatioOf(swap) != null;
  const showThrashTile = thrash != null;

  if (!showSwapTile && !showThrashTile) return null;
  // auto-fit：Windows 只剩一块磁贴时自动占满整行，不留半行空白
  return (
    <div className="grid shrink-0 grid-cols-[repeat(auto-fit,minmax(0,1fr))] items-stretch gap-2">
      {showSwapTile ? <SwapTile snapshot={snapshot} swapRates={swapRates} /> : null}
      {showThrashTile ? <ThrashTile snapshot={snapshot} /> : null}
    </div>
  );
}

function swapRatioOf(swap: MemSnapshot["swap"]): number | null {
  return swap.total > 0 ? swap.used / swap.total : null;
}

/** 交换空间磁贴：阈值色带 + 75/90 竖刻度，与 UI v4 swap 条同语义 */
function SwapTile({
  snapshot,
  swapRates,
}: {
  snapshot: MemSnapshot;
  swapRates: SwapRateReport | null;
}) {
  const { t } = useI18n();
  const { swap } = snapshot;
  const ratio = swapRatioOf(swap) ?? 0;

  // 口径行：macOS 有 swapfile 口径 → 「今日新增 n · 近 1h rate」；
  // Windows 只有 pagefile 与（可能有）近 1h 增速；什么都没有 → 只写 pagefile
  const rate1h = formatSignedBytes(swapRates?.recent1h?.sumDeltaBytes ?? null, "/h");
  let sub: string;
  if (swap.fileCount != null && rate1h != null) {
    sub = t("memory.tiles.swapSubMac", { n: swap.fileCount, rate: rate1h });
  } else if (swap.fileCount != null) {
    sub = t("memory.tiles.swapSubFiles", { n: swap.fileCount });
  } else if (rate1h != null) {
    sub = t("memory.tiles.swapSub1h", { rate: rate1h });
  } else {
    sub = t("memory.tiles.swapSubPagefile");
  }

  return (
    <InfoTile
      title={t("memory.tiles.swap")}
      caption={t("memory.swap.gaugeTip")}
      color={ratio >= 0.9 ? "var(--color-danger)" : ratio >= 0.75 ? "var(--color-warn)" : "var(--color-accent)"}
      icon={<SwapIcon size={12} />}
      main={{
        value: String(Math.round(ratio * 100)),
        unit: "%",
        tail: `${formatBytes(swap.used)} / ${formatBytes(swap.total)}`,
      }}
      sub={sub}
      viz={
        <div className="relative h-2 overflow-hidden rounded-[4px] bg-chip" title={t("memory.swap.bandTip")}>
          <div
            className="absolute inset-0 opacity-30"
            style={{
              background:
                "linear-gradient(90deg, var(--color-accent) 0 75%, var(--color-warn) 75% 90%, var(--color-danger) 90%)",
            }}
          />
          <div
            className="prune-anim absolute inset-y-0 left-0 rounded-[4px] transition-[width] duration-500"
            style={{
              width: `${Math.min(100, ratio * 100)}%`,
              background:
                ratio >= 0.9
                  ? "var(--color-danger)"
                  : ratio >= 0.75
                    ? "var(--color-warn)"
                    : "var(--color-accent)",
            }}
          />
          <div className="absolute -inset-y-px w-px bg-ink-faint/80" style={{ left: "75%" }} title={t("memory.swap.warnMark")} />
          <div className="absolute -inset-y-px w-px bg-ink-faint/80" style={{ left: "90%" }} title={t("memory.swap.dangerMark")} />
        </div>
      }
    />
  );
}

/** 换页颠簸磁贴：换入速率为主读数，走势为前端累积序列（≥2 点才画） */
function ThrashTile({ snapshot }: { snapshot: MemSnapshot }) {
  const { t } = useI18n();
  const thrash = snapshot.thrash;
  const [series, setSeries] = useState<number[]>([]);
  // 用快照时间戳去重：同一轮事件（mem://snapshot 与 mem://deep 双推）只记一个点
  const lastAt = useRef<number | null>(null);

  useEffect(() => {
    if (thrash == null) return;
    const at = snapshot.fastAt;
    if (lastAt.current === at) return;
    lastAt.current = at;
    setSeries((prev) => [...prev.slice(-59), thrash.swapinsPerSec]);
  }, [thrash, snapshot.fastAt]);

  if (thrash == null) return null;

  const hasLine = series.length >= 2;
  return (
    <InfoTile
      title={t("memory.tiles.thrash")}
      caption={t("memory.ribbon.thrashTip", {
        i: formatRate(thrash.swapinsPerSec),
        o: formatRate(thrash.swapoutsPerSec),
      })}
      color="var(--color-warn)"
      icon={<ThrashIcon size={12} />}
      main={{
        value: formatRate(thrash.swapinsPerSec),
        unit: "/s",
        tail: `${t("memory.tiles.swapin")} ${formatRate(thrash.swapoutsPerSec)}/s`,
      }}
      sub={
        hasLine
          ? t("memory.tiles.thrashSub", { o: formatRate(thrash.swapoutsPerSec) })
          : t("memory.tiles.thrashAccum")
      }
      viz={
        hasLine ? (
          <Sparkline values={series} height={22} tone="warn" />
        ) : undefined
      }
    />
  );
}
