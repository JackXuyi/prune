/**
 * 设置页「关于」卡片（docs/version-info-plan.md + F19 自耗透明）。
 *
 * 版本号来自 getVersion()（@tauri-apps/api/app）—— 运行时读 tauri.conf.json
 * 的 version，与 `pnpm bump` 维护的四处版本号天然同源，零 Rust command。
 *
 * 降级：浏览器 dev（isTauri() = false）不调 API；Tauri 下获取失败也静默
 * 降级 —— 两种情况都显示 "dev"，不渲染错误态（版本号拿不到不算故障）。
 * 只在挂载时取一次：应用生命周期内版本不会变。
 *
 * 「检查更新」按钮与托盘菜单项复用同一条 Rust 逻辑（updateCheckDialog →
 * check_and_prompt）：点了弹原生对话框，发现新版 / 已是最新 / 出错都有反馈。
 *
 * 「自身开销」四行（F19）：打开时拉一次，不轮询——这个数字不需要实时。
 * 缺数显示「暂无」而不是 0%（S3 纪律：不撒谎）；数字永远带口径与时间（S1）。
 */
import { useCallback, useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";

import { useI18n } from "../i18n/store";
import { isTauri, selfFootprint, updateCheckDialog } from "../lib/ipc";
import type { SelfFootprint } from "../types/net";
import { formatClock } from "../lib/format";
import { RefreshIcon } from "./icons";

export function AboutSection() {
  const { t } = useI18n();
  const [version, setVersion] = useState<string | null>(null);
  const [checking, setChecking] = useState(false);
  const [selfFp, setSelfFp] = useState<SelfFootprint | null>(null);

  useEffect(() => {
    if (!isTauri()) return;
    void getVersion()
      .then(setVersion)
      .catch(() => {});
    void selfFootprint()
      .then(setSelfFp)
      .catch(() => {});
  }, []);

  const check = useCallback(async () => {
    if (!isTauri() || checking) return;
    setChecking(true);
    try {
      await updateCheckDialog();
    } catch {
      // 弹窗结果由 Rust 侧负责；command 失败在这里静默，避免污染设置页
    } finally {
      setChecking(false);
    }
  }, [checking]);

  const mem = selfFp?.footprint;
  const memText =
    mem != null
      ? `${(mem / 1024 / 1024).toFixed(1)} MB`
      : t("settings.selfFootprint.unavailable");
  const cpu =
    selfFp?.cpuPctAvg != null
      ? `${selfFp.cpuPctAvg.toFixed(1)}%`
      : t("settings.selfFootprint.unavailable");
  const threads =
    selfFp?.threads != null
      ? String(selfFp.threads)
      : t("settings.selfFootprint.unavailable");
  const db =
    selfFp?.dbBytes != null
      ? `${(selfFp.dbBytes / 1024 / 1024).toFixed(1)} MB`
      : t("settings.selfFootprint.unavailable");

  return (
    <section
      className="rounded-card border border-hairline bg-surface-raised px-4 py-3.5"
      aria-label={t("settings.about.title")}
    >
      <div className="flex items-center justify-between">
        <span className="text-[13px] font-medium">Prune</span>
        <span className="text-[11px] font-mono tabular-nums text-ink-faint">
          {version ? `v${version}` : "dev"}
        </span>
      </div>

      {selfFp ? (
        <div className="mt-2.5 border-t border-hairline-soft pt-2.5">
          <div className="flex items-center justify-between">
            <span className="text-[11px] font-medium text-ink-dim">
              {t("settings.selfFootprint.title")}
            </span>
            <span
              className="text-[9.5px] text-ink-faint"
              title={t("settings.selfFootprint.sampleNote")}
            >
              {t("settings.selfFootprint.at", { clock: formatClock(selfFp.sampleAt) })}
            </span>
          </div>
          <dl className="mt-1.5 grid grid-cols-2 gap-x-4 gap-y-1">
            <div className="flex items-center justify-between gap-2">
              <dt className="text-[10px] text-ink-faint" title={t("settings.selfFootprint.memHint")}>
                {t("settings.selfFootprint.mem")}
              </dt>
              <dd className="font-mono text-[10.5px] tabular-nums text-ink">{memText}</dd>
            </div>
            <div className="flex items-center justify-between gap-2">
              <dt className="text-[10px] text-ink-faint">{t("settings.selfFootprint.cpu")}</dt>
              <dd className="font-mono text-[10.5px] tabular-nums text-ink">{cpu}</dd>
            </div>
            <div className="flex items-center justify-between gap-2">
              <dt className="text-[10px] text-ink-faint">{t("settings.selfFootprint.threads")}</dt>
              <dd className="font-mono text-[10.5px] tabular-nums text-ink">{threads}</dd>
            </div>
            <div className="flex items-center justify-between gap-2">
              <dt className="text-[10px] text-ink-faint">{t("settings.selfFootprint.db")}</dt>
              <dd className="font-mono text-[10.5px] tabular-nums text-ink">{db}</dd>
            </div>
          </dl>
          <p className="mt-1.5 text-[9.5px] leading-3.5 text-ink-faint">
            {t("settings.selfFootprint.noPromise")}
          </p>
        </div>
      ) : null}

      <div className="mt-2.5 border-t border-hairline-soft pt-2.5">
        <button
          type="button"
          onClick={() => void check()}
          disabled={checking || !isTauri()}
          className="flex items-center gap-1.5 text-[11px] text-ink-dim transition-colors hover:text-ink disabled:cursor-default disabled:opacity-60"
        >
          <RefreshIcon size={12} className={checking ? "animate-spin" : ""} />
          {checking ? t("settings.about.checking") : t("settings.about.checkUpdate")}
        </button>
      </div>
    </section>
  );
}
