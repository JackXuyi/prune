import { useI18n } from "../../i18n/store";
import { formatBytes, formatPercent, formatRate } from "../../lib/format";
import type { MemSnapshot } from "../../types/mem";
import {
  CompressIcon,
  MemoryCloudIcon,
  MemoryFreeIcon,
  SwapIcon,
  ThrashIcon,
} from "../icons";
import { RibbonChip, RibbonSeg, RibbonShell, RibbonValue } from "../Ribbon";
import { MiniGauge } from "../stats/MiniGauge";

/**
 * 内存页状态带（eul 重设计 · memory-eul-mockup.html ①）：
 * 6 段「图标 + mono 读数」——已用（环）/ 可用 / 压缩 / Swap（环）/ 换页 / 压力。
 *
 * 「环=占比」纪律：只有百分比读数（已用、Swap 占用比）套环；
 * 容量、速率保持图标芯片；压力不是百分比也不是速率，用「电平条 + 档位」。
 *
 * 诚实红线：某平台没有口径 → 整段隐藏（压缩 / 换页 / 压力 / swap=0），
 * 绝不显示 0 或「不可用」占位。口径全进 tooltip。
 */

/** 压力档位 → 电平条亮几格（三格：正常 / 警告 / 紧张） */
function pressureLit(raw: number, metric: "kernel" | "occupancy"): 1 | 2 | 3 {
  if (metric === "kernel") return raw >= 4 ? 3 : raw >= 2 ? 2 : 1;
  return raw >= 92 ? 3 : raw >= 80 ? 2 : 1;
}

export function MemRibbon({ snapshot }: { snapshot: MemSnapshot }) {
  const { t } = useI18n();
  const { physicalTotal, physicalUsedBytes, freeBytes, compressorBytes, swap, pressure, thrash } =
    snapshot;

  const used = physicalUsedBytes;
  const usedRatio = used != null && physicalTotal > 0 ? used / physicalTotal : null;
  const swapRatio = swap.total > 0 ? swap.used / swap.total : null;
  const isMac = compressorBytes != null || swap.fileCount != null;

  return (
    <RibbonShell ariaLabel={t("memory.ribbon.aria")}>
      {/* 已用：百分比 → 套环；旧快照无此口径 → 空轨道 + 「—」，不画 0 弧 */}
      <RibbonSeg
        title={
          used != null && physicalTotal > 0
            ? isMac
              ? t("memory.ribbon.usedTip", {
                  used: formatBytes(used),
                  total: formatBytes(physicalTotal),
                  pct: formatPercent(usedRatio ?? 0),
                })
              : t("memory.ribbon.usedTipWin", {
                  used: formatBytes(used),
                  total: formatBytes(physicalTotal),
                  pct: formatPercent(usedRatio ?? 0),
                })
            : t("memory.ribbon.usedUnknown")
        }
      >
        <MiniGauge
          ratio={usedRatio}
          color="var(--color-info)"
          icon={<MemoryCloudIcon size={11} />}
        />
        <RibbonValue
          value={usedRatio != null ? String(Math.round(usedRatio * 100)) : "—"}
          unit={usedRatio != null ? "%" : undefined}
        />
      </RibbonSeg>

      {/* 可用：容量 → 图标芯片 */}
      <RibbonSeg title={t("memory.hero.availableTip", { free: formatBytes(freeBytes) })}>
        <RibbonChip color="var(--color-accent)" icon={<MemoryFreeIcon size={13} />} />
        <RibbonValue value={formatBytes(freeBytes)} />
      </RibbonSeg>

      {/* 压缩：Windows 无口径 → 整段隐藏 */}
      {compressorBytes != null ? (
        <RibbonSeg title={t("memory.hero.compressedTip")}>
          <RibbonChip color="var(--color-info)" icon={<CompressIcon size={13} />} />
          <RibbonValue value={formatBytes(compressorBytes)} />
        </RibbonSeg>
      ) : null}

      {/* Swap：swap 配置为 0 → 整段不画（不是 0%） */}
      {swap.total > 0 && swapRatio != null ? (
        <RibbonSeg
          title={t("memory.ribbon.swapTip", {
            used: formatBytes(swap.used),
            total: formatBytes(swap.total),
            pct: formatPercent(swapRatio),
          })}
        >
          <MiniGauge
            ratio={swapRatio}
            color={swapRatio >= 0.9 ? "var(--color-danger)" : swapRatio >= 0.75 ? "var(--color-warn)" : "var(--color-accent)"}
            icon={<SwapIcon size={11} />}
          />
          <RibbonValue
            value={String(Math.round(swapRatio * 100))}
            unit="%"
            tone={swapRatio >= 0.9 ? "danger" : swapRatio >= 0.75 ? "warn" : undefined}
          />
        </RibbonSeg>
      ) : null}

      {/* 换页：无换页计数平台（Windows）→ 整段隐藏 */}
      {thrash != null ? (
        <RibbonSeg
          title={t("memory.ribbon.thrashTip", {
            i: formatRate(thrash.swapinsPerSec),
            o: formatRate(thrash.swapoutsPerSec),
          })}
        >
          <RibbonChip color="var(--color-warn)" icon={<ThrashIcon size={13} />} />
          <RibbonValue value={formatRate(thrash.swapinsPerSec)} unit="/s" />
        </RibbonSeg>
      ) : null}

      {/* 压力：读不到 → 隐藏；有值用「电平条 + 档位」，不用文字「正常」 */}
      {pressure ? (
        <RibbonSeg title={pressureTip(pressure, t)}>
          <PressureLevelBars
            lit={pressureLit(pressure.raw, pressure.metric)}
            level={pressure.level}
          />
          <RibbonValue
            value={String(pressure.raw)}
            unit={pressure.metric === "kernel" ? "/4" : "%"}
            tone={
              pressure.level === "critical" ? "danger" : pressure.level === "warning" ? "warn" : undefined
            }
          />
        </RibbonSeg>
      ) : null}
    </RibbonShell>
  );
}

function pressureTip(
  pressure: NonNullable<MemSnapshot["pressure"]>,
  t: ReturnType<typeof useI18n>["t"],
): string {
  return pressure.metric === "occupancy"
    ? t("memory.pressure.captionOccupancy", { raw: pressure.raw })
    : t("memory.pressure.captionKernel", { raw: pressure.raw });
}

/** 三段电平条（7/10.5/14px）：亮格数 + 档位色，替代文字「正常」 */
function PressureLevelBars({ lit, level }: { lit: 1 | 2 | 3; level: "normal" | "warning" | "critical" }) {
  const color =
    level === "critical"
      ? "var(--color-danger)"
      : level === "warning"
        ? "var(--color-warn)"
        : "var(--color-accent)";
  const heights = [7, 10.5, 14];
  return (
    <span className="flex shrink-0 items-end gap-[2.5px]" aria-hidden>
      {heights.map((h, i) => (
        <span
          key={h}
          className="w-[3px] rounded-[1.5px]"
          style={{
            height: h,
            background: i < lit ? color : "var(--color-chip)",
            boxShadow: i < lit ? `0 0 5px color-mix(in srgb, ${color} 55%, transparent)` : undefined,
          }}
        />
      ))}
    </span>
  );
}
