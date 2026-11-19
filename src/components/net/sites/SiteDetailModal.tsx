import { useEffect } from "react";

import { useDialogA11y } from "../../../lib/dialogA11y";
import { formatBytes, formatClock } from "../../../lib/format";
import { useI18n } from "../../../i18n/store";
import { type Site, type SiteProbe, formatRate, siteDelta, siteTone } from "../../../types/net";
import {
  failureText,
  hasAnyStage,
  stageLabel,
  statusDot,
} from "./shared";

/**
 * 详情弹窗：回答卡片回答不了的问题——「时间花在哪一段」。
 *
 * 上半是五段耗时瀑布（按累计时刻切分，没走到的那一截画成「等待」），
 * 下半是读数字段。**本段 / 累计两列都给**，因为阈值口径不同：
 * TCP 判的是累计 300 ms，TLS 与首字节判的是本段 400 ms / 1 s——
 * 只给一列就必然要在这里写一句「其实这个数指的是…」。
 */
export function SiteDetailModal({
  site,
  probe,
  busy,
  onRerun,
  onClose,
}: {
  site: Site;
  probe: SiteProbe | null;
  busy: boolean;
  onRerun: () => void;
  onClose: () => void;
}) {
  const { t } = useI18n();
  const dialogRef = useDialogA11y<HTMLDivElement>(true);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <div className="fixed inset-0 z-[70] flex items-center justify-center p-6">
      <div
        className="prune-fade-in absolute inset-0 bg-black/45 backdrop-blur-[2px]"
        onClick={onClose}
      />
      <div
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-label={t("net.sites.detailTitle", { name: site.name })}
        className="relative flex max-h-[86vh] w-[420px] max-w-full flex-col overflow-hidden rounded-[14px] border border-hairline bg-surface-raised shadow-2xl shadow-black/50"
      >
        {/* 头：名称 + 地址 + 状态徽标 */}
        <div className="flex items-start gap-2 px-4 pt-3.5 pb-2.5">
          <div className="min-w-0 flex-1">
            <div className="flex min-w-0 items-center gap-1.5">
              <span
                className={`size-[7px] shrink-0 rounded-full ${probe ? statusDot(probe.verdict) : "bg-ink-faint"} ${
                  busy ? "animate-pulse" : ""
                }`}
              />
              <span className="truncate text-[13.5px] font-medium text-ink">{site.name}</span>
              {probe ? <VerdictBadge probe={probe} /> : null}
            </div>
            <p className="mt-0.5 truncate font-mono text-[10px] text-ink-faint" title={site.url}>
              {site.url}
            </p>
          </div>
          <button
            type="button"
            onClick={onClose}
            title={t("common.action.close")}
            aria-label={t("common.action.close")}
            className="grid size-6 shrink-0 place-items-center rounded-ctl text-ink-faint transition-colors hover:bg-chip hover:text-ink"
          >
            ✕
          </button>
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto border-t border-hairline-soft px-4 py-3">
          {probe ? (
            <>
              {probe.verdict !== "ok" ? (
                <p
                  className={`text-[11px] leading-relaxed ${
                    siteTone(probe.verdict) === "danger" ? "text-danger" : "text-warn"
                  }`}
                >
                  {failureText(probe, t)}
                </p>
              ) : null}

              {/* 一个阶段读数都没拿到（error）：画瀑布和读数表只会是一排「—」，
                  这时候唯一的有效信息就是下面那段原文 */}
              {hasAnyStage(probe) ? (
                <>
                  <p className="mt-2 text-[9.5px] text-ink-faint">{t("net.sites.waterfall")}</p>
                  <StageWaterfall probe={probe} />

                  <div className="mt-2.5 grid grid-cols-[54px_1fr_1fr] gap-x-2 gap-y-1 font-mono text-[10px] tabular-nums">
                    <span className="text-ink-faint">{t("net.sites.colStage")}</span>
                    <span className="text-right text-ink-faint">{t("net.sites.colSeg")}</span>
                    <span className="text-right text-ink-faint">{t("net.sites.colCum")}</span>
                    <StageRows probe={probe} />
                  </div>

                  {probe.bytes > 0 ? (
                    <div className="mt-2.5 flex items-baseline gap-1.5 border-t border-hairline-soft pt-2 text-[10.5px]">
                      <span className="text-ink-faint">{t("net.sites.rateLabel")}</span>
                      {probe.transferBps != null ? (
                        <>
                          <span className="font-mono tabular-nums text-ink">
                            {formatRate(probe.transferBps)}
                          </span>
                          <span className="text-ink-faint">
                            {t("net.sites.rateBasis", {
                              bytes: formatBytes(probe.bytes),
                              ms: String(
                                Math.max(0, Math.round((probe.totalMs ?? 0) - (probe.ttfbMs ?? 0))),
                              ),
                            })}
                          </span>
                        </>
                      ) : (
                        <span className="text-ink-faint">
                          {t("net.sites.rateSampleShort", {
                            bytes: formatBytes(probe.bytes),
                            ms:
                              probe.totalMs != null && probe.ttfbMs != null
                                ? (probe.totalMs - probe.ttfbMs).toFixed(0)
                                : "—",
                          })}
                        </span>
                      )}
                    </div>
                  ) : null}
                  {probe.curlExit === 28 && probe.bytes > 0 ? (
                    <p className="mt-1 text-[9.5px] leading-relaxed text-ink-faint">
                      {t("net.sites.truncated")}
                    </p>
                  ) : null}

                  <div className="mt-3 grid grid-cols-2 gap-x-3 gap-y-1 border-t border-hairline-soft pt-2 font-mono text-[10px] tabular-nums">
                    <Meta label={t("net.sites.metaHttp")} value={probe.httpCode == null ? "—" : String(probe.httpCode)} />
                    <Meta label={t("net.sites.metaBytes")} value={formatBytes(probe.bytes)} />
                    <Meta label={t("net.sites.metaIp")} value={probe.remoteIp ?? "—"} />
                    <Meta label={t("net.sites.metaExit")} value={probe.curlExit == null ? "—" : String(probe.curlExit)} />
                  </div>
                </>
              ) : null}

              {probe.note && hasAnyStage(probe) ? (
                <div className="mt-2.5">
                  <p className="text-[9.5px] text-ink-faint">{t("net.sites.detailNote")}</p>
                  <p className="mt-0.5 break-all rounded-md bg-chip px-2 py-1.5 font-mono text-[9.5px] leading-relaxed text-ink-dim">
                    {probe.note}
                  </p>
                </div>
              ) : null}
            </>
          ) : (
            <p className="py-3 text-[11px] leading-relaxed text-ink-dim">
              {busy ? t("net.sites.probing") : t("net.sites.noProbe")}
            </p>
          )}
        </div>

        {/* 脚：检测时刻 + 重测 */}
        <div className="flex items-center justify-between gap-3 border-t border-hairline-soft px-4 py-2.5">
          <span className="min-w-0 truncate text-[10px] text-ink-faint">
            {probe ? t("net.sites.probedAt", { time: formatClock(probe.at) }) : ""}
          </span>
          <div className="flex shrink-0 items-center gap-2">
            <button
              type="button"
              onClick={onRerun}
              disabled={busy}
              className="rounded-md border border-hairline-soft px-2.5 py-1 text-[10.5px] text-ink-dim transition-colors hover:bg-chip disabled:opacity-50"
            >
              {busy ? t("net.sites.probing") : t("net.sites.rerun")}
            </button>
            <button
              type="button"
              onClick={onClose}
              className="rounded-md border border-hairline-soft px-2.5 py-1 text-[10.5px] text-ink-dim transition-colors hover:bg-chip"
            >
              {t("common.action.close")}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}

function VerdictBadge({ probe }: { probe: SiteProbe }) {
  const { t } = useI18n();
  const label =
    probe.verdict === "ok"
      ? t("net.sites.summary.ok")
      : probe.verdict === "slow"
        ? t("net.sites.slowChip", { stage: stageLabel(probe.stage, t) })
        : probe.verdict === "refused"
          ? t("net.sites.refusedChip", { code: probe.httpCode ?? "—" })
          : probe.verdict === "stalled"
            ? t("net.sites.summary.stalled")
            : probe.verdict === "unreachable"
              ? t("net.sites.unreachableChip", { stage: stageLabel(probe.stage, t) })
              : t("net.sites.summary.error");
  const tone = siteTone(probe.verdict);
  const cls =
    tone === "accent"
      ? "bg-accent/[0.12] text-accent"
      : tone === "warn"
        ? "bg-warn/[0.12] text-warn"
        : tone === "danger"
          ? "bg-danger/[0.12] text-danger"
          : "bg-chip text-ink-dim";
  return (
    <span className={`shrink-0 whitespace-nowrap rounded-full px-1.5 py-px text-[9.5px] ${cls}`}>
      {label}
    </span>
  );
}

type SegKey = "dns" | "tcp" | "tls" | "ttfb" | "transfer";

/** 五段：每段 = 后一个累计时刻 − 前一个；缺任一端就说明链路没走到这里 */
function stageSegments(p: SiteProbe): { key: SegKey; ms: number }[] {
  const out: { key: SegKey; ms: number }[] = [];
  const push = (key: SegKey, end: number | null, start: number | null) => {
    if (end == null || start == null) return;
    if (end > start) out.push({ key, ms: end - start });
  };
  push("dns", p.dnsMs, 0);
  push("tcp", p.tcpMs, p.dnsMs);
  push("tls", p.tlsMs, p.tcpMs);
  push("ttfb", p.ttfbMs, p.tlsMs ?? p.tcpMs);
  push("transfer", p.totalMs, p.ttfbMs);
  return out;
}

/**
 * 耗时瀑布：把整轮实际耗时按阶段切开。
 *
 * 两件事必须看见：
 * 1. 时间花在哪一类事上——握手（DNS/TCP/TLS）/ 等服务端首字节 / 收数据，三类分色；
 * 2. 断点那一段（`stage`）整体提亮成 warn/danger。
 *
 * 未读数的那一截画成「等待」段（超时是待在 TCP/TLS/首字节里）——
 * 只画已知段会让 4 s 超时的站看起来只花了 16 ms，那是撒谎。
 */
function StageWaterfall({ probe }: { probe: SiteProbe }) {
  const { t } = useI18n();
  const segs = stageSegments(probe);
  const known = segs.reduce((n, s) => n + s.ms, 0);
  const span = Math.max(probe.totalMs ?? known, known);
  const wait = span - known;
  const waitTone =
    probe.verdict === "slow"
      ? "bg-warn/30"
      : probe.verdict === "unreachable" || probe.verdict === "stalled"
        ? "bg-danger/30"
        : "bg-ink-faint/20";
  const kinds = new Set(segs.map((s) => segKind(s.key)));

  return (
    <div className="mt-1">
      <div className="flex h-[8px] w-full gap-px overflow-hidden rounded-full bg-chip">
        {segs.map((s) => (
          <span
            key={s.key}
            title={`${stageLabel(s.key, t)} ${s.ms.toFixed(0)} ms`}
            style={{ flexGrow: Math.max(s.ms, span * 0.01), flexBasis: 0 }}
            className={`h-full ${segTone(s.key, probe)}`}
          />
        ))}
        {wait > 0 ? (
          <span
            title={t("net.sites.waitSegment", {
              stage: stageLabel(probe.stage, t),
              ms: wait.toFixed(0),
            })}
            style={{ flexGrow: Math.max(wait, span * 0.01), flexBasis: 0 }}
            className={`h-full ${waitTone}`}
          />
        ) : null}
      </div>
      <div className="mt-1 flex flex-wrap items-center gap-x-2.5 gap-y-0.5 text-[9px] text-ink-faint">
        <LegendItem show={kinds.has("handshake")} swatch="bg-ink-faint/60" label={t("net.sites.lgHandshake")} />
        <LegendItem show={kinds.has("ttfb")} swatch="bg-warn/40" label={t("net.sites.lgTtfb")} />
        <LegendItem show={kinds.has("transfer")} swatch="bg-info/45" label={t("net.sites.lgTransfer")} />
        <LegendItem show={wait > 0} swatch={waitTone} label={t("net.sites.lgWait")} />
      </div>
    </div>
  );
}

type SegKind = "handshake" | "ttfb" | "transfer";

function segKind(key: SegKey): SegKind {
  if (key === "ttfb") return "ttfb";
  if (key === "transfer") return "transfer";
  return "handshake";
}

/** 先看「断在哪」，再按三段语义分色——同一段既是断点又是正常段时，断点说了算 */
function segTone(key: SegKey, probe: SiteProbe): string {
  if (probe.verdict !== "ok" && probe.stage === key) {
    return probe.verdict === "slow" ? "bg-warn" : "bg-danger";
  }
  switch (segKind(key)) {
    case "ttfb":
      return "bg-warn/40";
    case "transfer":
      return "bg-info/45";
    default:
      return "bg-ink-faint/60";
  }
}

function LegendItem({ show, swatch, label }: { show: boolean; swatch: string; label: string }) {
  if (!show) return null;
  return (
    <span className="flex items-center gap-1">
      <span className={`h-[6px] w-[6px] shrink-0 rounded-[2px] ${swatch}`} />
      {label}
    </span>
  );
}

/** 本段 / 累计两列：阈值口径不同（TCP 看累计、TLS 与首字节看本段），两列都给才不会误读 */
function StageRows({ probe }: { probe: SiteProbe }) {
  const { t } = useI18n();
  const rows: { key: SegKey; cum: number | null; seg: number | null }[] = [
    { key: "dns", cum: probe.dnsMs, seg: probe.dnsMs },
    { key: "tcp", cum: probe.tcpMs, seg: siteDelta(probe.tcpMs, probe.dnsMs) },
    { key: "tls", cum: probe.tlsMs, seg: siteDelta(probe.tlsMs, probe.tcpMs) },
    { key: "ttfb", cum: probe.ttfbMs, seg: siteDelta(probe.ttfbMs, probe.tlsMs ?? probe.tcpMs) },
    { key: "transfer", cum: probe.totalMs, seg: siteDelta(probe.totalMs, probe.ttfbMs) },
  ];
  return (
    <>
      {rows.map((r) => {
        const hit = probe.stage === r.key && probe.verdict !== "ok";
        return (
          <div key={r.key} className="col-span-3 grid grid-cols-[54px_1fr_1fr] gap-x-2">
            <span
              className={
                hit
                  ? probe.verdict === "slow"
                    ? "font-sans text-warn"
                    : "font-sans text-danger"
                  : "font-sans text-ink-dim"
              }
            >
              {stageLabel(r.key, t)}
            </span>
            <span className={`text-right ${hit ? "text-ink" : "text-ink-dim"}`}>
              {r.seg == null ? "—" : `${r.seg.toFixed(0)} ms`}
            </span>
            <span className="text-right text-ink-dim">
              {r.cum == null ? "—" : `${r.cum.toFixed(0)} ms`}
            </span>
          </div>
        );
      })}
    </>
  );
}

function Meta({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex min-w-0 items-baseline gap-1.5">
      <span className="shrink-0 text-ink-faint">{label}</span>
      <span className="truncate text-ink-dim" title={value}>
        {value}
      </span>
    </div>
  );
}
