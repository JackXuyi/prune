import { listen } from "@tauri-apps/api/event";
import { create } from "zustand";

import { devNetHistory, devNetTotal, devTrafficDays } from "../lib/devFixture";
import {
  isTauri,
  netTotalHistory,
  netTotalSetActive,
  netTotalSnapshot,
  netTrafficRecent,
} from "../lib/ipc";
import type { DayTraffic, NetTotalPoint, NetTotalSample } from "../types/net";

/**
 * 走势窗口（秒）。300 = 5 min，与 UI 上「近 5 min」的标注必须一致。
 * 后端环形缓冲比它大（900 点），留出余量是为了切 tab 回来时曲线不突然变短。
 */
export const HISTORY_WINDOW_SECS = 300;
/** 7 日柱的天数 */
export const TRAFFIC_DAYS = 7;
/** 前端环形缓冲上限（后端 900，这里留同样余量） */
const RING_CAP = 900;

interface NetTotalStore {
  /**
   * 最新一拍。null 有两种含义，UI 必须区分：
   * - `status === "warmup"`：还没攒出基线 → 「采样中…」
   * - `status === "unavailable"`：读不到 → 「不可读 + 原因」
   */
  sample: NetTotalSample | null;
  /** 走势点（升序，按 at 去重） */
  history: NetTotalPoint[];
  /** 近 7 天每日流量（只含有记录的日子） */
  days: DayTraffic[];
  /** 头部「刷新」按钮的 loading（只等 IPC，不等采样） */
  refreshing: boolean;
  /** 首屏订阅 + 取快照/历史/7 日；重复调用无副作用 */
  init: () => Promise<void>;
  /** 手动重取快照 + 历史 + 7 日（不打断采样） */
  refresh: () => Promise<void>;
  /** 网络 tab 可见时提速到 1 s；切走降回 2 s */
  setActive: (active: boolean) => Promise<void>;
}

/** StrictMode 下 effect 跑两次：订阅只做一次 */
let subscribed = false;

/** 按 `at` 去重后追加（同一秒重复推送不制造两个点） */
function appendPoint(points: NetTotalPoint[], p: NetTotalPoint): NetTotalPoint[] {
  const last = points[points.length - 1];
  if (last && last.at === p.at) {
    return [...points.slice(0, -1), p];
  }
  const next = [...points, p];
  return next.length > RING_CAP ? next.slice(next.length - RING_CAP) : next;
}

export const useNetTotalStore = create<NetTotalStore>((set) => ({
  sample: null,
  history: [],
  days: [],
  refreshing: false,

  init: async () => {
    if (subscribed) return;
    subscribed = true;

    if (!isTauri()) {
      if (import.meta.env.DEV) {
        set({ sample: devNetTotal(), history: devNetHistory(), days: devTrafficDays() });
      }
      return;
    }

    try {
      const [sample, history, days] = await Promise.all([
        netTotalSnapshot(),
        netTotalHistory(HISTORY_WINDOW_SECS),
        netTrafficRecent(TRAFFIC_DAYS),
      ]);
      set({ sample, history, days });
    } catch {
      // 快照拉不到不影响后续事件：下一拍会推过来
    }

    await listen<NetTotalSample>("net://total", (e) => {
      set((s) => ({
        sample: e.payload,
        history: appendPoint(s.history, {
          at: e.payload.at,
          downBps: e.payload.downBps,
          upBps: e.payload.upBps,
        }),
      }));
    });
  },

  refresh: async () => {
    if (!isTauri()) {
      if (import.meta.env.DEV) {
        set({ sample: devNetTotal(), history: devNetHistory(), days: devTrafficDays() });
      }
      return;
    }
    set({ refreshing: true });
    try {
      const [sample, history, days] = await Promise.all([
        netTotalSnapshot(),
        netTotalHistory(HISTORY_WINDOW_SECS),
        netTrafficRecent(TRAFFIC_DAYS),
      ]);
      set({ sample, history, days });
    } catch {
      // 失败保持原值：宁可显示稍旧的数字，也不要清成「0」
    } finally {
      set({ refreshing: false });
    }
  },

  /** 只改节拍，不清读数：切走再回来时数字不该先归零 */
  setActive: async (active) => {
    if (!isTauri()) return;
    try {
      await netTotalSetActive(active);
    } catch {
      // 门控失败只影响耗电，不影响正确性：不打断用户
    }
  },
}));

/** 供组件直接读取当前样本（避免在 render 里多订阅一次） */
export function currentTotalSample(): NetTotalSample | null {
  return useNetTotalStore.getState().sample;
}

/** 显式触发一次刷新（头部按钮用） */
export function refreshNetTotal(): Promise<void> {
  return useNetTotalStore.getState().refresh();
}

/** 判断是否需要展示「按应用」区块的降级文案 */
export function perAppUnavailableReason(sample: NetTotalSample | null): string | null {
  if (!sample) return "采样中…";
  if (sample.status === "unavailable") return sample.note ?? "系统口径不可读";
  return null;
}
