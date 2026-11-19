import { useEffect, useState } from "react";

import { useI18n } from "../../i18n/store";
import { diskSmart, isTauri } from "../../lib/ipc";
import type { DiskSmart } from "../../types/hardware";
import { WarnTriangleIcon } from "../icons";

/**
 * SMART 条件提醒带（storage-eul-mockup.html ①）：
 * 「已验证」是健康态——健康状态是噪音，**整条不渲染、不留白**；
 * 只有「警告 / 不支持」才出现。failing = danger，unsupported = 中性灰。
 *
 * SMART 进 tab 拉一次（后端 1 h 缓存）；读不到（非 macOS / 调用失败）也隐藏——
 * 读不到不是「有问题」，不制造假警报。
 */
export function SmartBand() {
  const { t } = useI18n();
  const [smart, setSmart] = useState<DiskSmart[] | null>(null);

  useEffect(() => {
    if (!isTauri()) return;
    let alive = true;
    void diskSmart()
      .then((r) => alive && setSmart(r))
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, []);

  if (!smart) return null;
  const abnormal = smart.filter((s) => s.status !== "verified");
  if (abnormal.length === 0) return null;

  const failing = abnormal.filter((s) => s.status === "failing");
  const tone = failing.length > 0 ? "danger" : "neutral";
  const names = abnormal.map((s) => s.bsd).join(" · ");

  return (
    <div
      className={
        tone === "danger"
          ? "flex h-[40px] shrink-0 items-center gap-2.5 rounded-[11px] border border-danger/30 bg-danger/[0.07] px-3.5 text-[10.5px] text-danger"
          : "flex h-[40px] shrink-0 items-center gap-2.5 rounded-[11px] border border-hairline-soft bg-surface-raised px-3.5 text-[10.5px] text-ink-dim"
      }
      title={t("disk.smartBand.tip", { names })}
    >
      <WarnTriangleIcon size={13} className="shrink-0" />
      <span className="min-w-0 flex-1 truncate">
        {failing.length > 0
          ? t("disk.smartBand.failing", { n: failing.length, names })
          : t("disk.smartBand.unsupported", { n: abnormal.length, names })}
      </span>
      <span className="shrink-0 font-mono text-[9.5px] tabular-nums text-ink-faint">
        {abnormal.map((s) => t(`disk.hw.smart.${s.status}` as const)).join(" · ")}
      </span>
    </div>
  );
}
