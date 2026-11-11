import { useI18n } from "../../i18n/store";
import type { NetDirectionReadout } from "../../lib/statsReadout";
import { Sparkline } from "../Sparkline";

/**
 * 方向读数主体（M16 v6）：总览页「下载 / 上传」各占一张卡，每卡一块。
 *
 * 拆卡前两个方向挤在同一张卡里共享脚注，读数只有 19 px；拆开后每卡与
 * 其它卡同级：24 px 主读数 + 各自归一化的走势 + 方向专属脚注。
 *
 * 走势仍然各自归一化（上下行量级常差 10 倍，共享纵轴会把上传压平），
 * `baselineZero` 让空闲时的几十 KB/s 抖动不被放大成满屏尖峰。
 *
 * 峰值只显示数值不显示时间：`peakAt` 是双方向共用的单一时戳，逐方向标
 * 会误读成「该方向在该时刻达到峰值」；时间只在网络 tab 的会话峰值里出现。
 */
export function NetDirectionReading({ readout }: { readout: NetDirectionReadout }) {
  const { t } = useI18n();
  const ok = readout.status === "ok";
  const down = readout.dir === "down";
  const arrow = down ? "↓" : "↑";
  const toneText = down ? "text-accent" : "text-info";

  if (!ok) {
    return (
      <p className="text-[11px] leading-relaxed text-ink-faint">
        {readout.note ?? "—"}
        {down ? (
          <span className="block">{t("stats.net.unreadableNote")}</span>
        ) : null}
      </p>
    );
  }

  const tooltip = [
    readout.peakText ? t("stats.net.peakTooltip", { arrow, value: readout.peakText }) : null,
    readout.sessionText ? t("stats.net.sessionTooltip", { value: readout.sessionText }) : null,
  ]
    .filter(Boolean)
    .join(" · ");

  return (
    <div title={tooltip}>
      <div className="flex items-baseline gap-1.5">
        <span className={`text-[13px] leading-none ${toneText}`}>{arrow}</span>
        <span className={`font-mono text-[24px] leading-none tabular-nums ${toneText}`}>
          {readout.main}
        </span>
        <span className="text-[11px] text-ink-faint">{readout.unit}</span>
      </div>
      <div className="mt-1 h-[26px]">
        {readout.series.length >= 2 ? (
          <Sparkline
            values={readout.series}
            height={26}
            tone={down ? "accent" : "info"}
            baselineZero
          />
        ) : (
          <div className="flex h-[26px] items-center text-[9.5px] text-ink-faint/70">
            {t("stats.net.trendWarmup")}
          </div>
        )}
      </div>
      <div className="mt-1 truncate text-[10px] leading-none text-ink-faint" dir="ltr">
        {readout.todayText != null
          ? t("stats.net.m5Today", { value: readout.avgText, today: readout.todayText })
          : t("stats.net.m5", { value: readout.avgText })}
      </div>
    </div>
  );
}
