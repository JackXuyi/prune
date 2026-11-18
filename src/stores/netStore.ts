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

// ---------------------------------------------------------------- 抖动抑制（EWMA）
//
// nettop 的按进程字节是**突发性**的（TCP 窗口 / 接收缓冲一次性灌入），
// 1 s 差分在「满速 ↔ 0」之间来回跳：UI 上表现为数字抖动、条形抖动、
// 排名洗牌（`> 0` 过滤还让行整行闪现/消失）。
//
// 这里对**展示口径**做指数平滑：`smooth = α·raw + (1−α)·prev`。
// 只动 per-pid 速率及其求和；差分基线、今日流量、历史累计都在后端别的口径里，不受影响。

/** 平滑系数。1/3 ≈ 3 拍收敛到 90%+：抖动被压平，新流量 3 s 内也能跟上来 */
const EWMA_ALPHA = 1 / 3;
/** 平滑值低于该阈值视为已归零并移除：不让死进程的残留永远挂在表里 */
const EWMA_FLOOR_BPS = 0.5;

/** pid → 平滑后的速率。模块级：订阅只做一次，状态跟着订阅走 */
let smoothPerPid = new Map<string, { downBps: number; upBps: number }>();

/** 清空平滑状态（切走 tab / 不可读时：断点之后不该带着旧惯性） */
function resetSmoothing() {
  smoothPerPid = new Map();
}

/**
 * 对一拍样本做 EWMA 平滑。纪律：
 * - 非 `ok`（warmup / unavailable）直接透传并清状态——「采样中」不该有速率残留
 * - 本拍没出现的 pid 按 0 衰减而不是移除：该秒确实没流量，速率该滑向 0 而不是断崖
 * - 首次观测直接取原始值（fast attack）：新流量不该被 3 倍低估
 */
function smoothSample(raw: NetSpeedSample): NetSpeedSample {
  if (raw.status !== "ok") {
    resetSmoothing();
    return raw;
  }
  const perPid: Record<string, { downBps: number; upBps: number }> = {};
  const next = new Map<string, { downBps: number; upBps: number }>();
  const keys = new Set([...Object.keys(raw.perPid), ...smoothPerPid.keys()]);
  let totalDown = 0;
  let totalUp = 0;
  for (const key of keys) {
    const prev = smoothPerPid.get(key);
    const cur = raw.perPid[key];
    const s = prev
      ? {
          downBps: EWMA_ALPHA * (cur?.downBps ?? 0) + (1 - EWMA_ALPHA) * prev.downBps,
          upBps: EWMA_ALPHA * (cur?.upBps ?? 0) + (1 - EWMA_ALPHA) * prev.upBps,
        }
      : // 首次观测不折半：平滑是为了稳，不是为了瞒
        { downBps: cur?.downBps ?? 0, upBps: cur?.upBps ?? 0 };
    if (s.downBps + s.upBps < EWMA_FLOOR_BPS) continue;
    next.set(key, s);
    perPid[key] = s;
    totalDown += s.downBps;
    totalUp += s.upBps;
  }
  smoothPerPid = next;
  return { ...raw, perPid, totalDownBps: totalDown, totalUpBps: totalUp };
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
      if (sample) set({ sample: smoothSample(sample) });
    } catch {
      // 快照拉不到不影响后续事件：下一轮采样会推过来
    }

    await listen<NetSpeedSample>("net://speed", (e) => set({ sample: smoothSample(e.payload) }));
  },

  /** 可见性门控：切出 tab 必须关掉，否则 1 s 一次 nettop 会一直跑 */
  setActive: async (active) => {
    // 重新进入时先清掉上一次的读数：否则会有 1 s 时间窗口显示着「离开前」的旧速率
    if (active) {
      resetSmoothing();
      set({ sample: null });
    }
    if (!isTauri()) return;
    try {
      await netSpeedSetActive(active);
    } catch {
      // 门控失败只影响耗电，不影响正确性：不打断用户
    }
  },
}));
