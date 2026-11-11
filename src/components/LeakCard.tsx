import { useEffect, useState } from "react";

import { i18nFor } from "../i18n";
import { useI18n } from "../i18n/store";
import type { I18n } from "../i18n";
import { formatBytes, formatPercent, formatRelative } from "../lib/format";
import { isTauri, leakCases, memHistory } from "../lib/ipc";
import { useAppIcons } from "../lib/useAppIcons";
import { useMemStore } from "../stores/memStore";
import { totalGrowth, type LeakCase, type LeakItem, type LeakReport } from "../types/mem";
import { AppIcon } from "./AppIcon";
import { ChevronRightIcon } from "./icons";
import { Badge } from "./primitives";
import { Sparkline } from "./Sparkline";

/**
 * 泄漏嫌疑的证据展示（M3 / B5）。
 *
 * 文案硬约束（功能方案 §6.3）——改这个文件前先读：
 * - 只能说「疑似内存泄漏」，**永远不说**「你的 XX 泄漏了」；
 * - 必须展示依据（增长量 + 时长 + R²），只给结论等于制造焦虑；
 * - 不允许出现「释放 / 加速 / 优化」这类词。
 *
 * 误报一次的信任成本远高于漏报一次，所以这里是「证据优先」的排版：
 * 数字在前，结论在后，且措辞留有余地。
 *
 * i18n：`windowLabel` / `evidenceLine` 是纯函数，locale 必须显式传入 ——
 * 函数内部读全局会让「切语言后已渲染的旧串」不更新。
 */

function windowLabel(secs: number, t: I18n["t"]): string {
  if (secs >= 3600) return t("memory.leak.windowHours", { n: Math.round(secs / 3600) });
  return t("memory.leak.windowMinutes", { n: Math.round(secs / 60) });
}

/** 一句话证据：近 30 min 增长 480 MB（+51%）· 斜率 960 MB/h · R² 0.86 */
export function evidenceLine(item: LeakItem, locale: I18n["locale"]): string {
  const t = i18nFor(locale).t;
  return t("memory.leak.evidence", {
    window: windowLabel(item.windowSecs, t),
    growth: formatBytes(item.growth),
    pct: formatPercent(item.growthRatio),
    slope: formatBytes(item.slopeMbPerHour * 1e6),
    r2: item.r2.toFixed(2),
  });
}

/** 面板用：一行提示条，只在有命中时出现 */
export function LeakBanner({
  report,
  onOpen,
}: {
  report: LeakReport;
  onOpen: () => void;
}) {
  const { t, locale } = useI18n();
  const items = report.items;
  if (items.length === 0) return null;

  const families = new Set(items.map((i) => i.family));
  const window = windowLabel(items[0].windowSecs, t);

  return (
    <button
      type="button"
      onClick={onOpen}
      title={items.map((i) => `${i.name}: ${evidenceLine(i, locale)}`).join("\n")}
      className="mb-2 flex w-full items-center gap-2 rounded-lg border border-warn/25 bg-warn/[0.07] px-2.5 py-2 text-left transition-colors hover:bg-warn/[0.11]"
    >
      <span className="min-w-0 flex-1">
        <span className="block text-[11px] leading-tight text-warn">
          {t("memory.leak.bannerTitle")}
        </span>
        <span className="mt-0.5 block text-[10px] leading-tight text-ink-faint">
          {t("memory.leak.bannerMeta", {
            families: families.size,
            procs: items.length,
            window,
            growth: formatBytes(totalGrowth(items)),
          })}
        </span>
      </span>
      <ChevronRightIcon size={12} className="shrink-0 text-warn/70" />
    </button>
  );
}

/** 主窗口用：悬案卷宗（M22）——嫌疑人档案照 + 证物曲线 + 结案陈词 */
export function LeakCard({
  report,
  onOpen,
}: {
  report: LeakReport;
  onOpen: (pid: number) => void;
}) {
  const { t } = useI18n();
  // 无法判定时必须说出来，不能让用户以为「检查过、没问题」
  if (report.note) {
    return (
      <p className="rounded-lg border border-hairline-soft bg-surface-raised/40 px-3 py-2 text-[11px] leading-relaxed text-ink-faint">
        {t("memory.leak.unavailable", { note: report.note })}
      </p>
    );
  }

  if (report.items.length === 0) return null;

  return <CaseSection report={report} onOpen={onOpen} />;
}

/** 案件簿快照：随 report（每轮深探）变化重新拉取 */
function useCaseBook(report: LeakReport): LeakCase[] {
  const [cases, setCases] = useState<LeakCase[]>([]);
  useEffect(() => {
    if (!isTauri()) return;
    let alive = true;
    void leakCases()
      .then((r) => {
        if (alive) setCases(r);
      })
      .catch(() => {
        /* 卷宗是叙事层，拉不到就降级为「无编号」模式，不弹错 */
      });
    return () => {
      alive = false;
    };
  }, [report]);
  return cases;
}

/** 证物曲线：某案件的 footprint 序列（70 min 覆盖 GC 语言的 60 min 窗口） */
function CaseCurve({ pid }: { pid: number }) {
  const [values, setValues] = useState<number[]>([]);
  useEffect(() => {
    let alive = true;
    void memHistory(pid, 70)
      .then((pts) => {
        if (alive) setValues(pts.map((p) => p.footprint ?? p.rss));
      })
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, [pid]);
  if (values.length < 2) return null;
  return <Sparkline values={values} tone="warn" height={34} />;
}

function CaseSection({
  report,
  onOpen,
}: {
  report: LeakReport;
  onOpen: (pid: number) => void;
}) {
  const cases = useCaseBook(report);
  const processes = useMemStore((s) => s.processes);
  const i18n = useI18n();
  const { t } = i18n;

  // 嫌疑人档案照：按 family 取主进程 exe 换真图标；拿不到 AppIcon 自己降级字母徽标
  const families = [...new Set(report.items.map((i) => i.family))];
  const iconItems = families.map((f) => ({
    family: f,
    exe: processes.find((p) => p.family === f && p.exe)?.exe ?? null,
  }));
  const icons = useAppIcons(iconItems);

  const closed = cases.filter((c) => c.status !== "open");

  return (
    <section className="rounded-lg border border-warn/25 bg-warn/[0.05] p-3">
      <div className="flex items-baseline justify-between gap-2">
        <div className="flex items-center gap-1.5">
          <span className="text-[11px] font-medium text-warn">{t("memory.leak.caseTitle")}</span>
          <Badge tone="warn" title={t("memory.leak.caseBadgeTip")}>
            {t("memory.leak.caseBadge")}
          </Badge>
        </div>
        <span className="text-[10px] text-ink-faint">
          {t("memory.leak.checked", { n: report.observed, m: report.items.length })}
        </span>
      </div>

      <ul className="mt-1.5">
        {report.items.map((item) => (
          <li key={`${item.pid}-${item.startTime}`} className="border-t border-warn/10 first:border-t-0 first:pt-0 pt-1.5 mt-1.5">
            <button
              type="button"
              onClick={() => onOpen(item.pid)}
              className="w-full rounded-md px-1 py-1 text-left transition-colors hover:bg-white/[0.05]"
            >
              <div className="flex items-center gap-2.5">
                <AppIcon
                  family={item.family}
                  icon={icons[item.family] ?? null}
                  size={28}
                  title={t("memory.leak.avatarTip", { family: item.family })}
                />
                <span className="min-w-0 flex-1">
                  <span className="flex items-baseline gap-1.5">
                    <span className="truncate font-mono text-[10px] tabular-nums text-warn/80">
                      {caseIdOf(cases, item) ?? t("memory.leak.pid", { pid: item.pid })}
                    </span>
                    <span className="truncate text-[12px] leading-tight text-ink">{item.name}</span>
                    <span className="truncate text-[10px] text-ink-faint">{item.family}</span>
                    {item.gcRuntime ? (
                      <span
                        className="shrink-0 text-[10px] text-ink-faint"
                        title={t("memory.leak.gcTip")}
                      >
                        {t("memory.leak.gc")}
                      </span>
                    ) : null}
                    <span className="ml-auto shrink-0 text-[10px] text-ink-faint">
                      {t("memory.leak.investigating", {
                        d: durationLabel(item, cases, t),
                      })}
                    </span>
                  </span>
                  <span className="mt-0.5 block truncate text-[10px] leading-tight text-ink-faint">
                    {evidenceLine(item, i18n.locale)}
                  </span>
                </span>
                <span className="shrink-0 text-right">
                  <span className="block font-mono text-[11px] tabular-nums text-warn">
                    +{formatBytes(item.growth)}
                  </span>
                  <span className="mt-0.5 block font-mono text-[10px] tabular-nums text-ink-faint">
                    {t("memory.leak.current", { size: formatBytes(item.footprint) })}
                  </span>
                </span>
                <ChevronRightIcon size={11} className="shrink-0 text-ink-faint" />
              </div>
              {/* 证物：生长曲线。缺序列时安静跳过（深探轮次积累后自然出现） */}
              <div className="mt-1 px-0.5 pb-0.5">
                <CaseCurve pid={item.pid} />
              </div>
            </button>
          </li>
        ))}
      </ul>

      {closed.length > 0 && <ArchiveRoom cases={closed} />}

      <p className="mt-1.5 text-[10px] leading-relaxed text-ink-faint">
        {t("memory.leak.criteria", { window: windowLabel(report.windowSecs, t) })}
      </p>
    </section>
  );
}

/** 在案件簿里找这轮命中的卷宗编号 */
function caseIdOf(cases: LeakCase[], item: LeakItem): string | null {
  return cases.find((c) => c.pid === item.pid && c.startTime === item.startTime)?.id ?? null;
}

/** 侦查时长：立案至今的口语化描述（卷宗编号缺失时退回「窗口」口径） */
function durationLabel(item: LeakItem, cases: LeakCase[], t: I18n["t"]): string {
  const c = cases.find((x) => x.pid === item.pid && x.startTime === item.startTime);
  if (!c) return windowLabel(item.windowSecs, t);
  const mins = Math.max(1, Math.round((c.lastSeen - c.firstSeen) / 60));
  if (mins >= 60) return `${Math.floor(mins / 60)}h${mins % 60 > 0 ? ` ${mins % 60}m` : ""}`;
  return `${mins} min`;
}

/** 档案室：已结案卷宗（折叠，默认收起——这是彩蛋不是告警） */
function ArchiveRoom({ cases }: { cases: LeakCase[] }) {
  const { t, locale } = useI18n();
  return (
    <details className="mt-2 border-t border-warn/10 pt-2">
      <summary className="cursor-pointer select-none text-[10px] text-ink-faint transition-colors hover:text-ink-dim">
        {t("memory.leak.archive", { n: cases.length })}
      </summary>
      <ul className="mt-1.5 space-y-1">
        {cases.map((c) => (
          <li key={c.id} className="flex items-baseline gap-2 text-[11px]">
            <span className="shrink-0 font-mono text-[10px] tabular-nums text-ink-faint">{c.id}</span>
            <span className="truncate text-ink-dim">
              {c.name}
              <span className="ml-1 text-[10px] text-ink-faint">{c.family}</span>
            </span>
            <span className="ml-auto shrink-0 text-[10px] text-ink-faint">
              {c.status === "relapsed" ? t("memory.leak.relapsed") : t("memory.leak.archived")}
            </span>
            <span className="w-16 shrink-0 text-right text-[10px] text-ink-faint">
              {formatRelative(c.closedAt, locale)}
            </span>
          </li>
        ))}
      </ul>
    </details>
  );
}

/** 检视器用：选中进程命中时展示完整证据 */
export function LeakEvidence({ item }: { item: LeakItem }) {
  const { t } = useI18n();
  return (
    <section className="rounded-lg border border-warn/25 bg-warn/[0.05] p-3.5">
      <div className="flex items-baseline justify-between">
        <span className="text-[11px] font-medium text-warn">{t("memory.leak.badge")}</span>
        <span className="text-[10px] text-ink-faint">{t("memory.leak.badgeTip")}</span>
      </div>
      <dl className="mt-2 space-y-1.5 text-[11px]">
        <EvRow
          label={t("memory.leak.windowRow")}
          value={t("memory.leak.windowValue", {
            window: windowLabel(item.windowSecs, t),
            n: item.points,
          })}
        />
        <EvRow
          label={t("memory.leak.growthRow")}
          value={t("memory.leak.growthValue", {
            size: formatBytes(item.growth),
            pct: formatPercent(item.growthRatio),
          })}
        />
        <EvRow
          label={t("memory.leak.slopeRow")}
          value={`${formatBytes(item.slopeMbPerHour * 1e6)}/h`}
        />
        <EvRow label={t("memory.leak.r2Row")} value={`R² ${item.r2.toFixed(3)}`} />
        <EvRow
          label={t("memory.leak.stepRow")}
          value={t("memory.leak.stepValue", { pct: formatPercent(item.stepRatio) })}
        />
        {item.gcRuntime ? (
          <EvRow label={t("memory.leak.thresholdRow")} value={t("memory.leak.thresholdGc")} />
        ) : null}
      </dl>
      <p className="mt-2 text-[10px] leading-relaxed text-ink-faint">
        {t("memory.leak.conclusion")}
      </p>
    </section>
  );
}

function EvRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-start gap-3">
      <dt className="w-20 shrink-0 text-ink-faint">{label}</dt>
      <dd className="min-w-0 flex-1 text-ink-dim">{value}</dd>
    </div>
  );
}
