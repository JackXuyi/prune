/**
 * 设置页「关于」卡片（docs/version-info-plan.md）。
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
 */
import { useCallback, useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";

import { useI18n } from "../i18n/store";
import { isTauri, updateCheckDialog } from "../lib/ipc";
import { RefreshIcon } from "./icons";

export function AboutSection() {
  const { t } = useI18n();
  const [version, setVersion] = useState<string | null>(null);
  const [checking, setChecking] = useState(false);

  useEffect(() => {
    if (!isTauri()) return;
    void getVersion()
      .then(setVersion)
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
