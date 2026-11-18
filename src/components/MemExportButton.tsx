/**
 * F20 内存报告导出按钮（docs/f20-memory-report-plan.md）。
 *
 * 挂在内存页家族列表头部。链路：invoke mem_export_csv → Rust 侧聚合 +
 * 原生 save 对话框（Rust DialogExt，无需 JS ACL）→ 写盘 → 返回路径。
 *
 * 两种「无事发生」必须区分呈现（f20 S1）：
 * - 返回 null = 用户取消保存 → 静默，不弹任何提示；
 * - 返回 Err（含「暂无序列数据」）→ toast 错误，指路「先跑一轮采样」。
 */
import { useCallback, useState } from "react";

import { useI18n } from "../i18n/store";
import { isTauri, memExportCsv } from "../lib/ipc";
import { toast } from "./primitives";

export function MemExportButton() {
  const { t } = useI18n();
  const [busy, setBusy] = useState(false);

  const run = useCallback(async () => {
    if (busy || !isTauri()) return;
    setBusy(true);
    try {
      const path = await memExportCsv();
      if (path) {
        // 只报路径不报体积——文件就在磁盘上，用户自己会打开
        toast(t("memory.exportCsv.done", { path }), "ok");
      }
      // null = 用户取消：无事发生
    } catch (e) {
      toast(`${t("memory.exportCsv.failed")} · ${String(e)}`, "error");
    } finally {
      setBusy(false);
    }
  }, [busy, t]);

  return (
    <button
      type="button"
      onClick={() => void run()}
      disabled={busy || !isTauri()}
      title={t("memory.exportCsv.hint")}
      className="shrink-0 text-[10px] text-accent transition-opacity hover:opacity-80 disabled:opacity-60"
    >
      {busy ? t("memory.exportCsv.exporting") : t("memory.exportCsv.action")}
    </button>
  );
}
