import { listen } from "@tauri-apps/api/event";
import { create } from "zustand";

import { devNetSpeed } from "../lib/devFixture";
import { isTauri, netSpeedSetActive, netSpeedSnapshot } from "../lib/ipc";
import type { NetSpeedSample } from "../types/net";

/**
 * 网速 store（M10）。
 *
 * 与端口 store 同一纪律：采样只在「应用资源」tab 可见时进行。
 * 差异是网速的采样频率高（1 s）但载荷小（只有 pid → 速率），
 * 所以不做手动刷新按钮——数字自己会跳。
 */
interface NetSpeedStore {
  /**
   * 最新样本。null 有两种含义，UI 必须区分：
   * - `status === "warmup"` 还没攒出基线 → 「采样中…」
   * - `status === "unavailable"` 读不到 → 「不可读 + 原因」
   */
  sample: NetSpeedSample | null;
  setActive: (active: boolean) => Promise<void>;
  init: () => Promise<void>;
}

/** StrictMode 下 effect 跑两次：订阅只做一次 */
let subscribed = false;

export const useNetSpeedStore = create<NetSpeedStore>((set) => ({
  sample: null,

  init: async () => {
    if (subscribed) return;
    subscribed = true;

    if (!isTauri()) {
      if (import.meta.env.DEV) set({ sample: devNetSpeed() });
      return;
    }

    try {
      const sample = await netSpeedSnapshot();
      if (sample) set({ sample });
    } catch {
      // 快照拉不到不影响后续事件：下一轮采样会推过来
    }

    await listen<NetSpeedSample>("net://speed", (e) => set({ sample: e.payload }));
  },

  /** 可见性门控：切出 tab 必须关掉，否则 1 s 一次 nettop 会一直跑 */
  setActive: async (active) => {
    // 重新进入时先清掉上一次的读数：否则会有 1 s 时间窗口显示着「离开前」的旧速率
    if (active) set({ sample: null });
    if (!isTauri()) return;
    try {
      await netSpeedSetActive(active);
    } catch {
      // 门控失败只影响耗电，不影响正确性：不打断用户
    }
  },
}));
