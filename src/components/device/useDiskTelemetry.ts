import { useEffect, useState } from "react";

import { diskIoSetActive, diskIoSnapshot, diskSmart, isTauri } from "../../lib/ipc";
import type { DiskIoSnapshot, DiskSmart } from "../../types/hardware";

export interface DiskTelemetry {
  /** I/O 快照（sysinfo bsd 级差分）；读不到为 null（调用方按「失败即隐藏」处理） */
  io: DiskIoSnapshot | null;
  /** SMART 状态灯（diskutil 解析，后端 1 h 缓存）；空数组 = 暂不可读 */
  smart: DiskSmart[];
}

/**
 * 磁盘遥测 hook（f26 G-04/G-03 的采样逻辑，从 DiskHardwareStrip 抽出）：
 * 设备页 Ribbon 与磁盘磁贴共用同一套门控与节奏。
 *
 * 纪律：
 * - **谁可见谁负责**：挂载时 diskIoSetActive(true)，卸载时 false——后端门控
 *   循环随之停止，常驻开销 0；
 * - I/O 前端 2.5 s 轮询快照（后端 2 s 一轮差分，纯内存读）；
 * - SMART 进页面拉一次（后端 1 h 缓存）；
 * - **失败即隐藏**：io 为 null / smart 为空数组时由调用方隐藏对应读数，
 *   绝不给 0 占位。
 */
export function useDiskTelemetry(): DiskTelemetry {
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

  return { io, smart: smart ?? [] };
}

/** KB/s → 人读速率（与 net 侧 formatRate 同形，单位是字节速率） */
export function formatDiskRate(kbps: number): string {
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
