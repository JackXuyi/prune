import { useI18n } from "../../i18n/store";
import { formatBytes, formatPercent } from "../../lib/format";
import type { Totals, VolumeInfo } from "../../types/disk";
import { DatabaseIcon, DiskIcon, ScissorsIcon, StackIcon } from "../icons";
import { RibbonChip, RibbonSeg, RibbonShell, RibbonValue } from "../Ribbon";
import { MiniGauge } from "../stats/MiniGauge";

/**
 * 存储页状态带（eul 重设计 · storage-eul-mockup.html ①）：
 * 4 段「图标 + mono 读数」——卷已用（环）/ 可用 / 可回收 / L1 死重。
 *
 * 取舍（用户定稿）：只放**本页任务内**的读数。I/O 速率与 SMART 是设备遥测，
 * 与「看可回收 → 勾选 → 清理」无关 → 不进状态带（SMART 异常走条件提醒带）。
 *
 * 「环=占比」纪律：只有卷已用是百分比，套环；容量读数保持图标芯片。
 * volume 缺失（statvfs 失败）→ 环段整段隐藏，不画 0 弧。
 */
export function StRibbon({
  volume,
  totals,
}: {
  volume: VolumeInfo | null | undefined;
  totals: Totals;
}) {
  const { t } = useI18n();

  // 卷已用：与 VolumeCard 同口径（availableBytes 含 purgeable，偏乐观）
  const used =
    volume && volume.totalBytes > 0
      ? Math.max(0, volume.totalBytes - volume.availableBytes)
      : null;
  const usedRatio = used != null && volume ? used / volume.totalBytes : null;

  return (
    <RibbonShell ariaLabel={t("disk.ribbon.aria")}>
      {/* 卷已用：百分比 → 套环；statvfs 失败 → 整段隐藏 */}
      {volume && used != null && usedRatio != null ? (
        <RibbonSeg
          title={t("disk.ribbon.usedTip", {
            pct: formatPercent(usedRatio),
            used: formatBytes(used),
            total: formatBytes(volume.totalBytes),
          })}
        >
          <MiniGauge
            ratio={usedRatio}
            color={
              usedRatio >= 0.9
                ? "var(--color-danger)"
                : usedRatio >= 0.8
                  ? "var(--color-warn)"
                  : "var(--color-accent)"
            }
            icon={<DiskIcon size={11} />}
          />
          <RibbonValue
            value={String(Math.round(usedRatio * 100))}
            unit="%"
            tone={usedRatio >= 0.9 ? "danger" : usedRatio >= 0.8 ? "warn" : undefined}
          />
        </RibbonSeg>
      ) : null}

      {/* 可用：用户视角剩余（含 purgeable）→ 图标芯片 */}
      {volume ? (
        <RibbonSeg
          title={t("disk.ribbon.availTip", { free: formatBytes(volume.userAvailableBytes) })}
        >
          <RibbonChip color="var(--color-info)" icon={<DatabaseIcon size={13} />} />
          <RibbonValue value={formatBytes(volume.userAvailableBytes)} />
        </RibbonSeg>
      ) : null}

      {/* 可回收：L1+L2+L3（不含 L4，tooltip 写明）→ accent 强调 */}
      <RibbonSeg
        title={t("disk.ribbon.reclaimTip", {
          size: formatBytes(totals.reclaimable),
          l1: formatBytes(totals.deadweight),
          l2: formatBytes(totals.l2),
          l3: formatBytes(totals.l3),
        })}
      >
        <RibbonChip color="var(--color-accent)" icon={<ScissorsIcon size={13} />} />
        <RibbonValue value={formatBytes(totals.reclaimable)} />
      </RibbonSeg>

      {/* L1 死重：零后悔的量 */}
      <RibbonSeg title={t("disk.ribbon.l1Tip", { size: formatBytes(totals.deadweight) })}>
        <RibbonChip color="var(--color-accent)" icon={<StackIcon size={13} />} />
        <RibbonValue value={formatBytes(totals.deadweight)} />
      </RibbonSeg>
    </RibbonShell>
  );
}
