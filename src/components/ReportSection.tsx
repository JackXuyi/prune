/**
 * 设置页「体检报告」卡片（复用已装 CLI，docs/w1-report-sample-plan.md §10）。
 *
 * 生成走 Rust 侧 report_generate：探测本机 prune CLI（≥ v0.13）→ spawn 五条
 * 只读命令（doctor 独占跑）→ 组装九段 MD（脚注溯源 / 0≠读不到 / 脱敏）→
 * 写入 app_data_dir/reports/。全程不联网、不删任何文件。
 *
 * 未装 CLI 时不挡路：只给安装提示，指向 brew 与仓库脚本两条路径。
 * 生成耗时约 1 分钟（磁盘面为主），按钮期间禁用并显示进度文案。
 */
import { useCallback, useEffect, useState } from "react";

import { useI18n } from "../i18n/store";
import { isTauri, reportCliDetect, reportGenerate, revealPath, type ReportCliInfo, type ReportOutcome } from "../lib/ipc";
import { FileIcon, FolderIcon, RefreshIcon } from "./icons";

type Phase = "detecting" | "missing" | "ready";

export function ReportSection() {
  const { t } = useI18n();
  const [cli, setCli] = useState<ReportCliInfo | null>(null);
  const [phase, setPhase] = useState<Phase>("detecting");
  const [generating, setGenerating] = useState(false);
  const [outcome, setOutcome] = useState<ReportOutcome | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!isTauri()) {
      setPhase("missing");
      return;
    }
    void reportCliDetect()
      .then((info) => {
        setCli(info);
        setPhase(info ? "ready" : "missing");
      })
      .catch(() => setPhase("missing"));
  }, []);

  const generate = useCallback(async () => {
    if (!isTauri() || generating || phase !== "ready") return;
    setGenerating(true);
    setError(null);
    try {
      const r = await reportGenerate();
      setOutcome(r);
    } catch (e) {
      setError(String(e));
    } finally {
      setGenerating(false);
    }
  }, [generating, phase]);

  return (
    <section className="rounded-card border border-hairline bg-surface-raised px-4 py-3.5" aria-label={t("settings.report.title")}>
      <div className="flex items-center gap-1.5">
        <FileIcon size={12} />
        <span className="text-[13px] font-medium">{t("settings.report.title")}</span>
      </div>
      <p className="mt-1.5 text-[11px] leading-4.5 text-ink-dim">{t("settings.report.desc")}</p>

      <div className="mt-2.5 flex flex-wrap items-center gap-x-3 gap-y-1.5 border-t border-hairline-soft pt-2.5">
        {phase === "detecting" && <span className="text-[11px] text-ink-faint">{t("settings.report.detecting")}</span>}

        {phase === "missing" && (
          <p className="text-[11px] leading-4.5 text-ink-faint">
            {t("settings.report.notFound")}
            <br />
            {t("settings.report.notFoundHint")}
          </p>
        )}

        {phase === "ready" && (
          <>
            <span className="text-[11px] font-mono tabular-nums text-ink-faint">
              {t("settings.report.cliFound", { version: cli?.version ?? "" })}
            </span>
            <button
              type="button"
              onClick={() => void generate()}
              disabled={generating}
              className="flex items-center gap-1.5 text-[11px] text-ink-dim transition-colors hover:text-ink disabled:cursor-default disabled:opacity-60"
            >
              <RefreshIcon size={12} className={generating ? "animate-spin" : ""} />
              {generating ? t("settings.report.generating") : t("settings.report.generate")}
            </button>
          </>
        )}
      </div>

      {error && <p className="mt-1.5 text-[11px] leading-4.5 text-danger">{t("settings.report.failed", { err: error })}</p>}

      {outcome && !error && (
        <div className="mt-1.5 space-y-1">
          <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
            <span className="text-[11px] text-ink-faint">
              {t("settings.report.done", { seconds: outcome.elapsedS })}
            </span>
            <button
              type="button"
              onClick={() => void revealPath(outcome.path).catch(() => {})}
              className="flex items-center gap-1.5 text-[11px] text-ink-dim transition-colors hover:text-ink"
            >
              <FolderIcon size={12} />
              {t("settings.report.openFolder")}
            </button>
          </div>
          {outcome.degraded.length > 0 && (
            <p className="text-[11px] leading-4.5 text-ink-faint">
              {t("settings.report.degradedWarn", { items: outcome.degraded.join("；") })}
            </p>
          )}
        </div>
      )}
    </section>
  );
}
