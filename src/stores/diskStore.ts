import { listen } from "@tauri-apps/api/event";
import { create } from "zustand";

import { devDiskPreview, devDiskReport } from "../lib/devFixture";
import { diskReport, diskScan, isTauri, reclaimExecute, reclaimPreview } from "../lib/ipc";
import type { DiskReport, ReclaimPreview, ReclaimReport, ScanProgress } from "../types/disk";

interface DiskStore {
  report: DiskReport | null;
  progress: ScanProgress | null;
  preview: ReclaimPreview | null;
  /** 扫描中（按钮禁用 + 进度条） */
  scanning: boolean;
  /** dry-run / 执行中 */
  busy: boolean;
  error: string | null;
  /** 最近一次清理结果（成功后展示摘要） */
  lastReclaimed: ReclaimReport | null;
  init: () => Promise<void>;
  refresh: () => Promise<void>;
  loadPreview: (keys: string[]) => Promise<void>;
  clearPreview: () => void;
  execute: (confirmed: boolean) => Promise<void>;
  clearReclaimed: () => void;
}

/** StrictMode 下 effect 跑两次：初始化与订阅只做一次 */
let initStarted = false;
let subscribed = false;

export const useDiskStore = create<DiskStore>((set, get) => ({
  report: null,
  progress: null,
  preview: null,
  scanning: false,
  busy: false,
  error: null,
  lastReclaimed: null,

  init: async () => {
    if (initStarted) return;
    initStarted = true;

    if (!isTauri()) {
      if (import.meta.env.DEV) {
        set({ report: devDiskReport(), scanning: false });
      } else {
        set({ error: "需要在 Prune 桌面端内运行（pnpm tauri dev）" });
      }
      return;
    }

    try {
      const report = await diskReport();
      set({ report });
    } catch (e) {
      set({ error: String(e) });
    }

    if (subscribed) return;
    subscribed = true;

    // 扫描进度：只用于进度条，不参与业务判断
    await listen<ScanProgress>("disk://progress", (e) => set({ progress: e.payload }));
    // 扫描/清理完成：直接换报告（Rust 侧已完成装配）
    await listen<DiskReport>("disk://done", (e) =>
      set({ report: e.payload, scanning: false, progress: null }),
    );
  },

  refresh: async () => {
    if (get().scanning || !isTauri()) return;
    set({ scanning: true, error: null });
    try {
      await diskScan();
      const report = await diskReport();
      set({ report });
    } catch (e) {
      set({ error: String(e) });
    } finally {
      set({ scanning: false, progress: null });
    }
  },

  loadPreview: async (keys) => {
    if (get().busy || !isTauri()) {
      if (!isTauri() && import.meta.env.DEV) set({ preview: devDiskPreview() });
      return;
    }
    set({ busy: true, error: null });
    try {
      const preview = await reclaimPreview(keys);
      set({ preview });
    } catch (e) {
      set({ error: String(e) });
    } finally {
      set({ busy: false });
    }
  },

  clearPreview: () => set({ preview: null }),

  execute: async (confirmed) => {
    const preview = get().preview;
    if (!preview || get().busy) return;
    if (!isTauri()) {
      set({ lastReclaimed: { moved: preview.items.length, bytes: preview.totalBytes, failed: [], skipped: 0 }, preview: null });
      return;
    }
    set({ busy: true, error: null });
    try {
      const result = await reclaimExecute(preview.items, confirmed);
      set({ lastReclaimed: result, preview: null });
      // 报告由 disk://done 事件刷新；这里再拉一次兜底
      const report = await diskReport();
      set({ report });
    } catch (e) {
      set({ error: String(e) });
    } finally {
      set({ busy: false });
    }
  },

  clearReclaimed: () => set({ lastReclaimed: null }),
}));
