import { useEffect, useState } from "react";

import { cpuTempSnapshot, cpuTempSetActive, hwFlags, isTauri, powerSnapshot, powerSetActive } from "../../lib/ipc";
import type { CpuTempSnapshot, PowerSnapshot, PowerSource } from "../../types/hardware";

export interface HardwareReadouts {
  /** 主电池；台式机 / 读不到为 null（调用方按「失败即隐藏」处理） */
  battery: PowerSource | null;
  /** 蓝牙配件电量（键盘 / 鼠标 / 触控板…） */
  accessories: PowerSource[];
  /** CPU 温度；SMC 不可用为 null */
  temp: CpuTempSnapshot | null;
}

/**
 * 硬件读数 hook（f26 G-05/G-06/G-07 的采样逻辑，从 HardwareStrip 抽出）：
 * 状态带的电池段、设备磁贴与设备页读数条共用同一套门控与节奏。
 *
 * 纪律（方案 §四 D4/D6）：
 * - **失败即隐藏**：任一模块读不到就返回 null / 空数组，绝不给 0 占位；
 * - **谁可见谁负责**：挂载时 set_active(true)，卸载时 false——后端门控循环
 *   随之停止采样，常驻开销 0；多处同时挂载时各自开关，语义一致；
 * - 快照轮询 15 s：后端 30 s 一轮，前端半频轮询只是取最新值（纯内存读）。
 */
export function useHardwareReadouts(): HardwareReadouts {
  const [flags, setFlags] = useState<Map<string, boolean> | null>(null);
  const [power, setPower] = useState<PowerSnapshot | null>(null);
  const [temp, setTemp] = useState<CpuTempSnapshot | null>(null);

  const powerEnabled = flags?.get("hw.power.enabled") ?? true;
  const tempEnabled = flags?.get("hw.cpuTemp.enabled") ?? true;

  useEffect(() => {
    if (!isTauri()) return;
    hwFlags()
      .then((fs) => setFlags(new Map(fs.map((f) => [f.key, f.enabled]))))
      .catch(() => setFlags(new Map()));
  }, []);

  useEffect(() => {
    if (!isTauri()) return;
    const wanted = powerEnabled || tempEnabled;
    if (!wanted) return;
    void powerSetActive(powerEnabled);
    void cpuTempSetActive(tempEnabled);

    const tick = () => {
      if (powerEnabled) void powerSnapshot().then(setPower).catch(() => setPower(null));
      if (tempEnabled) void cpuTempSnapshot().then(setTemp).catch(() => setTemp(null));
    };
    tick();
    const timer = setInterval(tick, 15_000);
    return () => {
      clearInterval(timer);
      void powerSetActive(false);
      void cpuTempSetActive(false);
    };
  }, [powerEnabled, tempEnabled]);

  const battery = power?.sources.find((s) => s.kind === "battery" && s.percent != null) ?? null;
  const accessories = power?.sources.filter((s) => s.kind === "accessory" && s.percent != null) ?? [];
  return { battery, accessories, temp };
}
