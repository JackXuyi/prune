import { useEffect, useMemo, useState } from "react";

import { formatClock } from "../../../lib/format";
import { useI18n } from "../../../i18n/store";
import { useSiteStore } from "../../../stores/siteStore";
import {
  SITE_THRESHOLDS,
  type Site,
  summarizeSites,
} from "../../../types/net";
import { GhostButton } from "../bits";
import { SiteCard } from "./SiteCard";
import { SiteDetailModal } from "./SiteDetailModal";

/**
 * 站点检测区（F15，docs/net-sites-plan.md §9）——测试页底部 flex 区。
 *
 * 三条纪律（§12）决定这里的每一处措辞：
 * - 断点分开显示：DNS / TCP / TLS / 首字节 / 传输，`stage` 决定文案，不写「被墙」；
 * - `refused`（403/429）是**可达**，只标黄不标红；
 * - 速率为 null 时显示「样本不足（体积 / 时长）」，**不显示 0 KB/s**。
 *
 * 布局：汇总条 + 卡片网格给「一眼看十站」，点卡片开详情弹窗看「时间花在哪一段」；
 * 阈值口径逐行贴在页脚（不搞黑盒）。编辑态在页内切换（非弹窗）。
 */
export function SitesSection() {
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

  // 首次进入本区才拉站表 / 订阅进度事件
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

  // curl 缺失：整区换成说明，不渲染带 0 的空表（W2 先例：无等价物就隐藏）
  if (capability && !capability.curl) {
    return (
      <section className="rounded-card border border-hairline-soft bg-surface-raised px-4 py-3.5">
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

  // 头部 note：站数 · 上次全量 · 耗时；running/cancelled 提示覆盖同一行
  const note = running
    ? t("net.sites.running", { done, total })
    : cancelled && !running
      ? t("net.sites.cancelledNote")
      : sites.length > 0
        ? `${t("net.sites.headCount", { n: sites.length })}${
            ranAt != null ? ` · ${t("net.sites.lastRun", { time: formatClock(ranAt) })}` : ""
          }${
            elapsedMs != null && !running
              ? ` · ${t("net.sites.elapsed", { s: (elapsedMs / 1000).toFixed(1) })}`
              : ""
          }`
        : t("net.sites.empty");

  return (
    <section className="flex flex-col">
      {/* 头：区块名 + 口径行 + 主操作 */}
      <div className="flex shrink-0 items-center gap-2">
        <span className="shrink-0 text-[11px] font-semibold text-ink-dim">
          {t("net.test.sitesTitle")}
        </span>
        <span
          className={`min-w-0 flex-1 truncate text-[9.5px] ${running || (cancelled && !running) ? "text-warn" : "text-ink-faint"}`}
        >
          {note}
        </span>
        {recovered ? (
          <span className="shrink-0 text-[9.5px] text-warn" title={t("net.sites.recovered")}>
            ⚠ {t("net.sites.recovered")}
          </span>
        ) : null}
        {editing ? (
          <>
            <GhostButton onClick={() => void submitEdit()}>{t("net.sites.save")}</GhostButton>
            <GhostButton onClick={() => setEditing(false)}>{t("net.sites.cancelEdit")}</GhostButton>
          </>
        ) : (
          <>
            {running ? (
              <GhostButton variant="stop" onClick={() => void cancel()} title={t("net.test.runStopTip")}>
                {t("net.sites.cancel")}
              </GhostButton>
            ) : (
              <GhostButton
                variant="go"
                onClick={() => void runAll()}
                disabled={sites.length === 0}
                title={t("net.test.runAllTip")}
              >
                {t("net.sites.run")}
              </GhostButton>
            )}
            <GhostButton onClick={startEdit} title={t("net.test.editTip")}>
              {t("net.sites.edit")}
            </GhostButton>
          </>
        )}
      </div>

      {listError ? (
        <p className="mt-1.5 shrink-0 rounded-md bg-warn/[0.1] px-2.5 py-1.5 text-[10.5px] leading-relaxed text-warn">
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
        <p className="mt-2 text-[11px] leading-relaxed text-ink-faint">{t("net.sites.empty")}</p>
      ) : (
        <>
          {summary.total > 0 ? <SummaryBar summary={summary} /> : null}
          <div
            className="mt-2 grid content-start gap-2"
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
        </>
      )}

      {/* 页脚：阈值逐行展示（不搞黑盒）+ 复制报告 */}
      <div className="mt-auto flex shrink-0 items-baseline justify-between gap-3 pt-1.5">
        <p className="min-w-0 truncate text-[9.5px] text-ink-faint" title={t("net.sites.scope")}>
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
          className={`shrink-0 text-[9.5px] transition-colors disabled:opacity-50 ${
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
      </div>

      {detailSite ? (
        <SiteDetailModal
          site={detailSite}
          probe={probes[detailSite.id] ?? null}
          busy={running || Boolean(rerunning[detailSite.id])}
          onRerun={() => void rerun(detailSite.id)}
          onClose={() => setDetailId(null)}
        />
      ) : null}
    </section>
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
    <div className="mt-1.5 flex shrink-0 flex-wrap items-center gap-x-3.5 gap-y-1 border-b border-hairline-soft pb-1.5">
      {parts
        .filter((p) => summary[p.key] > 0)
        .map((p) => (
          <span key={p.key} className="font-mono text-[10.5px] tabular-nums">
            <span className={p.tone}>{summary[p.key]}</span>
            <span className="ml-1 font-sans text-ink-faint">{p.label}</span>
          </span>
        ))}
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
    <div className="mt-2 flex flex-col">
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
      <div className="mt-2 flex shrink-0 items-center justify-between">
        <button
          type="button"
          onClick={() => onChange([...draft, { id: "", name: "", url: "" }])}
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
      <p className="mt-1.5 shrink-0 text-[10px] leading-relaxed text-ink-faint">
        {t("net.sites.validateHint")}
      </p>
    </div>
  );
}
