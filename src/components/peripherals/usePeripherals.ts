import { useEffect, useState } from "react";

import { epSnapshot, epSetActive, hwFlags, isTauri } from "../../lib/ipc";
import type { EpSnapshot } from "../../types/peripheral";

/**
 * 外设读数 hook（m21，docs/m21-peripherals-plan.md §三）。
 *
 * 纪律（对齐 useHardwareReadouts / ps_sampler）：
 * - **失败即隐藏**：快照 null / 源时间戳缺失就按空处理，绝不给 0 占位；
 * - **谁可见谁负责**：挂载时 set_active(true)，卸载时 false——后端门控循环
 *   随之停止采样，常驻开销 0；
 * - 快照轮询 15 s：后端 15 s 一轮，前端同频轮询只是取最新值（纯内存读）。
 */
export function usePeripherals(): {
  /** null = 未采集（整页空态）；有值时三源各自按 *_at / 数组判断渲染 */
  snapshot: EpSnapshot | null;
  enabled: boolean;
} {
  const [snapshot, setSnapshot] = useState<EpSnapshot | null>(null);
  const [flags, setFlags] = useState<Map<string, boolean> | null>(null);

  const enabled = flags?.get("hw.periph.enabled") ?? true;

  useEffect(() => {
    if (!isTauri()) return;
    hwFlags()
      .then((fs) => setFlags(new Map(fs.map((f) => [f.key, f.enabled]))))
      .catch(() => setFlags(new Map()));
  }, []);

  useEffect(() => {
    if (!isTauri() || !enabled) return;
    void epSetActive(true);

    const tick = () => {
      void epSnapshot().then(setSnapshot).catch(() => setSnapshot(null));
    };
    tick();
    const timer = setInterval(tick, 15_000);
    return () => {
      clearInterval(timer);
      void epSetActive(false);
    };
  }, [enabled]);

  return { snapshot, enabled };
}
