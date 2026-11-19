import { useI18n } from "../../i18n/store";
import { formatBytes } from "../../lib/format";
import type { CacheGroup, DupSummary } from "../../types/disk";
import { cacheLabel } from "../../types/disk";
import { ChipIcon, StackIcon } from "../icons";
import { StackBar } from "../charts/StackBar";
import { InfoTile } from "../stats/InfoTile";

/**
 * 存储磁贴（eul 重设计 · storage-eul-mockup.html ③）：死重比 + 重复依赖。
 *
 * 取舍：原 DeadweightCard 的平台明细 / Top 条目收进 L1 分级展开的 GroupDetails，
 * 磁贴只留「一眼能读的量」；重复依赖从整卡降级为磁贴。
 * **无数据整块消失**：deadweight 为空或 dup 为 null 的磁贴不渲染，网格不留白
 * （两个都缺 → 整行不渲染）。
 *
 * 「环=占比」纪律：死重比是**比值**（浪费 ÷ 本机原生），不是百分比 → 不套环。
 */
export function StTiles({
  deadweightGroup,
  dup,
}: {
  /** A1 判定组：带 deadweight 的缓存组（通常 0 或 1 个） */
  deadweightGroup: CacheGroup | null;
  dup: DupSummary | null;
}) {
  const { t, locale } = useI18n();
  const dw = deadweightGroup?.deadweight ?? null;

  const showDw = dw != null && dw.bytes > 0;
  if (!showDw && dup == null) return null;

  return (
    <div className="flex shrink-0 flex-col gap-[9px]">
      {showDw && dw ? (
        <InfoTile
          title={t("disk.tile.deadweight", { label: cacheLabel(deadweightGroup!, locale) })}
          caption={
            dw.ratio != null
              ? t("disk.deadweight.summaryRatio", {
                  waste: formatBytes(dw.bytes),
                  native: formatBytes(dw.nativeBytes),
                  ratio: dw.ratio.toFixed(1),
                })
              : t("disk.deadweight.summary", {
                  waste: formatBytes(dw.bytes),
                  native: formatBytes(dw.nativeBytes),
                })
          }
          color="var(--color-accent)"
          icon={<StackIcon size={12} />}
          main={{ value: dw.ratio != null ? dw.ratio.toFixed(1) : "—", unit: dw.ratio != null ? ": 1" : undefined }}
          sub={
            dw.ratio != null
              ? t("disk.tile.dwSub", {
                  waste: formatBytes(dw.bytes),
                  native: formatBytes(dw.nativeBytes),
                  n: dw.byPlatform.length,
                })
              : t("disk.deadweight.summary", {
                  waste: formatBytes(dw.bytes),
                  native: formatBytes(dw.nativeBytes),
                })
          }
          viz={
            <StackBar
              height={7}
              ariaLabel={
                dw.ratio != null
                  ? t("disk.deadweight.ariaRatio", {
                      waste: formatBytes(dw.bytes),
                      native: formatBytes(dw.nativeBytes),
                      ratio: dw.ratio.toFixed(1),
                    })
                  : t("disk.deadweight.aria", {
                      waste: formatBytes(dw.bytes),
                      native: formatBytes(dw.nativeBytes),
                    })
              }
              segments={[
                { key: "waste", label: t("disk.deadweight.segWaste"), value: dw.bytes, tone: "accent" },
                { key: "native", label: t("disk.deadweight.segNative"), value: dw.nativeBytes, tone: "neutral" },
              ]}
            />
          }
        />
      ) : null}

      {dup ? (
        <InfoTile
          title={t("disk.dup.title", { n: dup.managers.length })}
          caption={dup.advice}
          color="var(--color-warn)"
          icon={<ChipIcon size={12} />}
          main={{ value: formatBytes(dup.totalBytes) }}
          sub={dup.advice}
          viz={
            <span className="flex flex-wrap gap-1">
              {dup.managers.map(([name, bytes]) => (
                <span key={name} className="rounded-full bg-chip px-1.5 py-px text-[9px] text-ink-dim">
                  {name}
                  <b className="ml-1 font-mono tabular-nums text-ink">{formatBytes(bytes)}</b>
                </span>
              ))}
            </span>
          }
        />
      ) : null}
    </div>
  );
}
