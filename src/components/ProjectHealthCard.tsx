import { useEffect, useState } from "react";

import { useI18n } from "../i18n/store";
import type { I18n } from "../i18n";
import type { NoParamKey } from "../i18n/types";
import { formatBytes } from "../lib/format";
import { isTauri, revealPath } from "../lib/ipc";
import type { ProjectHealth, ProjectVerdict } from "../types/disk";
import { Badge, type Tone } from "./primitives";

/** 与 prune-core PROJECT_STALE_DAYS 对齐：依赖 60 天没重装即「久未重装」 */
const STALE_DAYS = 60;

const VERDICT_META: Record<
  ProjectVerdict,
  { tone: Tone; labelKey: NoParamKey; tipKey: NoParamKey }
> = {
  zombie: {
    tone: "danger",
    labelKey: "disk.project.verdict.zombie",
    tipKey: "disk.project.verdict.zombieTip",
  },
  heavy: {
    tone: "warn",
    labelKey: "disk.project.verdict.heavy",
    tipKey: "disk.project.verdict.heavyTip",
  },
  stale: {
    tone: "warn",
    labelKey: "disk.project.verdict.stale",
    tipKey: "disk.project.verdict.staleTip",
  },
  healthy: {
    tone: "neutral",
    labelKey: "disk.project.verdict.healthy",
    tipKey: "disk.project.verdict.healthyTip",
  },
};

function installLabel(days: number, t: I18n["t"]): string {
  if (days <= 0) return t("disk.project.installToday");
  if (days === 1) return t("disk.project.installYesterday");
  if (days < 30) return t("disk.project.installDays", { n: days });
  if (days < 365) return t("disk.project.installMonths", { n: Math.round(days / 30) });
  return t("disk.project.installYears", { n: (days / 365).toFixed(1) });
}

function activityLabel(days: number | null, t: I18n["t"]): string {
  if (days == null) return t("disk.project.activityNone");
  if (days <= 0) return t("disk.project.activityToday");
  if (days === 1) return t("disk.project.activityYesterday");
  if (days < 30) return t("disk.project.activityDays", { n: days });
  if (days < 365) return t("disk.project.activityMonths", { n: Math.round(days / 30) });
  return t("disk.project.activityYears", { n: (days / 365).toFixed(1) });
}

/**
 * F9 项目健康卡（M24）：把「N 个 node_modules 共 X GB」拆成按项目的依赖账本。
 *
 * 只做体检：不做单项目清理入口（存量清理通道一次列出全部 node_modules，
 * 单独剪某一个需要新 API，不在本期）——卡里所有数字都可追溯到一次 stat，
 * 不显示「预计可省」这类推断值。
 */
export function ProjectHealthCard({ projects }: { projects: ProjectHealth[] }) {
  const [toast, setToast] = useState<string | null>(null);
  const { t } = useI18n();

  // 提示条 3 s 自走：Finder 打开失败是一次性事件，不该常驻在左下角
  useEffect(() => {
    if (!toast) return;
    const timer = window.setTimeout(() => setToast(null), 3000);
    return () => window.clearTimeout(timer);
  }, [toast]);

  if (projects.length === 0) return null;

  const totalBytes = projects.reduce((acc, p) => acc + p.depsBytes, 0);
  const zombies = projects.filter((p) => p.verdict === "zombie");
  const zombieBytes = zombies.reduce((acc, p) => acc + p.depsBytes, 0);
  const multi = projects.filter((p) => p.multiManager);
  const shown = projects.slice(0, 6);

  const open = async (p: ProjectHealth) => {
    if (!isTauri()) {
      setToast(t("disk.err.needsDesktop"));
      return;
    }
    try {
      await revealPath(p.root);
    } catch (e) {
      setToast(String(e));
    }
  };

  return (
    <section className="mt-4 rounded-xl border border-hairline-soft bg-chip-soft p-3.5">
      <div className="flex items-baseline justify-between gap-3">
        <h2 className="text-[12px] font-medium text-ink">{t("disk.project.title")}</h2>
        <span className="font-mono text-[11px] tabular-nums text-ink-dim">
          {t("disk.project.meta", { n: projects.length, size: formatBytes(totalBytes) })}
        </span>
      </div>

      <p className="mt-1 text-[10.5px] leading-relaxed text-ink-faint">
        {zombies.length > 0 ? (
          <>
            <span className="text-danger">{t("disk.project.zombies", { n: zombies.length })}</span>
            {t("disk.project.zombiesNote", {
              size: formatBytes(zombieBytes),
              days: STALE_DAYS,
            })}
          </>
        ) : (
          <>{t("disk.project.noZombies")}</>
        )}
        {multi.length > 0 ? t("disk.project.multi", { n: multi.length }) : ""}
      </p>

      <ul className="mt-2.5 space-y-0.5">
        {shown.map((p) => {
          const meta = VERDICT_META[p.verdict];
          return (
            <li key={p.root}>
              <button
                type="button"
                onClick={() => void open(p)}
                title={t("disk.fileList.title", {
                  path: p.depsPath,
                  tip: t("disk.pathMenu.openWithTip"),
                })}
                className="flex w-full items-center gap-2 rounded-md px-1 py-1.5 text-left transition-colors hover:bg-chip-hover"
              >
                <span className="min-w-0 flex-1">
                  <span className="block truncate text-[11.5px] text-ink">{p.name}</span>
                  <span className="mt-0.5 block truncate text-[10px] text-ink-faint">
                    {t("disk.project.line", {
                      install: installLabel(p.depsAgeDays, t),
                      activity: activityLabel(p.activityAgeDays, t),
                    })}
                  </span>
                </span>
                {p.multiManager ? (
                  <Badge
                    tone="info"
                    title={t("disk.project.multiTip", { files: p.lockfiles.join(" / ") })}
                  >
                    {t("disk.project.multiBadge")}
                  </Badge>
                ) : null}
                <Badge tone={meta.tone} title={t(meta.tipKey)}>
                  {t(meta.labelKey)}
                </Badge>
                <span className="w-16 shrink-0 text-right font-mono text-[11.5px] tabular-nums text-ink">
                  {formatBytes(p.depsBytes)}
                </span>
              </button>
            </li>
          );
        })}
        {projects.length > shown.length ? (
          <li className="px-1 pt-1 text-[10px] text-ink-faint">
            {t("disk.project.more", { n: projects.length - shown.length })}
          </li>
        ) : null}
      </ul>

      <p className="mt-1.5 text-[10px] leading-relaxed text-ink-faint">
        {t("disk.project.footnote")}
      </p>

      {toast ? (
        <div className="fixed bottom-16 left-5 z-50 rounded-md border border-danger/30 bg-danger/[0.12] px-3 py-2 text-[10.5px] text-danger shadow-xl">
          {toast}
        </div>
      ) : null}
    </section>
  );
}
