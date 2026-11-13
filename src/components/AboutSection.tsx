/**
 * 设置页「关于」卡片（docs/version-info-plan.md）。
 *
 * 版本号来自 getVersion()（@tauri-apps/api/app）—— 运行时读 tauri.conf.json
 * 的 version，与 `pnpm bump` 维护的四处版本号天然同源，零 Rust command。
 *
 * 降级：浏览器 dev（isTauri() = false）不调 API；Tauri 下获取失败也静默
 * 降级 —— 两种情况都显示 "dev"，不渲染错误态（版本号拿不到不算故障）。
 * 只在挂载时取一次：应用生命周期内版本不会变。
 */
import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";

import { useI18n } from "../i18n/store";
import { isTauri } from "../lib/ipc";

export function AboutSection() {
  const { t } = useI18n();
  const [version, setVersion] = useState<string | null>(null);

  useEffect(() => {
    if (!isTauri()) return;
    void getVersion()
      .then(setVersion)
      .catch(() => {});
  }, []);

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
    </section>
  );
}
