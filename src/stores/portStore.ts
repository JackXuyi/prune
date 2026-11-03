import { listen } from "@tauri-apps/api/event";
import { create } from "zustand";

import { devPortSnapshot } from "../lib/devFixture";
import { isTauri, portRefresh, portSetActive, portSnapshot } from "../lib/ipc";
import type { PortSnapshot } from "../types/net";

interface PortStore {
  /**
   * 最新一轮快照。null 有两种含义，UI 必须区分：
   * - `loading` = 正在扫（第一次）
   * - `!loading` = 还没扫过 / 平台不支持 → 显示原因，**绝不显示「没有端口占用」**
   */
  snapshot: PortSnapshot | null;
  loading: boolean;
  refreshing: boolean;
  error: string | null;
  /** 折叠层是否展开（默认折叠：70% 的内部 IPC 端口是噪音） */
  showFolded: boolean;
  /**
   * 从别处跳进来时要预填的搜索词（应用行点端口徽标 → 端口 tab 直接定位）。
   * 只消费一次，PortBoard 读完就清空，否则用户手动改了搜索又会被拉回去。
   */
  focusQuery: string;
  init: () => Promise<void>;
  refresh: () => Promise<void>;
  setActive: (active: boolean) => Promise<void>;
  toggleFolded: () => void;
  setFocus: (q: string) => void;
  clearFocus: () => void;
  /** 只读已有快照，不触发扫描：给「应用行端口徽标」用，避免为了一枚徽标在启动时跑 lsof */
  hydrate: () => Promise<void>;
}

/** StrictMode 下 effect 跑两次：初始化与订阅只做一次 */
let initStarted = false;
let subscribed = false;

export const usePortStore = create<PortStore>((set, get) => ({
  snapshot: null,
  loading: true,
  refreshing: false,
  error: null,
  showFolded: false,
  focusQuery: "",

  init: async () => {
    if (initStarted) return;
    initStarted = true;

    if (!isTauri()) {
      if (import.meta.env.DEV) {
        set({ snapshot: devPortSnapshot(), loading: false });
      } else {
        set({ loading: false, error: "需要在 Prune 桌面端内运行（pnpm tauri dev）" });
      }
      return;
    }

    try {
      const snapshot = await portSnapshot();
      set({ snapshot, loading: false });
      // 冷启动：一次都没有 → 立即扫一轮（用户已经站在端口 tab 前了）
      if (!snapshot) await get().refresh();
    } catch (e) {
      set({ loading: false, error: String(e) });
      return;
    }

    if (subscribed) return;
    subscribed = true;

    // 30 s 定时轮的产出：定时器只在 tab 可见时跑，事件到来说明用户正在看
    await listen<PortSnapshot>("port://snapshot", (e) =>
      set({ snapshot: e.payload, refreshing: false, loading: false }),
    );
  },

  refresh: async () => {
    if (get().refreshing || !isTauri()) return;
    set({ refreshing: true, error: null });
    try {
      await portRefresh();
      // 命中单飞闸门时 Rust 侧直接返回旧快照：这里必须重新拉，不能假设已变
      const snapshot = await portSnapshot();
      set({ snapshot });
    } catch (e) {
      set({ error: String(e) });
    } finally {
      set({ refreshing: false });
    }
  },

  /** 可见性门控：切出 tab 必须关掉，否则 30 s 定时器会一直跑 lsof（验收 §12-9） */
  setActive: async (active) => {
    if (!isTauri()) return;
    try {
      await portSetActive(active);
    } catch {
      // 门控失败只影响耗电，不影响正确性：不打断用户
    }
  },

  toggleFolded: () => set({ showFolded: !get().showFolded }),

  setFocus: (q) => set({ focusQuery: q }),

  clearFocus: () => set({ focusQuery: "" }),

  hydrate: async () => {
    if (get().snapshot || !isTauri()) return;
    try {
      const snapshot = await portSnapshot();
      if (snapshot) set({ snapshot, loading: false });
    } catch {
      // 只读探测失败就保持「还没扫」，不打扰用户
    }
  },
}));
