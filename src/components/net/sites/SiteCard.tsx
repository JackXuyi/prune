import { useI18n } from "../../../i18n/store";
import { formatBytes } from "../../../lib/format";
import {
  type Site,
  type SiteProbe,
  formatRate,
  siteDelta,
} from "../../../types/net";
import { ChevronRightIcon } from "../../icons";
import {
  failureText,
  hasReadings,
  stageLabel,
  statusDot,
  statusText,
  statusTextClass,
} from "./shared";

/**
 * 一站一张卡：状态点 + 名称 + 状态 chip，第二行三段读数，第三行速率条。
 * 整卡是一个 button（键盘 Tab 可达、Enter 可开），内部只用 span，不塞 div。
 *
 * 卡片只放三个数（TCP / TLS / TTFB）与速率条——五段读数和本段/累计的区分
 * 属于详情弹窗，塞进卡片会让十张卡都变成表格。
 */
export function SiteCard({
  site,
  probe,
  maxRate,
  busy,
  onOpen,
}: {
  site: Site;
  probe: SiteProbe | null;
  maxRate: number;
  busy: boolean;
  onOpen: () => void;
}) {
  const { t } = useI18n();
  const withReadings = probe != null && hasReadings(probe);

  return (
    <button
      type="button"
      onClick={onOpen}
      aria-label={t("net.sites.cardAria", { name: site.name })}
      className="group flex min-w-0 flex-col gap-1.5 rounded-card border border-hairline-soft bg-chip-soft px-2.5 py-2 text-left transition-colors hover:border-hairline hover:bg-chip-hover"
    >
      <span className="flex min-w-0 items-center gap-1.5">
        <span
          className={`size-[6px] shrink-0 rounded-full ${probe ? statusDot(probe.verdict) : "bg-ink-faint"} ${
            busy ? "animate-pulse" : ""
          }`}
        />
        <span className="min-w-0 flex-1 truncate text-[12px] leading-tight text-ink">
          {site.name}
        </span>
        {probe ? <VerdictChip probe={probe} /> : null}
        <ChevronRightIcon
          size={10}
          className="shrink-0 text-ink-faint opacity-0 transition-opacity group-hover:opacity-100"
        />
      </span>

      {withReadings && probe ? (
        <>
          <span className="flex items-baseline gap-2.5 font-mono text-[10px] tabular-nums">
            <Reading label="TCP" ms={probe.tcpMs} />
            <Reading label="TLS" ms={siteDelta(probe.tlsMs, probe.tcpMs)} />
            <Reading label="TTFB" ms={siteDelta(probe.ttfbMs, probe.tlsMs ?? probe.tcpMs)} />
          </span>
          <span className="flex items-center gap-1.5">
            <RateBar
              bps={probe.transferBps}
              maxRate={maxRate}
              bytes={probe.bytes}
              totalMs={probe.totalMs}
              ttfbMs={probe.ttfbMs}
            />
          </span>
        </>
      ) : (
        <span
          title={probe ? failureText(probe, t) : undefined}
          className={`min-w-0 truncate text-[10.5px] leading-[16px] ${
            probe ? statusTextClass(probe.verdict) : "text-ink-dim"
          }`}
        >
          {probe
            ? statusText(probe, t)
            : busy
              ? t("net.sites.probing")
              : t("net.sites.pending")}
        </span>
      )}
    </button>
  );
}

/** 单段读数：null 显「—」（没走到/没读到 ≠ 0 ms） */
function Reading({ label, ms }: { label: string; ms: number | null }) {
  return (
    <span className="shrink-0 whitespace-nowrap">
      <span className="text-ink-faint">{label}</span>{" "}
      <span className="text-ink">{ms == null ? "—" : `${ms.toFixed(0)}ms`}</span>
    </span>
  );
}

/** 速率条 + 数字。样本不足时**不画条、不给数字**，只报体积与时长 */
function RateBar({
  bps,
  maxRate,
  bytes,
  totalMs,
  ttfbMs,
}: {
  bps: number | null;
  maxRate: number;
  bytes: number;
  totalMs: number | null;
  ttfbMs: number | null;
}) {
  const { t } = useI18n();
  if (bps == null) {
    const transfer = totalMs != null && ttfbMs != null ? totalMs - ttfbMs : null;
    return (
      <span className="min-w-0 truncate text-[10px] text-ink-faint">
        {t("net.sites.rateSampleShort", {
          bytes: formatBytes(bytes),
          ms: transfer == null ? "—" : transfer.toFixed(0),
        })}
      </span>
    );
  }
  const pct = maxRate > 0 ? Math.max(4, Math.round((bps / maxRate) * 100)) : 0;
  return (
    <span className="flex min-w-0 flex-1 items-center gap-1.5">
      <span className="h-[4px] min-w-0 flex-1 overflow-hidden rounded-full bg-chip">
        <span className="block h-full rounded-full bg-info" style={{ width: `${pct}%` }} />
      </span>
      <span className="shrink-0 whitespace-nowrap font-mono text-[10px] tabular-nums text-ink">
        {formatRate(bps)}
      </span>
    </span>
  );
}

/** 读数齐全的卡也要说清状态：小圆点单靠自己表达不了「403」或「TLS 慢」 */
function VerdictChip({ probe }: { probe: SiteProbe }) {
  const { t } = useI18n();
  if (probe.verdict === "slow") {
    return (
      <span className="shrink-0 whitespace-nowrap rounded-full bg-warn/[0.12] px-1.5 text-[9.5px] text-warn">
        {t("net.sites.slowChip", { stage: stageLabel(probe.stage, t) })}
      </span>
    );
  }
  if (probe.verdict === "refused") {
    return (
      <span
        title={t("net.sites.row.refused", { code: probe.httpCode ?? "—" })}
        className="shrink-0 whitespace-nowrap rounded-full bg-warn/[0.12] px-1.5 text-[9.5px] text-warn"
      >
        {t("net.sites.refusedChip", { code: probe.httpCode ?? "—" })}
      </span>
    );
  }
  return null;
}
