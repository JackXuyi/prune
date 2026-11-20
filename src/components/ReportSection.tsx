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
import { StatusDot } from "./settings/kit";
import { FolderIcon, RefreshIcon } from "./icons";

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
    <>
      {/* 紧凑行（settings-eul-mockup）：label + CLI 状态点 + 生成按钮。
          desc / 未装 CLI 的完整提示进行级 tooltip，不挡行 */}
      <div
        className="flex items-center justify-between gap-2.5 py-1"
        title={phase === "missing" ? `${t("settings.report.notFound")} ${t("settings.report.notFoundHint")}` : t("settings.report.desc")}
      >
        <span className="min-w-0 truncate text-[11.5px] text-ink">
          {t("settings.report.title")}
        </span>
        <span className="flex shrink-0 items-center gap-2">
          {phase === "detecting" && (
            <span className="text-[10px] text-ink-faint">{t("settings.report.detecting")}</span>
          )}
          {phase === "missing" && (
            <StatusDot tone="warn" title={`${t("settings.report.notFound")} ${t("settings.report.notFoundHint")}`}>
              {t("settings.report.notFound")}
            </StatusDot>
          )}
          {phase === "ready" && (
            <>
              <StatusDot title={t("settings.report.desc")}>
                {t("settings.report.cliFound", { version: cli?.version ?? "" })}
              </StatusDot>
              <button
                type="button"
                onClick={() => void generate()}
                disabled={generating}
                className="flex items-center gap-1 rounded-md bg-chip px-2.5 py-1 text-[10.5px] text-ink-dim transition-colors hover:bg-chip-hover hover:text-ink disabled:cursor-default disabled:opacity-60"
              >
                <RefreshIcon size={11} className={generating ? "animate-spin" : ""} />
                {generating ? t("settings.report.generating") : t("settings.report.generate")}
              </button>
            </>
          )}
        </span>
      </div>

      {error && <p className="pb-1 text-[9.5px] leading-4 text-danger">{t("settings.report.failed", { err: error })}</p>}

      {outcome && !error && (
        <div className="pb-1">
          <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
            <span className="text-[9.5px] text-ink-faint">
              {t("settings.report.done", { seconds: outcome.elapsedS })}
            </span>
            <button
              type="button"
              onClick={() => void revealPath(outcome.path).catch(() => {})}
              className="flex items-center gap-1.5 text-[9.5px] text-ink-dim transition-colors hover:text-ink"
            >
              <FolderIcon size={11} />
              {t("settings.report.openFolder")}
            </button>
          </div>
          {outcome.degraded.length > 0 && (
            <p className="mt-0.5 text-[9.5px] leading-4 text-ink-faint">
              {t("settings.report.degradedWarn", { items: outcome.degraded.join("；") })}
            </p>
          )}
        </div>
      )}
    </>
  );
}
