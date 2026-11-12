import { listen } from "@tauri-apps/api/event";
import { create } from "zustand";

import { isTauri, lanScan, lanSnapshot } from "../lib/ipc";
import { i18nFor } from "../i18n";
import { currentLocale } from "../i18n/store";
import type { Locale } from "../i18n/types";
import type { LanProgress, LanSnapshot } from "../types/lan";
import { LAN_PHASE_KEY } from "../types/lan";

/**
 * 局域网设备 store（m18-lan-devices-spec.md §10 / §11）。
 *
 * 与 netDiagStore 同型的手动触发短任务：没有常驻采样、没有事件订阅（除进度）、
 * 不落库——用户点了「扫描局域网」才跑（~35s）。**进入分段绝不自动扫描**：
 * 这是 Prune 第一次引入 macOS 本地网络授权弹窗，不能让它在切 tab 时蹦出来。
 */
interface LanStore {
  /** 最近一轮快照；null = 还没扫过（空态文案） */
  snapshot: LanSnapshot | null;
  scanning: boolean;
  /** 扫描中进度（lan://progress 事件驱动；null = 阶段间空档，显示阶段文案即可） */
  progress: LanProgress | null;
  error: string | null;
  /** 初始化：拉缓存快照 + 订阅进度事件（幂等） */
  init: () => Promise<void>;
  scan: () => Promise<void>;
}

let initStarted = false;

export const useLanStore = create<LanStore>((set, get) => ({
  snapshot: null,
  scanning: false,
  progress: null,
  error: null,

  init: async () => {
    if (initStarted) return;
    initStarted = true;
    if (!isTauri()) return; // 浏览器 dev：空态即可，不给假设备
    // 缓存快照：拉到什么显示什么（null = 空态），失败不拦扫描
    lanSnapshot()
      .then((s) => {
        if (s && !get().scanning) set({ snapshot: s });
      })
      .catch(() => {});
    try {
      // 进度订阅跟随 App 生命周期，不需要退订（与 portStore 的事件订阅同型）
      await listen<LanProgress>("lan://progress", (ev) => {
        set({ progress: ev.payload });
      });
    } catch {
      // 订阅失败只影响进度条，扫描结果照常返回
    }
  },

  scan: async () => {
    if (get().scanning) return; // 门控在前端也挡一层（后端另有 AtomicBool）
    set({ scanning: true, error: null, progress: null });
    try {
      const snap = await lanScan();
      set({ snapshot: snap });
    } catch (e) {
      set({ error: String(e) });
    } finally {
      set({ scanning: false, progress: null });
    }
  },
}));

// 浏览器 dev 验收用：动态 import 拿到的是新模块实例，这里把页面真正用的这份挂到 window
if (import.meta.env.DEV) {
  (window as unknown as Record<string, unknown>).__useLanStore = useLanStore;
}

/**
 * 阶段文案：progress 事件驱动，阶段间空档显示最后已知阶段。
 *
 * `locale` 显式传入（默认取当前语言）：这个函数在 store 层也被调用，
 * 而 store 不订阅语言变化，所以组件侧必须把 `locale` 带进来才会跟着切。
 */
export function phaseText(p: LanProgress | null, locale: Locale = currentLocale()): string {
  const { t } = i18nFor(locale);
  if (!p) return t("lan.phase.preparing");
  const label = t(LAN_PHASE_KEY[p.phase]);
  if (p.phase === "probe" && p.total > 1) {
    return t("lan.phase.withProgress", { label, done: p.done, total: p.total });
  }
  return label;
}
