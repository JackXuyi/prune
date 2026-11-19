import { useState } from "react";

import { useI18n } from "../../i18n/store";
import { formatBytes } from "../../lib/format";
import { totalGrowth, type LeakReport } from "../../types/mem";
import { ChevronRightIcon, WarnTriangleIcon } from "../icons";
import { evidenceLine, LeakCard, windowLabel } from "../LeakCard";

/**
 * 泄漏嫌疑紧凑带（eul 重设计 · memory-eul-mockup.html ④）：
 * 一条带替代整张 LeakCard——头部统计 + 最多两行嫌疑（斜率证据走 tooltip），
 * 「卷宗」入口展开完整证据卡（LeakCard）。
 *
 * 出现条件不变（功能方案 §6.3）：有命中**或**无法判定（note 非空）才出现；
 * 没有命中整条带子不出现——沉默即正常。文案硬约束延续：
 * 只说「疑似」，永远不说「泄漏了」；「无法判定」如实显示，不冒充「没有泄漏」。
 */
export function LeakStrip({
  report,
  onOpen,
}: {
  report: LeakReport;
  /** 点嫌疑行 → 进程详情 */
  onOpen: (pid: number) => void;
}) {
  const { t, locale } = useI18n();
  const [dossier, setDossier] = useState(false);

  const hasHits = report.items.length > 0;
  // 无法判定（note 非空）也要出现：如实说明，不冒充「没有泄漏」
  if (!hasHits && !report.note) return null;

  const window = windowLabel(report.windowSecs, t);

  return (
    <>
      <div
        className="flex shrink-0 flex-col gap-1 rounded-[12px] border px-3 pt-1.5 pb-2"
        style={{
          borderColor: "color-mix(in srgb, var(--color-warn) 30%, transparent)",
          backgroundColor: "color-mix(in srgb, var(--color-warn) 6%, transparent)",
        }}
      >
        {/* 头部：tag + 观察统计 + 卷宗入口 */}
        <div className="flex items-center gap-2">
          <span className="flex items-center gap-1.5 text-[10px] font-semibold text-warn">
            <WarnTriangleIcon size={11} />
            {hasHits
              ? t("memory.leakStrip.suspects", { n: report.items.length })
              : t("memory.leakStrip.indeterminate")}
          </span>
          <span className="min-w-0 flex-1 truncate font-mono text-[9.5px] tabular-nums text-ink-faint">
            {report.note ?? t("memory.leakStrip.stripMeta", {
              observed: report.observed,
              benign: report.benign,
              insufficient: report.insufficient,
              window,
            })}
          </span>
          {hasHits ? (
            <button
              type="button"
              onClick={() => setDossier((v) => !v)}
              title={t("memory.leakStrip.dossierTip")}
              className="flex shrink-0 items-center gap-0.5 rounded px-1 py-0.5 text-[9.5px] text-warn transition-colors hover:bg-warn/10"
            >
              {t("memory.leakStrip.dossier")}
              <ChevronRightIcon
                size={9}
                className={`transition-transform ${dossier ? "rotate-90" : ""}`}
              />
            </button>
          ) : null}
        </div>

        {/* 嫌疑行：名称 / 家族 / 净增长 / R²；斜率与窗口证据全在 tooltip */}
        {hasHits
          ? report.items.slice(0, 2).map((item) => (
              <button
                key={item.pid}
                type="button"
                onClick={() => onOpen(item.pid)}
                title={`${item.name} · ${item.family}\n${evidenceLine(item, locale)}`}
                className="flex items-center gap-2 whitespace-nowrap text-left text-[10.5px] transition-colors hover:opacity-80"
              >
                <span className="min-w-0 flex-1 overflow-hidden text-ellipsis text-ink">{item.name}</span>
                {item.family !== item.name ? (
                  <span className="shrink-0 text-[9px] text-ink-faint">{item.family}</span>
                ) : null}
                <span className="shrink-0 font-mono text-[10.5px] tabular-nums text-warn">
                  +{formatBytes(item.growth)}
                </span>
                <span className="w-[56px] shrink-0 text-right font-mono text-[9.5px] tabular-nums text-ink-faint">
                  R² {item.r2.toFixed(2)}
                </span>
              </button>
            ))
          : null}
      </div>

      {/* 卷宗展开：完整证据卡（文案约束在 LeakCard 内） */}
      {dossier && hasHits ? (
        <div className="shrink-0">
          <LeakCard report={report} onOpen={onOpen} />
          <p className="mt-1 px-1 text-[9.5px] text-ink-faint">
            {t("memory.leak.bannerMeta", {
              families: new Set(report.items.map((i) => i.family)).size,
              procs: report.items.length,
              window,
              growth: formatBytes(totalGrowth(report.items)),
            })}
          </p>
        </div>
      ) : null}
    </>
  );
}
