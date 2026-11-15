import { useEffect, useMemo, useState } from "react";

import { ChevronRightIcon } from "./icons";
import { useDialogA11y } from "../lib/dialogA11y";
import { formatBytes, formatClock } from "../lib/format";
import { useI18n } from "../i18n/store";
import { useSiteStore } from "../stores/siteStore";
import {
  SITE_THRESHOLDS,
  formatRate,
  siteDelta,
  siteTone,
  summarizeSites,
  type FailStage,
  type Site,
  type SiteProbe,
} from "../types/net";

/**
 * 站点检测页（F15，docs/net-sites-plan.md §9）。
 *
 * 三条纪律（§12）决定这里的每一处措辞：
 * - 断点分开显示：DNS / TCP / TLS / 首字节 / 传输，`stage` 决定文案，不写「被墙」；
 * - `refused`（403/429）是**可达**，只标黄不标红；
 * - 速率为 null 时显示「样本不足（体积 / 时长）」，**不显示 0 KB/s**。
 *
 * 布局：卡片网格给「一眼看十站」，点卡片开详情弹窗看「时间花在哪一段」。
 * 卡片只放三个数（TCP / TLS / TTFB）与速率条——五段读数和本段/累计的区分
 * 属于详情，塞进卡片会让 10 张卡都变成表格。
 */
export function SitesPage() {
  const { t } = useI18n();
  const sites = useSiteStore((s) => s.sites);
  const probes = useSiteStore((s) => s.probes);
  const running = useSiteStore((s) => s.running);
  const rerunning = useSiteStore((s) => s.rerunning);
  const done = useSiteStore((s) => s.done);
  const total = useSiteStore((s) => s.total);
  const cancelled = useSiteStore((s) => s.cancelled);
  const ranAt = useSiteStore((s) => s.ranAt);
  const elapsedMs = useSiteStore((s) => s.elapsedMs);
  const copied = useSiteStore((s) => s.copied);
  const copyFailed = useSiteStore((s) => s.copyFailed);
  const resetCopied = useSiteStore((s) => s.resetCopied);
  const capability = useSiteStore((s) => s.capability);
  const recovered = useSiteStore((s) => s.recovered);
  const listError = useSiteStore((s) => s.listError);
  const runAll = useSiteStore((s) => s.runAll);
  const cancel = useSiteStore((s) => s.cancel);
  const rerun = useSiteStore((s) => s.rerun);
  const save = useSiteStore((s) => s.save);
  const reset = useSiteStore((s) => s.reset);
  const copyReport = useSiteStore((s) => s.copyReport);
  const init = useSiteStore((s) => s.init);

  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState<Site[]>([]);
  /** 打开详情的站 id（不存整条 probe：让它跟着本轮结果实时更新） */
  const [detailId, setDetailId] = useState<string | null>(null);

  // 首次进入本页才拉站表 / 订阅进度事件（与诊断页不同：它只拉 capability）
  useEffect(() => {
    void init();
  }, [init]);

  useEffect(() => {
    if (!copied && !copyFailed) return;
    const timer = setTimeout(resetCopied, 1500);
    return () => clearTimeout(timer);
  }, [copied, copyFailed, resetCopied]);

  const rows = useMemo(() => sites.map((s) => probes[s.id]).filter(Boolean), [sites, probes]);
  const summary = useMemo(() => summarizeSites(rows), [rows]);
  const maxRate = useMemo(
    () => Math.max(0, ...rows.map((p) => p.transferBps ?? 0)),
    [rows],
  );

  const detailSite = detailId ? (sites.find((s) => s.id === detailId) ?? null) : null;

  // curl 缺失：整页换成说明，不渲染带 0 的空表（W2 先例：无等价物就隐藏）
  if (capability && !capability.curl) {
    return (
      <section className="mt-4 rounded-card border border-hairline-soft bg-surface-raised px-4 py-3.5">
        <p className="text-[11px] leading-relaxed text-warn">{t("net.sites.unavailable")}</p>
        <p className="mt-1 text-[10.5px] leading-relaxed text-ink-faint">
          {t("net.sites.unavailableHint")}
        </p>
      </section>
    );
  }

  const startEdit = () => {
    setDraft(sites.map((s) => ({ ...s })));
    setEditing(true);
    // 站表可能要删站：先收起详情，避免弹窗指向一条已不存在的站
    setDetailId(null);
  };

  const submitEdit = async () => {
    try {
      await save(draft);
      setEditing(false);
    } catch {
      // 校验失败：错误留在 listError，编辑态保持不动（不让用户丢掉刚填的内容）
    }
  };

  return (
    <>
      {/* 头：口径 + 站数 + 上次 + 主操作 */}
      <div className="mt-4 flex items-center justify-between gap-3">
        <span className="min-w-0 truncate text-[10px] text-ink-faint">
          {t("net.sites.headCount", { n: sites.length })}
          {ranAt != null ? ` · ${t("net.sites.lastRun", { time: formatClock(ranAt) })}` : ""}
        </span>
        <div className="flex shrink-0 items-center gap-2">
          {editing ? (
            <>
              <button
                type="button"
                onClick={() => void submitEdit()}
                className="rounded-md border border-hairline-soft px-2.5 py-1 text-[10.5px] text-accent transition-colors hover:bg-chip"
              >
                {t("net.sites.save")}
              </button>
              <button
                type="button"
                onClick={() => setEditing(false)}
                className="rounded-md border border-hairline-soft px-2.5 py-1 text-[10.5px] text-ink-dim transition-colors hover:bg-chip"
              >
                {t("net.sites.cancelEdit")}
              </button>
            </>
          ) : (
            <>
              {running ? (
                <button
                  type="button"
                  onClick={() => void cancel()}
                  className="rounded-md border border-hairline-soft px-2.5 py-1 text-[10.5px] text-warn transition-colors hover:bg-chip"
                >
                  {t("net.sites.cancel")}
                </button>
              ) : (
                <button
                  type="button"
                  onClick={() => void runAll()}
                  disabled={sites.length === 0}
                  className="rounded-md border border-hairline-soft px-2.5 py-1 text-[10.5px] text-ink-dim transition-colors hover:bg-chip disabled:opacity-50"
                >
                  {t("net.sites.run")}
                </button>
              )}
              <button
                type="button"
                onClick={startEdit}
                className="rounded-md border border-hairline-soft px-2.5 py-1 text-[10.5px] text-ink-dim transition-colors hover:bg-chip"
              >
                {t("net.sites.edit")}
              </button>
            </>
          )}
        </div>
      </div>

      {running ? (
        <p className="mt-2 text-[10.5px] text-ink-faint">
          {t("net.sites.running", { done, total })}
        </p>
      ) : null}
      {cancelled && !running ? (
        <p className="mt-2 text-[10.5px] text-warn">{t("net.sites.cancelledNote")}</p>
      ) : null}
      {recovered ? (
        <p className="mt-2 rounded-md bg-warn/[0.1] px-2.5 py-1.5 text-[10.5px] leading-relaxed text-warn">
          {t("net.sites.recovered")}
        </p>
      ) : null}
      {listError ? (
        <p className="mt-2 rounded-md bg-warn/[0.1] px-2.5 py-1.5 text-[10.5px] leading-relaxed text-warn">
          {t("net.sites.listError", { why: listError })}
        </p>
      ) : null}

      {editing ? (
        <SiteEditor
          draft={draft}
          onChange={setDraft}
          onReset={() => {
            void reset().then(() => setEditing(false));
          }}
        />
      ) : sites.length === 0 ? (
        <p className="mt-3 text-[11px] leading-relaxed text-ink-faint">{t("net.sites.empty")}</p>
      ) : (
        <>
          {summary.total > 0 ? <SummaryBar summary={summary} /> : null}
          <div
            className="mt-2.5 grid gap-2"
            style={{ gridTemplateColumns: "repeat(auto-fill, minmax(238px, 1fr))" }}
          >
            {sites.map((s) => (
              <SiteCard
                key={s.id}
                site={s}
                probe={probes[s.id] ?? null}
                maxRate={maxRate}
                busy={running || Boolean(rerunning[s.id])}
                onOpen={() => setDetailId(s.id)}
              />
            ))}
          </div>
          {elapsedMs != null && !running ? (
            <p className="mt-2 text-[10px] text-ink-faint">
              {t("net.sites.elapsed", { s: (elapsedMs / 1000).toFixed(1) })}
            </p>
          ) : null}
        </>
      )}

      <p className="mt-3 text-[10px] leading-relaxed text-ink-faint">{t("net.sites.scope")}</p>
      <p className="mt-1 text-[10px] leading-relaxed text-ink-faint">
        {t("net.sites.thresholds", {
          tcp: SITE_THRESHOLDS.tcpMs,
          tls: SITE_THRESHOLDS.tlsMs,
          ttfb: SITE_THRESHOLDS.ttfbMs,
          rate: SITE_THRESHOLDS.slowBps / 1000,
        })}
      </p>

      <button
        type="button"
        onClick={() => void copyReport()}
        disabled={summary.total === 0}
        className={`mt-2 text-[10px] transition-colors disabled:opacity-50 ${
          copyFailed ? "text-danger" : copied ? "text-accent" : "text-ink-faint hover:text-ink-dim"
        }`}
        title={t("net.sites.copyTitle")}
      >
        {copyFailed
          ? `✗ ${t("common.action.copyFailed")}`
          : copied
            ? `✓ ${t("common.action.copied")}`
            : t("net.sites.copy")}
      </button>

      {detailSite ? (
        <SiteDetailModal
          site={detailSite}
          probe={probes[detailSite.id] ?? null}
          busy={running || Boolean(rerunning[detailSite.id])}
          onRerun={() => void rerun(detailSite.id)}
          onClose={() => setDetailId(null)}
        />
      ) : null}
    </>
  );
}

// ---------------------------------------------------------------- 汇总条

function SummaryBar({ summary }: { summary: ReturnType<typeof summarizeSites> }) {
  const { t } = useI18n();
  const parts: { key: keyof typeof summary; label: string; tone: string }[] = [
    { key: "ok", label: t("net.sites.summary.ok"), tone: "text-accent" },
    { key: "slow", label: t("net.sites.summary.slow"), tone: "text-warn" },
    { key: "stalled", label: t("net.sites.summary.stalled"), tone: "text-danger" },
    { key: "refused", label: t("net.sites.summary.refused"), tone: "text-warn" },
    { key: "unreachable", label: t("net.sites.summary.unreachable"), tone: "text-danger" },
    { key: "error", label: t("net.sites.summary.error"), tone: "text-ink-dim" },
  ];
  return (
    <div className="mt-3 flex flex-wrap items-center gap-x-4 gap-y-1 border-b border-hairline-soft pb-2">
      {parts
        .filter((p) => summary[p.key] > 0)
        .map((p) => (
          <span key={p.key} className="font-mono text-[10.5px] tabular-nums">
            <span className={p.tone}>{summary[p.key]}</span>
            <span className="ml-1 text-ink-faint">{p.label}</span>
          </span>
        ))}
    </div>
  );
}

// ---------------------------------------------------------------- 卡片

/**
 * 一站一张卡：状态点 + 名称 + 状态 chip，第二行三段读数，第三行速率条。
 * 整卡是一个 button（键盘 Tab 可达、Enter 可开），内部只用 span，不塞 div。
 */
function SiteCard({
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

      {withReadings ? (
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

function statusDot(v: SiteProbe["verdict"]): string {
  const tone = siteTone(v);
  return tone === "accent"
    ? "bg-accent"
    : tone === "warn"
      ? "bg-warn"
      : tone === "danger"
        ? "bg-danger"
        : "bg-ink-faint";
}

function statusTextClass(v: SiteProbe["verdict"]): string {
  const tone = siteTone(v);
  return tone === "accent"
    ? "text-accent"
    : tone === "warn"
      ? "text-warn"
      : tone === "danger"
        ? "text-danger"
        : "text-ink-dim";
}

/**
 * 卡片上只写**状态**，不写细节（2026-11-15 用户要求）。
 *
 * 之前这里放的是 `failureText`：`探测本身出错：write-out has 7 fields…`
 * 这种串在 238 px 的卡里根本放不下，会溢出到隔壁卡片上，十张卡并排时
 * 屏幕上全是 curl 原文，反而看不出「哪几站有问题」。
 * 断点、超时秒数、HTTP 码、curl 原文都进弹窗——那一层才有空间讲清楚。
 */
function statusText(p: SiteProbe, t: ReturnType<typeof useI18n>["t"]): string {
  switch (p.verdict) {
    case "slow":
      return t("net.sites.slowChip", { stage: stageLabel(p.stage, t) });
    case "refused":
      return t("net.sites.refusedChip", { code: p.httpCode ?? "—" });
    case "stalled":
      return t("net.sites.summary.stalled");
    case "unreachable":
      return t("net.sites.unreachableChip", { stage: stageLabel(p.stage, t) });
    case "error":
      return t("net.sites.summary.error");
    default:
      return t("net.sites.summary.ok");
  }
}

/** 有阶段读数 = 画三段读数；unreachable/stalled 只给一句断点说明 */
function hasReadings(p: SiteProbe): boolean {
  return p.ttfbMs != null && p.verdict !== "unreachable";
}

/** 拿没拿到任何阶段读数：`error` 全是 null，瀑布与读数表都无从画起 */
function hasAnyStage(p: SiteProbe): boolean {
  return p.dnsMs != null || p.tcpMs != null;
}

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

/** 阶段名（只有名字，句子的主语在调用方） */
function stageLabel(stage: FailStage | null, t: ReturnType<typeof useI18n>["t"]): string {
  switch (stage) {
    case "dns":
      return t("net.sites.stage.dns");
    case "tcp":
      return t("net.sites.stage.tcp");
    case "tls":
      return t("net.sites.stage.tls");
    case "ttfb":
      return t("net.sites.stage.ttfb");
    case "transfer":
      return t("net.sites.stage.transfer");
    default:
      return t("net.sites.stage.http");
  }
}

/** 失败/拒绝的文案：只描述断点，不推断原因 */
function failureText(
  p: SiteProbe,
  t: ReturnType<typeof useI18n>["t"],
): string {
  const sec = p.totalMs == null ? null : (p.totalMs / 1000).toFixed(1);
  const stage: FailStage | null = p.stage;
  switch (p.verdict) {
    case "stalled":
      return t("net.sites.row.stalled", { sec: sec ?? "—" });
    case "refused":
      return t("net.sites.row.refused", { code: p.httpCode ?? "—" });
    case "error":
      return t("net.sites.row.errorNote", { why: p.note ?? "—" });
    case "unreachable":
      if (stage === "dns") return t("net.sites.row.dnsFail");
      if (stage === "tls") {
        return p.tcpMs == null
          ? t("net.sites.row.tlsFailNoTcp")
          : t("net.sites.row.tlsFail", { tcp: p.tcpMs.toFixed(0) });
      }
      return t("net.sites.row.tcpFail", { sec: sec ?? "—" });
    default:
      return "—";
  }
}

// ---------------------------------------------------------------- 详情弹窗

/**
 * 详情弹窗：回答卡片回答不了的问题——「时间花在哪一段」。
 *
 * 上半是五段耗时瀑布（按累计时刻切分，没走到的那一截画成「等待」），
 * 下半是读数字段。**本段 / 累计两列都给**，因为阈值口径不同：
 * TCP 判的是累计 300 ms，TLS 与首字节判的是本段 400 ms / 1 s——
 * 只给一列就必然要在这里写一句「其实这个数指的是…」。
 */
function SiteDetailModal({
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

// ---------------------------------------------------------------- 编辑态

function SiteEditor({
  draft,
  onChange,
  onReset,
}: {
  draft: Site[];
  onChange: (next: Site[]) => void;
  onReset: () => void;
}) {
  const { t } = useI18n();
  const setAt = (i: number, patch: Partial<Site>) =>
    onChange(draft.map((s, idx) => (idx === i ? { ...s, ...patch } : s)));

  return (
    <div className="mt-3">
      <ul className="space-y-1.5">
        {draft.map((s, i) => (
          // 编辑态按索引做 key：行没有稳定身份（新站的 id 由 Rust 生成），
          // 而输入框都是受控的，删中间一行不会串值
          <li key={i} className="flex items-center gap-2">
            <input
              value={s.name}
              onChange={(e) => setAt(i, { name: e.target.value })}
              placeholder={t("net.sites.namePlaceholder")}
              aria-label={t("net.sites.namePlaceholder")}
              maxLength={32}
              className="w-[118px] shrink-0 rounded-md border border-hairline-soft bg-transparent px-2 py-1 text-[11px] text-ink placeholder:text-ink-faint focus:outline-none focus:ring-1 focus:ring-accent/40"
            />
            <input
              value={s.url}
              onChange={(e) => setAt(i, { url: e.target.value })}
              placeholder={t("net.sites.urlPlaceholder")}
              aria-label={t("net.sites.urlPlaceholder")}
              spellCheck={false}
              className="min-w-0 flex-1 rounded-md border border-hairline-soft bg-transparent px-2 py-1 font-mono text-[11px] text-ink placeholder:text-ink-faint focus:outline-none focus:ring-1 focus:ring-accent/40"
            />
            <button
              type="button"
              onClick={() => onChange(draft.filter((_, idx) => idx !== i))}
              className="shrink-0 text-[10px] text-ink-faint transition-colors hover:text-danger"
            >
              {t("net.sites.remove")}
            </button>
          </li>
        ))}
      </ul>
      <div className="mt-2 flex items-center justify-between">
        <button
          type="button"
          onClick={() =>
            onChange([...draft, { id: "", name: "", url: "" }])
          }
          disabled={draft.length >= SITE_THRESHOLDS.maxSites}
          className="text-[10.5px] text-ink-dim transition-colors hover:text-ink disabled:opacity-50"
        >
          + {t("net.sites.add")}
        </button>
        <button
          type="button"
          onClick={onReset}
          className="text-[10.5px] text-ink-faint transition-colors hover:text-ink-dim"
        >
          {t("net.sites.reset")}
        </button>
      </div>
      <p className="mt-1.5 text-[10px] leading-relaxed text-ink-faint">{t("net.sites.validateHint")}</p>
    </div>
  );
}
