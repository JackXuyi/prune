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
import { RefreshIcon, ScissorsIcon } from "./icons";
import { SettingsGroup } from "./settings/kit";

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
    <SettingsGroup
      icon={<ScissorsIcon size={13} />}
      title={t("settings.about.title")}
      note={version ? `v${version}` : "dev"}
      titleTooltip={t("settings.selfFootprint.sampleNote")}
    >
      {/* 版本行 + 检查更新（与托盘菜单项复用同一条 Rust 逻辑 updateCheckDialog） */}
      <div className="flex items-center justify-between gap-2.5 py-1">
        <span className="text-[11.5px] text-ink">Prune</span>
        <button
          type="button"
          onClick={() => void check()}
          disabled={checking || !isTauri()}
          className="flex items-center gap-1.5 text-[10.5px] text-ink-dim transition-colors hover:text-ink disabled:cursor-default disabled:opacity-60"
        >
          <RefreshIcon size={11} className={checking ? "animate-spin" : ""} />
          {checking ? t("settings.about.checking") : t("settings.about.checkUpdate")}
        </button>
      </div>

      {selfFp ? (
        <div className="border-t border-hairline-soft pt-0.5" aria-label={t("settings.selfFootprint.title")}>
          {/* 自身开销 2×2 mono 网格（F19 自耗透明）：打开时拉一次不轮询，
              缺数显示「暂无」而不是 0%（S3）；口径与采样时间进 tooltip（S1） */}
          <div
            className="grid grid-cols-2 gap-x-4 gap-y-0.5 py-1"
            title={t("settings.selfFootprint.sampleNote")}
          >
            <div className="flex items-baseline justify-between gap-2">
              <span className="text-[9.5px] text-ink-faint" title={t("settings.selfFootprint.memHint")}>
                {t("settings.selfFootprint.mem")}
              </span>
              <span className="font-mono text-[10.5px] tabular-nums text-ink">{memText}</span>
            </div>
            <div className="flex items-baseline justify-between gap-2">
              <span className="text-[9.5px] text-ink-faint">
                {t("settings.selfFootprint.cpu")}
              </span>
              <span className="font-mono text-[10.5px] tabular-nums text-ink">{cpu}</span>
            </div>
            <div className="flex items-baseline justify-between gap-2">
              <span className="text-[9.5px] text-ink-faint">{t("settings.selfFootprint.threads")}</span>
              <span className="font-mono text-[10.5px] tabular-nums text-ink">{threads}</span>
            </div>
            <div className="flex items-baseline justify-between gap-2">
              <span className="text-[9.5px] text-ink-faint">{t("settings.selfFootprint.db")}</span>
              <span className="font-mono text-[10.5px] tabular-nums text-ink">{db}</span>
            </div>
          </div>
          <p className="pb-0.5 text-[9px] leading-3.5 text-ink-faint">
            {t("settings.selfFootprint.at", { clock: formatClock(selfFp.sampleAt) })} · {t("settings.selfFootprint.noPromise")}
          </p>
        </div>
      ) : null}
    </SettingsGroup>
  );
}
