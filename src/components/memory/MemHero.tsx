import { useI18n } from "../../i18n/store";
import { formatBytes, formatPercent, formatRelative } from "../../lib/format";
import type { MemSnapshot } from "../../types/mem";
import { RingGauge } from "../charts/RingGauge";

/**
 * 内存构成 Hero（eul 重设计 · memory-eul-mockup.html ②）：
 * 一条堆叠条替代原来的 5 个并列统计格——已用 / 缓存·可回收 / 可用。
 *
 * 诚实红线（设计稿注解）：
 * - 压缩内存已包含在「已用」内 → 只出现在图例（tooltip 写明不重复相加），
 *   堆叠条不单独给段，防止视觉上被二次累加；
 * - 「缓存·可回收」= 总量 − 已用 − 可用 的**推导值**，斜纹表示非实测；
 *   Windows 口径推导不出（used ≡ total − free）→ 段与图例一起消失；
 * - physicalUsedBytes 未就绪（旧快照）→ 大读数显示「—」，不显示 0。
 *
 * 右侧压力环沿用 UI v4 语义：环比值口径随平台不同，caption 写明换算。
 */
export function MemHero({ snapshot }: { snapshot: MemSnapshot }) {
  const { t, locale } = useI18n();
  const { physicalTotal, physicalUsedBytes, freeBytes, compressorBytes, pressure } = snapshot;

  const used = physicalUsedBytes;
  const usedRatio = used != null && physicalTotal > 0 ? used / physicalTotal : null;
  // 缓存·可回收：推导值。推导不出（无 used 口径 / 结果非正）→ null，段与图例一起隐藏
  const cached =
    used != null && physicalTotal > 0 ? Math.max(0, physicalTotal - used - freeBytes) : null;
  const cachedRatio = cached != null && cached > 1024 && physicalTotal > 0 ? cached / physicalTotal : null;
  const freeRatio = physicalTotal > 0 ? Math.min(1, freeBytes / physicalTotal) : null;

  const hatch =
    "repeating-linear-gradient(45deg, var(--color-hatch) 0 3px, transparent 3px 6px)";

  return (
    <section
      className="relative flex shrink-0 items-center gap-4 overflow-hidden rounded-[16px] border px-[18px] py-3"
      style={{
        borderColor: "color-mix(in srgb, var(--color-info) 26%, transparent)",
        backgroundImage:
          "linear-gradient(120deg, color-mix(in srgb, var(--color-info) 9%, transparent), color-mix(in srgb, var(--color-info) 2%, transparent) 46%, transparent 76%)",
        backgroundColor: "var(--color-surface-raised)",
      }}
      title={t("memory.hero.usedTip")}
    >
      <div className="min-w-0 flex-1">
        {/* 主读数行：只有一个大数字，其余进图例与状态带 */}
        <div className="flex items-baseline gap-2.5">
          <span className="font-mono text-[25px] leading-none font-semibold tabular-nums text-ink">
            {used != null ? formatBytes(used) : "—"}
          </span>
          {usedRatio != null ? (
            <span className="font-mono text-[12px] font-semibold tabular-nums text-info">
              {formatPercent(usedRatio)}
            </span>
          ) : null}
          <span className="min-w-0 truncate text-[10px] text-ink-faint">
            {used != null && physicalTotal > 0
              ? t("memory.hero.deepAgo", {
                  when: formatRelative(snapshot.deepUpdatedAt, locale),
                })
              : t("memory.ribbon.usedUnknown")}
          </span>
        </div>

        {/* 构成堆叠条：已用（info 实色）/ 缓存·可回收（斜纹）/ 可用（accent 淡色） */}
        <div className="mt-2.5 flex h-[11px] gap-[2px] overflow-hidden rounded-[6px] bg-chip">
          {usedRatio != null && usedRatio > 0 ? (
            <i
              className="block h-full"
              style={{
                width: `${usedRatio * 100}%`,
                background:
                  "linear-gradient(90deg, color-mix(in srgb, var(--color-info) 92%, transparent), color-mix(in srgb, var(--color-info) 62%, transparent))",
              }}
            />
          ) : null}
          {cachedRatio != null ? (
            <i className="block h-full" style={{ width: `${cachedRatio * 100}%`, background: hatch }} />
          ) : null}
          {freeRatio != null && freeRatio > 0 ? (
            <i
              className="block h-full"
              style={{
                width: `${freeRatio * 100}%`,
                background:
                  "linear-gradient(90deg, color-mix(in srgb, var(--color-accent) 30%, transparent), color-mix(in srgb, var(--color-accent) 12%, transparent))",
              }}
            />
          ) : null}
        </div>

        {/* 图例：色块 + 值；压缩只在有口径时出现且注明「已含在已用内」 */}
        <div className="mt-2 flex flex-wrap gap-x-3 gap-y-1">
          {used != null ? (
            <LegendDot
              swatch="var(--color-info)"
              label={t("memory.stat.used")}
              value={formatBytes(used)}
              tip={t("memory.hero.usedTip")}
            />
          ) : null}
          {compressorBytes != null ? (
            <LegendDot
              swatch="color-mix(in srgb, var(--color-info) 45%, transparent)"
              label={t("memory.stat.compressed")}
              value={formatBytes(compressorBytes)}
              tip={t("memory.hero.compressedTip")}
            />
          ) : null}
          {cached != null && cachedRatio != null ? (
            <LegendDot
              swatch={hatch}
              label={t("memory.hero.cached")}
              value={formatBytes(cached)}
              tip={t("memory.hero.cachedTip")}
            />
          ) : null}
          <LegendDot
            swatch="var(--color-accent)"
            label={t("memory.stat.available")}
            value={formatBytes(freeBytes)}
            tip={t("memory.hero.availableTip", { free: formatBytes(freeBytes) })}
          />
        </div>
      </div>

      {/* 压力环：kernel 1/4 → 环心 25；occupancy → 环心占用率 */}
      {pressure ? <PressureRing pressure={pressure} /> : null}
    </section>
  );
}

function LegendDot({
  swatch,
  label,
  value,
  tip,
}: {
  swatch: string;
  label: string;
  value: string;
  tip: string;
}) {
  return (
    <span className="flex items-center gap-1.5 text-[9.5px] text-ink-faint" title={tip}>
      <em className="size-[7px] shrink-0 rounded-[2px]" style={{ background: swatch }} aria-hidden />
      {label}
      <b className="font-mono text-[10.5px] font-medium tabular-nums text-ink-dim">{value}</b>
    </span>
  );
}

/**
 * 内存压力环（UI v4 §6.3 语义不变）：黄=Warning / 红=Critical 发光弧。
 * 比值口径随平台 metrics 不同：occupancy 是占用率（raw/100），
 * kernel 是 1/2/4 档（raw/4）——caption 写明换算，不冒充占用率。
 */
function PressureRing({
  pressure,
}: {
  pressure: NonNullable<MemSnapshot["pressure"]>;
}) {
  const ratio = pressure.metric === "occupancy" ? pressure.raw / 100 : pressure.raw / 4;
  const critical = pressure.metric === "occupancy" ? pressure.raw >= 92 : pressure.raw >= 4;
  const warning = !critical && (pressure.metric === "occupancy" ? pressure.raw >= 80 : pressure.raw >= 2);
  const color = critical
    ? "var(--color-danger)"
    : warning
      ? "var(--color-warn)"
      : "var(--color-accent)";
  const { t } = useI18n();
  const caption =
    pressure.metric === "occupancy"
      ? t("memory.pressure.captionOccupancy", { raw: pressure.raw })
      : t("memory.pressure.captionKernel", { raw: pressure.raw });

  return (
    <div className="shrink-0" title={caption}>
      <RingGauge
        segments={[{ value: Math.max(0.04, Math.min(1, ratio)), color }]}
        size={104}
        stroke={9}
        ariaLabel={t("memory.pressure.aria", { caption })}
      >
        <div className="text-center">
          <span
            className="block font-mono text-[20px] leading-none tabular-nums"
            style={{ color, textShadow: `0 0 12px color-mix(in srgb, ${color} 45%, transparent)` }}
          >
            {pressure.metric === "occupancy" ? pressure.raw : Math.round(ratio * 100)}
          </span>
          <span className="mt-0.5 block text-[9px] text-ink-faint">
            {pressure.metric === "occupancy"
              ? t("memory.pressure.occupancyUnit")
              : `${t("memory.pressure.title")} ${pressure.raw}/4`}
          </span>
        </div>
      </RingGauge>
    </div>
  );
}
