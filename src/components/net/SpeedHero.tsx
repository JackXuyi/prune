import { useI18n } from "../../i18n/store";
import { formatClock } from "../../lib/format";
import { useNetDiagStore } from "../../stores/netDiagStore";
import { rpmLevelLabel } from "../../types/net";
import { Badge } from "../primitives";
import { GhostButton, GIGABIT_BPS, rpmTone, splitMbps } from "./bits";

/**
 * 测速 Hero（128px）：↓ 大读数 + ↑ 次读数 + 千兆参考构成条；右侧 RPM 数值块 + 开始按钮。
 *
 * 纪律（设计稿定稿）：
 * - RPM / 速率是数值不是占比 → 不套环，只用 mono 读数 + 分级徽标；
 * - running 态不显示中间值：只有扫光进度 + 提示文案，完成一次性给 ↓/↑/RPM；
 * - 没跑过显示「—」+ 指路，不显示 0 Mbps；
 * - 构成条参考 = 1 Gbps 扣协议开销的实测可用值，条满只说明「不低于参考」。
 */
export function SpeedHero() {
  const { t, locale } = useI18n();
  const capability = useNetDiagStore((s) => s.capability);
  const speed = useNetDiagStore((s) => s.speed);
  const speedRunning = useNetDiagStore((s) => s.speedRunning);
  const runSpeed = useNetDiagStore((s) => s.runSpeed);

  // Windows 无系统级等价物 → 整卡不渲染（W2 先例：隐藏优于「不可用」占位）
  if (!capability?.speedTest) return null;

  const down = speed ? splitMbps(speed.downBps) : null;
  const up = speed ? splitMbps(speed.upBps) : null;
  // 构成条宽度：各自相对千兆参考，>100% 封顶（条满 = 不低于参考，不是上限）
  const downPct = speed ? Math.min(100, (speed.downBps / GIGABIT_BPS) * 100) : 0;
  const upPct = speed ? Math.min(100, (speed.upBps / GIGABIT_BPS) * 100) : 0;

  return (
    <section
      className="grid h-[128px] shrink-0 grid-cols-[minmax(0,1fr)_232px] items-stretch gap-5 rounded-[16px] border border-domain-net/25 bg-gradient-to-br from-domain-net/[0.09] to-transparent bg-surface-raised px-[18px] py-3"
      title={t("net.speed.gigabit")}
    >
      {/* 左：主读数 + 构成条 + 图例 */}
      <div className="min-w-0">
        <div className="flex min-h-[30px] items-baseline gap-2.5">
          {speedRunning ? (
            <span className="flex items-center gap-2 text-[13px] text-ink-dim">
              <span className="relative inline-block h-1.5 w-[60px] overflow-hidden rounded-full bg-chip">
                <span className="prune-sweep absolute inset-0 w-2/5" />
              </span>
              {t("net.test.heroRunning")}
            </span>
          ) : speed && down && up ? (
            <>
              <b className="font-mono text-[27px] leading-none font-semibold tabular-nums text-domain-net">
                {down.value}
                <small className="ml-0.5 text-[10px] font-medium text-ink-faint">{down.unit}</small>
              </b>
              <span className="font-mono text-[15px] leading-none font-semibold tabular-nums text-domain-cpu">
                ↑ {up.value}
                <small className="ml-0.5 text-[10px] font-medium text-ink-faint">{up.unit}</small>
              </span>
              <span className="min-w-0 truncate text-[10px] text-ink-faint">
                {t("net.test.heroSource", { time: formatClock(speed.at) })}
              </span>
            </>
          ) : (
            <>
              <b className="font-mono text-[27px] leading-none font-semibold text-ink-faint">—</b>
              <span className="text-[10px] text-ink-faint">{t("net.test.heroIdle")}</span>
            </>
          )}
        </div>

        {/* 构成条：下行（青）+ 上行（紫）堆在同一条里，宽度 = 相对千兆参考 */}
        <div className="mt-3 flex h-2.5 gap-0.5 overflow-hidden rounded-full bg-chip">
          {speed && !speedRunning ? (
            <>
              <span
                className="block h-full rounded-r-full bg-gradient-to-r from-domain-net to-domain-net/60"
                style={{ width: `${downPct}%` }}
                title={t("net.test.stackDownTip", { speed: down?.value ?? "—" })}
              />
              <span
                className="block h-full rounded-r-full bg-gradient-to-r from-domain-cpu to-domain-cpu/55"
                style={{ width: `${upPct}%` }}
                title={t("net.test.stackUpTip", { speed: up?.value ?? "—" })}
              />
            </>
          ) : null}
        </div>

        <div className="mt-2 flex flex-wrap gap-x-3 gap-y-1">
          {speedRunning ? (
            <span className="text-[9.5px] text-ink-faint">{t("net.test.heroNoIntermediate")}</span>
          ) : speed && down && up ? (
            <>
              <LegendItem color="var(--color-domain-net)" label={t("net.down")} value={`${down.value} ${down.unit}`} />
              <LegendItem color="var(--color-domain-cpu)" label={t("net.up")} value={`${up.value} ${up.unit}`} />
              <span className="text-[9.5px] text-ink-faint">{t("net.test.heroRef")}</span>
            </>
          ) : (
            <span className="text-[9.5px] text-ink-faint">{t("net.test.heroIdleLegend")}</span>
          )}
        </div>
      </div>

      {/* 右：RPM 数值块 + 开始按钮 */}
      <div className="flex min-w-0 flex-col justify-center gap-1 border-l border-hairline-soft pl-[18px]">
        <span className="flex items-center gap-1.5 text-[10px] text-ink-faint">
          {t("net.speed.rpm")}
          {speed ? (
            <span className="text-ink-faint/80">· {t("net.speed.baseRtt", { n: speed.baseRttMs.toFixed(0) })}</span>
          ) : null}
        </span>
        <span className="flex min-h-[24px] items-baseline gap-2">
          <b
            className={`font-mono text-[24px] leading-none font-semibold tabular-nums ${
              speed ? "text-ink" : "text-ink-faint"
            }`}
          >
            {speed && !speedRunning ? speed.rpm.toFixed(0) : "—"}
          </b>
          {speed && !speedRunning ? (
            <Badge tone={rpmTone(speed.rpmLevel)} title={t("net.speed.rpmTitle")}>
              {rpmLevelLabel(speed.rpmLevel, locale)}
            </Badge>
          ) : null}
        </span>
        <span
          className="text-[9.5px] leading-snug text-ink-faint"
          title={t("net.speed.rpmNote")}
        >
          {t("net.test.rpmSub")}
        </span>
        <GhostButton
          variant="primary"
          onClick={() => void runSpeed()}
          disabled={speedRunning}
          title={t("net.test.heroRunTip")}
        >
          {speedRunning ? t("net.test.heroRunningBtn") : t("net.speed.start")}
        </GhostButton>
      </div>
    </section>
  );
}

function LegendItem({ color, label, value }: { color: string; label: string; value: string }) {
  return (
    <span className="flex items-center gap-1.5 text-[9.5px] text-ink-faint">
      <span className="size-[7px] shrink-0 rounded-[2px]" style={{ background: color }} aria-hidden />
      {label}
      <b className="font-mono text-[10px] font-medium tabular-nums text-ink-dim">{value}</b>
    </span>
  );
}
