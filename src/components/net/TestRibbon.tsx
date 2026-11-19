import { useMemo } from "react";

import { useI18n } from "../../i18n/store";
import { formatClock } from "../../lib/format";
import { useNetDiagStore } from "../../stores/netDiagStore";
import { useSiteStore } from "../../stores/siteStore";
import { DIAG_THRESHOLDS, rpmLevelLabel, summarizeSites } from "../../types/net";
import { Badge } from "../primitives";
import { ActivityIcon, GlobeIcon, RouterIcon, WifiIcon } from "../icons";
import { RibbonChip, RibbonSeg, RibbonShell, RibbonValue } from "../Ribbon";
import { rpmTone } from "./bits";

/**
 * 测试页速读条（4 段）：链路结论 / RPM / 站点汇总 / 网关。
 *
 * 全部是「数值 + 状态」不是占比 → 图标位一律 RibbonChip，不套环；
 * 每段的完整口径（阈值、样本口径、上次时间）进 title，段内只留读数。
 * 数据全部来自 store，这里不拉 IPC。
 */
export function TestRibbon() {
  const { t, locale } = useI18n();
  const diag = useNetDiagStore((s) => s.diag);
  const speed = useNetDiagStore((s) => s.speed);
  const sites = useSiteStore((s) => s.sites);
  const probes = useSiteStore((s) => s.probes);
  const ranAt = useSiteStore((s) => s.ranAt);

  const summary = useMemo(
    () => summarizeSites(sites.map((s) => probes[s.id]).filter(Boolean)),
    [sites, probes],
  );

  const done = diag?.status === "done" ? diag : null;
  const failed = diag?.status === "failed";

  // ---- 段 1：链路结论（verdict） ----
  const wanSlow =
    done?.wan?.medianMs != null && done.wan.medianMs > DIAG_THRESHOLDS.wanLanGapMs;
  const verdictValue = diag
    ? diag.status === "failed"
      ? t("net.test.verdictUnavailable")
      : diag.status === "running"
        ? t("net.test.verdictRunning")
        : diag.status === "idle"
          ? "—"
          : (diag.verdict?.label ?? "—")
    : "—";
  const verdictTone = failed
    ? "danger"
    : diag?.status === "done" && wanSlow
      ? "warn"
      : undefined;
  const verdictColor = failed
    ? "var(--color-danger)"
    : diag?.status === "done" && wanSlow
      ? "var(--color-warn)"
      : diag
        ? "var(--color-accent)"
        : "var(--color-ink-faint)";

  // ---- 段 2：RPM ----
  const rpmToneCls = speed ? rpmTone(speed.rpmLevel) : "neutral";

  return (
    <RibbonShell ariaLabel={t("net.test.ribbonAria")}>
      <RibbonSeg title={t("net.test.verdictTip")}>
        <RibbonChip color={verdictColor} icon={<WifiIcon size={13} />} />
        {/* verdict 词可能很长（如「出口/运营商延迟高（差值 207 ms)」），截断 + tooltip 兜底 */}
        <span className="block min-w-0 max-w-[180px] truncate">
          <RibbonValue value={verdictValue} tone={verdictTone} />
        </span>
      </RibbonSeg>

      <RibbonSeg
        title={
          speed
            ? t("net.test.rpmTip", { rtt: speed.baseRttMs.toFixed(0) })
            : t("net.speed.idle")
        }
      >
        <RibbonChip color="var(--color-accent)" icon={<ActivityIcon size={13} />} />
        <RibbonValue value={speed ? speed.rpm.toFixed(0) : "—"} />
        {speed ? (
          <Badge tone={rpmToneCls} title={t("net.speed.rpmTitle")}>
            {rpmLevelLabel(speed.rpmLevel, locale)}
          </Badge>
        ) : null}
      </RibbonSeg>

      <RibbonSeg
        title={t("net.test.sitesTip", {
          n: sites.length,
          ok: summary.ok,
          time: ranAt != null ? formatClock(ranAt) : "—",
        })}
      >
        <RibbonChip color="var(--color-info)" icon={<GlobeIcon size={13} />} />
        <RibbonValue
          value={summary.total > 0 ? String(summary.ok) : "—"}
          unit={t("net.test.sitesUnit", { n: sites.length })}
        />
      </RibbonSeg>

      <RibbonSeg title={t("net.test.gatewayTip")}>
        <RibbonChip color="var(--color-domain-cpu)" icon={<RouterIcon size={13} />} />
        <RibbonValue value={diag?.gateway ?? "—"} />
      </RibbonSeg>
    </RibbonShell>
  );
}
