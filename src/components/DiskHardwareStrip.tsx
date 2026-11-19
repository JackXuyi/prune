import { useEffect, useState } from "react";

import { useI18n } from "../i18n/store";
import { diskIoSetActive, diskIoSnapshot, diskSmart, isTauri } from "../lib/ipc";
import type { DiskIoSnapshot, DiskSmart } from "../types/hardware";

/**
 * 存储 tab 硬件读数条（f26 G-04 磁盘 I/O + G-03 SMART 状态灯）。
 *
 * 纪律：
 * - **谁可见谁负责**：挂载时 diskIoSetActive(true)，卸载时 false；
 * - I/O 前端 2.5 s 轮询快照（后端 2 s 一轮差分，纯内存读）；
 * - SMART 进 tab 拉一次（后端 1 h 缓存），状态灯三态：已验证 / 警告 / 不支持；
 * - 口径诚实：iostat/sysinfo 的速率是**每块物理盘**的读+写分列（方案升级后
 *   有真拆分），多卷共享物理盘时不双计。
 */
export function DiskHardwareStrip() {
  const { t } = useI18n();
  const [io, setIo] = useState<DiskIoSnapshot | null>(null);
  const [smart, setSmart] = useState<DiskSmart[] | null>(null);

  useEffect(() => {
    if (!isTauri()) return;
    void diskIoSetActive(true);
    void diskSmart()
      .then((r) => setSmart(r))
      .catch(() => setSmart(null));
    const timer = setInterval(() => {
      void diskIoSnapshot().then(setIo).catch(() => setIo(null));
    }, 2_500);
    void diskIoSnapshot().then(setIo).catch(() => setIo(null));
    return () => {
      clearInterval(timer);
      void diskIoSetActive(false);
    };
  }, []);

  const smartRows = smart ?? [];
  if (io == null && smartRows.length === 0) return null;

  return (
    <div className="mt-2.5 rounded-card border border-hairline-soft bg-surface-raised px-4 py-2.5">
      <div className="flex items-baseline justify-between gap-3">
        <span className="text-[11px] font-medium tracking-wide text-ink-dim">
          {t("disk.hw.title")}
        </span>
        <span className="text-[9px] text-ink-faint">{t("disk.hw.hint")}</span>
      </div>

      <div className="mt-1.5 space-y-0.5">
        {(io?.disks ?? []).map((d) => (
          <div key={d.name} className="flex items-center gap-2" title={d.name}>
            <span className="w-[64px] shrink-0 font-mono text-[11px] text-ink">{d.name}</span>
            <span className="w-[88px] shrink-0 font-mono text-[11px] tabular-nums text-info">
              ↓ {fmtKbs(d.readKbPerSec)}
            </span>
            <span className="w-[88px] shrink-0 font-mono text-[11px] tabular-nums text-accent">
              ↑ {fmtKbs(d.writeKbPerSec)}
            </span>
            {smartRows
              .filter((s) => s.bsd === d.name)
              .map((s) => (
                <span
                  key={s.bsd}
                  className={`shrink-0 rounded-full px-1.5 py-px text-[9px] font-medium ${
                    s.status === "verified"
                      ? "bg-accent/12 text-accent"
                      : s.status === "failing"
                        ? "bg-danger/12 text-danger"
                        : "bg-chip text-ink-faint"
                  }`}
                >
                  SMART {t(`disk.hw.smart.${s.status}` as const)}
                </span>
              ))}
          </div>
        ))}
        {io == null && smartRows.length > 0
          ? smartRows.map((s) => (
              <div key={s.bsd} className="flex items-center gap-2">
                <span className="w-[64px] shrink-0 font-mono text-[11px] text-ink">{s.bsd}</span>
                <span
                  className={`shrink-0 rounded-full px-1.5 py-px text-[9px] font-medium ${
                    s.status === "verified"
                      ? "bg-accent/12 text-accent"
                      : s.status === "failing"
                        ? "bg-danger/12 text-danger"
                        : "bg-chip text-ink-faint"
                  }`}
                >
                  SMART {t(`disk.hw.smart.${s.status}` as const)}
                </span>
              </div>
            ))
          : null}
        {io != null && io.disks.length === 0 ? (
          <p className="text-[10.5px] text-ink-faint">{t("disk.hw.ioUnavailable")}</p>
        ) : null}
      </div>
    </div>
  );
}

/** KB/s → 人读格式（与 net 侧 splitSpeed 同形，但单位是字节速率） */
function fmtKbs(kbps: number): string {
  const bps = Math.max(0, kbps) * 1024;
  if (bps < 1024) return `${bps.toFixed(0)} B/s`;
  const units = ["KB/s", "MB/s", "GB/s"];
  let v = bps / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i += 1;
  }
  return `${v >= 100 ? v.toFixed(0) : v.toFixed(1)} ${units[i]}`;
}
