import { useEffect, useState } from "react";

import { listen } from "@tauri-apps/api/event";
import { create } from "zustand";

import { tGlobal } from "../i18n/store";

import { isTauri, memHistory, memProcesses, memRefresh, memSnapshot } from "../lib/ipc";
import { devProcesses, devSnapshot } from "../lib/devFixture";
import type { MemSnapshot, Point, ProcMem } from "../types/mem";

interface MemStore {
  snapshot: MemSnapshot | null;
  processes: ProcMem[];
  loading: boolean;
  refreshing: boolean;
  error: string | null;
  selectedPid: number | null;
  select: (pid: number | null) => void;
  init: () => Promise<void>;
  refresh: () => Promise<void>;
}

/** StrictMode 下 effect 会跑两次，初始化与事件订阅必须只做一次 */
let initStarted = false;
let subscribed = false;

export const useMemStore = create<MemStore>((set, get) => ({
  snapshot: null,
  processes: [],
  loading: true,
  refreshing: false,
  error: null,
  selectedPid: null,

  select: (pid) => set({ selectedPid: pid }),

  init: async () => {
    if (initStarted) return;
    initStarted = true;

    if (!isTauri()) {
      if (import.meta.env.DEV) {
        // 开发态兜底：浏览器里也能迭代 UI（生产构建不含 fixture）
        set({ snapshot: devSnapshot(), processes: devProcesses(), loading: false });
      } else {
        set({
          loading: false,
          error: tGlobal("common.err.needsDesktop"),
        });
      }
      return;
    }

    try {
      const [snapshot, processes] = await Promise.all([memSnapshot(), memProcesses()]);
      set({ snapshot, processes, loading: false });
    } catch (e) {
      set({ loading: false, error: String(e) });
      return;
    }

    if (subscribed) return;
    subscribed = true;

    // 后台常采 → 事件推送；窗口数 ≤ 2，全局广播即可（方案 §6）
    await listen<MemSnapshot>("mem://snapshot", (e) => set({ snapshot: e.payload }));
    await listen<MemSnapshot>("mem://deep", (e) => {
      set({ snapshot: e.payload });
      // 深探后 footprint / swapped 变了，进程明细补一次拉取
      memProcesses()
        .then((processes) => set({ processes }))
        .catch(() => undefined);
    });
    await listen<number>("mem://select", (e) => set({ selectedPid: e.payload }));
  },

  refresh: async () => {
    if (get().refreshing || !isTauri()) return;
    set({ refreshing: true });
    try {
      await memRefresh();
      const [snapshot, processes] = await Promise.all([memSnapshot(), memProcesses()]);
      set({ snapshot, processes });
    } catch (e) {
      set({ error: String(e) });
    } finally {
      set({ refreshing: false });
    }
  },
}));

/** 某进程的序列（5 min 一个点；深探轮次完成后自动重取） */
export function useMemHistory(pid: number | null, minutes = 240): Point[] {
  const deepUpdatedAt = useMemStore((s) => s.snapshot?.deepUpdatedAt ?? null);
  const [points, setPoints] = useState<Point[]>([]);

  useEffect(() => {
    if (pid == null || !isTauri()) {
      setPoints([]);
      return;
    }
    let cancelled = false;
    memHistory(pid, minutes)
      .then((p) => {
        if (!cancelled) setPoints(p);
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [pid, minutes, deepUpdatedAt]);

  return points;
}
