import { listen } from "@tauri-apps/api/event";
import { create } from "zustand";

import { devDiskPreview, devDiskReport } from "../lib/devFixture";
import { diskReport, diskScan, isTauri, reclaimExecute, reclaimPreview } from "../lib/ipc";
import type { DiskReport, ReclaimPreview, ReclaimReport, ScanProgress } from "../types/disk";

interface DiskStore {
  report: DiskReport | null;
  progress: ScanProgress | null;
  preview: ReclaimPreview | null;
  /** dry-run 清单生成中：弹层先出、内容后到 */
  previewLoading: boolean;
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
  previewLoading: false,
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
      if (!isTauri() && import.meta.env.DEV) set({ preview: devDiskPreview(), previewLoading: false });
      return;
    }
    // 弹层先出：previewLoading 驱动骨架屏，清单生成后再填内容
    set({ busy: true, error: null, preview: null, previewLoading: true });
    try {
      const preview = await reclaimPreview(keys);
      set({ preview, previewLoading: false });
    } catch (e) {
      set({ error: String(e), previewLoading: false });
    } finally {
      set({ busy: false });
    }
  },

  clearPreview: () => set({ preview: null, previewLoading: false }),

  execute: async (confirmed) => {
    const preview = get().preview;
    if (!preview || get().busy) return;
    // 仅 DEV fixture 允许在浏览器里模拟清理结果；生产构建在非 Tauri 环境绝不伪造
    // 「清理成功」摘要——放行到 reclaimExecute 抛错，走下方 catch 诚实报错
    if (!isTauri() && import.meta.env.DEV) {
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
