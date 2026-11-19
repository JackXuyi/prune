import { useEffect } from "react";

import { useI18n } from "../../i18n/store";
import { useNetDiagStore } from "../../stores/netDiagStore";
import { DIAG_THRESHOLDS, type LayerProbe, type NetDiag } from "../../types/net";
import { GhostButton } from "./bits";

/**
 * 链路诊断轨道卡（84px，自诊断二级页并入测试页）：DNS → 网关 → 出口三节点轨道
 * + 右侧 verdict 块。高度账（设计稿定稿）只放得下「轨道 + 结论」，逐层横柱图
 * 让位给节点 tooltip（样本、阈值、口径全在里面）。
 *
 * 状态机纪律（§7 / 设计稿交互契约）：
 * - running 禁用重复点击，节点逐段点亮，不出中间结论；
 * - failed（探测机制本身失败）与「连不上路由器」（done + verdict）严格区分；
 * - median null = 探测失败 → danger，**不是 0 ms**；
 * - 超阈值 = 该层是当前瓶颈 → warn；正常 → accent。
 */
export function DiagTrackCard() {
  const { t } = useI18n();
  const diag = useNetDiagStore((s) => s.diag);
  const running = useNetDiagStore((s) => s.running);
  const copied = useNetDiagStore((s) => s.copied);
  const copyFailed = useNetDiagStore((s) => s.copyFailed);
  const run = useNetDiagStore((s) => s.run);
  const copyReport = useNetDiagStore((s) => s.copyReport);
  const resetCopied = useNetDiagStore((s) => s.resetCopied);

  useEffect(() => {
    if (!copied && !copyFailed) return;
    const timer = setTimeout(resetCopied, 1500);
    return () => clearTimeout(timer);
  }, [copied, copyFailed, resetCopied]);

  return (
    <section className="flex h-[84px] shrink-0 flex-col rounded-[14px] border border-hairline-soft bg-surface-raised px-4 py-2.5">
      <div className="flex shrink-0 items-baseline gap-2">
        <span className="text-[11px] font-semibold text-ink-dim">{t("net.test.diagTitle")}</span>
        <span className="min-w-0 flex-1 truncate text-[9.5px] text-ink-faint" title={t("net.diag.scopeNote")}>
          {t("net.test.diagNote")}
        </span>
        <GhostButton onClick={() => void copyReport()} title={t("net.diag.copyTitle")}>
          {copyFailed
            ? `✗ ${t("common.action.copyFailed")}`
            : copied
              ? `✓ ${t("common.action.copied")}`
              : t("net.diag.copy")}
        </GhostButton>
        <GhostButton
          onClick={() => void run()}
          disabled={running}
          title={t("net.test.diagRunTip")}
        >
          {running ? t("net.test.diagRunningBtn") : t("net.test.diagRerun")}
        </GhostButton>
      </div>

      <div className="mt-1 grid min-h-0 flex-1 grid-cols-[minmax(0,1fr)_300px] items-center gap-4">
        <Track diag={diag} running={running} />
        <Verdict diag={diag} running={running} />
      </div>
    </section>
  );
}

// ---------------------------------------------------------------- 轨道

/** 紧凑横排节点（设计稿定稿形态）：8px 状态点 + 名称一行、mono 读数一行 */
function Track({ diag, running }: { diag: NetDiag | null; running: boolean }) {
  const { t } = useI18n();
  const status = diag?.status ?? "idle";

  const node = (label: string, probe: LayerProbe | null, threshold: number, tip: string) => {
    const median = probe?.medianMs ?? null;
    // failed（探测机制本身出错）节点保持灰点——「没探成」不是「这层坏了」
    const state =
      status === "running" || status === "idle" || status === "failed"
        ? "idle"
        : median == null
          ? "bad"
          : median > threshold
            ? "warn"
            : "ok";
    const reading =
      status === "running"
        ? "…"
        : status === "idle" || status === "failed"
          ? "—"
          : median != null
            ? `${median.toFixed(1)} ms`
            : (probe?.note ?? t("net.diag.probeFailed"));
    return { label, state, reading, tip };
  };

  const nodes = [
    node(
      "DNS",
      diag?.dns ?? null,
      DIAG_THRESHOLDS.dnsMs,
      t("net.diag.dnsSub"),
    ),
    node(
      diag?.gateway ? t("net.diag.routerWith", { gateway: diag.gateway }) : t("net.diag.router"),
      diag?.lan ?? null,
      DIAG_THRESHOLDS.lanMs,
      t("net.diag.lanSub"),
    ),
    node(
      t("net.diag.wanName"),
      diag?.wan ?? null,
      DIAG_THRESHOLDS.wanLanGapMs,
      diag?.jitterMs != null
        ? t("net.diag.wanSubJitter", {
            jitter: diag.jitterMs.toFixed(1),
            threshold: DIAG_THRESHOLDS.jitterMs,
          })
        : t("net.diag.wanSub"),
    ),
  ];

  const linkTone = (a: (typeof nodes)[number], b: (typeof nodes)[number]) => {
    if (a.state === "ok" && b.state === "ok") return "var(--color-accent)";
    if (a.state === "ok" && (b.state === "warn" || b.state === "bad"))
      return b.state === "warn" ? "var(--color-warn)" : "var(--color-danger)";
    return null;
  };

  const dotCls = (s: string) =>
    s === "ok"
      ? "bg-accent shadow-[0_0_6px_color-mix(in_srgb,var(--color-accent)_55%,transparent)]"
      : s === "warn"
        ? "bg-warn shadow-[0_0_6px_color-mix(in_srgb,var(--color-warn)_55%,transparent)]"
        : s === "bad"
          ? "bg-danger shadow-[0_0_6px_color-mix(in_srgb,var(--color-danger)_55%,transparent)]"
          : running
            ? "bg-info animate-pulse"
            : "bg-chip-hover";

  const readCls = (s: string) =>
    s === "warn" ? "text-warn" : s === "bad" ? "text-danger" : s === "ok" ? "text-ink" : "text-ink-faint";

  return (
    <div
      className="flex min-w-0 items-center"
      role="img"
      aria-label={t("net.diag.trackAria")}
      title={t("net.test.trackTip")}
    >
      {nodes.map((n, i) => (
        <div key={n.label} className="flex min-w-0 items-center">
          {i > 0 ? (
            <span
              className="mx-2 h-[2px] min-w-3 flex-1 rounded-full"
              style={{
                background: linkTone(nodes[i - 1], n)
                  ? `color-mix(in srgb, ${linkTone(nodes[i - 1], n)} 45%, transparent)`
                  : "var(--color-chip)",
              }}
              aria-hidden
            />
          ) : null}
          <div className="flex shrink-0 flex-col gap-0.5" title={n.tip}>
            <span className="flex items-center gap-1.5 text-[10.5px] text-ink-dim">
              <span className={`size-2 shrink-0 rounded-full ${dotCls(n.state)}`} aria-hidden />
              {n.label}
            </span>
            <span className={`whitespace-nowrap font-mono text-[11.5px] leading-none tabular-nums ${readCls(n.state)}`}>
              {n.reading}
            </span>
          </div>
        </div>
      ))}
    </div>
  );
}

// ---------------------------------------------------------------- verdict 块

/** 右侧结论区：idle / running / failed / captive / verdict 五态，阈值与规则随文案给出 */
function Verdict({ diag, running }: { diag: NetDiag | null; running: boolean }) {
  const { t } = useI18n();
  const status = diag?.status ?? "idle";

  const head = (text: string, tone: "idle" | "warn" | "danger" | "ok") => {
    const cls =
      tone === "warn"
        ? "text-warn"
        : tone === "danger"
          ? "text-danger"
          : tone === "ok"
            ? "text-ink"
            : "font-medium text-ink-faint";
    return <div className={`text-[12px] leading-tight font-semibold ${cls}`}>{text}</div>;
  };

  const body = (text: string) => (
    <p className="mt-0.5 line-clamp-2 text-[9.5px] leading-[1.45] text-ink-faint">{text}</p>
  );

  if (running || status === "running") {
    return (
      <div className="min-w-0 border-l border-hairline-soft pl-3.5">
        {head(t("net.test.diagRunningHead"), "idle")}
        {body(t("net.diag.running"))}
      </div>
    );
  }
  if (status === "failed") {
    return (
      <div className="min-w-0 border-l border-hairline-soft pl-3.5">
        {head(t("net.test.diagFailedHead"), "danger")}
        {body(t("net.diag.failed", { why: diag?.note ?? t("net.diag.unknownWhy") }))}
      </div>
    );
  }
  if (status !== "done" || !diag) {
    return (
      <div className="min-w-0 border-l border-hairline-soft pl-3.5">
        {head(t("net.test.diagIdleHead"), "idle")}
        {body(t("net.test.diagIdleBody"))}
      </div>
    );
  }
  if (diag.captive === "suspected") {
    return (
      <div className="min-w-0 border-l border-hairline-soft pl-3.5">
        {head(t("net.test.captiveHead"), "warn")}
        {body(t("net.diag.captive"))}
      </div>
    );
  }
  if (diag.verdict) {
    const wanSlow = diag.wan?.medianMs != null && diag.wan.medianMs > DIAG_THRESHOLDS.wanLanGapMs;
    return (
      <div
        className="min-w-0 border-l border-hairline-soft pl-3.5"
        title={t("net.test.verdictTip")}
      >
        {head(diag.verdict.label, wanSlow ? "warn" : "ok")}
        {body(t("net.diag.verdictRule", { advice: diag.verdict.advice, rule: diag.verdict.rule }))}
      </div>
    );
  }
  return (
    <div className="min-w-0 border-l border-hairline-soft pl-3.5">
      {head("—", "idle")}
      {body(t("net.diag.scopeNote"))}
    </div>
  );
}
