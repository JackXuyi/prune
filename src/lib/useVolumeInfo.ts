import { useEffect, useState } from "react";

import { devVolume } from "./devFixture";
import { diskVolume, isTauri } from "./ipc";
import type { VolumeInfo } from "../types/disk";

/**
 * home 所在卷的容量（statvfs 直读，不依赖全盘扫描）。
 *
 * 从 M9 的 StatsBoard 私有 effect 提出来（M16）：主窗口统计页与菜单栏面板
 * 都要这一个读数。`statvfs` 是微秒级，两边各持一份比共享 store 更简单。
 *
 * `known=false` 表示「还没读到」——调用方要区分「读取中」与「读不到」，
 * 不能都显示成 0 或「不可读」。
 */
export function useVolumeInfo(refreshNonce: number): {
  volume: VolumeInfo | null;
  known: boolean;
} {
  const [volume, setVolume] = useState<VolumeInfo | null>(null);
  const [known, setKnown] = useState(false);

  useEffect(() => {
    let cancelled = false;
    const load = async () => {
      try {
        const v = !isTauri() ? (import.meta.env.DEV ? devVolume() : null) : await diskVolume();
        if (!cancelled) {
          setVolume(v);
          setKnown(true);
        }
      } catch {
        // statvfs 失败：保持「容量不可读」，不猜
        if (!cancelled) setKnown(true);
      }
    };
    void load();
    return () => {
      cancelled = true;
    };
  }, [refreshNonce]);

  return { volume, known };
}
