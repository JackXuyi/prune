import { create } from "zustand";

import { devNetDiag, devSpeedResult } from "../lib/devFixture";
import { isTauri, netDiagCapability, netDiagReport, netDiagRun, netDiagSpeedRun } from "../lib/ipc";
import type { NetDiag, NetDiagCapability, SpeedResult } from "../types/net";

/**
 * 网络诊断 store（net-diag-plan §4 / D2）。
 *
 * 与 netTotalStore 的分工不同：诊断是**手动触发的短任务**，没有常驻采样、
 * 没有事件订阅、不落库——每轮 ~5 s，测速 ~30 s，都是用户点了才跑。
 * capability 决定测速卡是否渲染（Windows 上后端就是 false），不硬编码平台。
 */
interface NetDiagStore {
  /** 最近一轮诊断结果；null = 没跑过（idle 文案） */
  diag: NetDiag | null;
  /** 整轮诊断进行中（禁用重复点击） */
  running: boolean;
  /** 最近一次测速结果；null = 没跑过 */
  speed: SpeedResult | null;
  /** 测速进行中（~30 s，禁用重复点击） */
  speedRunning: boolean;
  /** 后端能力；null = 还没拉到（测速卡先不渲染，拉到后再显隐） */
  capability: NetDiagCapability | null;
  /** 报告复制反馈（true 只闪一次，由组件在 timeout 后复位） */
  copied: boolean;
  /** 取 capability（进入网络 tab 时调一次） */
  init: () => Promise<void>;
  run: () => Promise<void>;
  runSpeed: () => Promise<void>;
  copyReport: () => Promise<void>;
  resetCopied: () => void;
}

/** 剪贴板：navigator.clipboard 不可用时退回 execCommand（Tauri WebView 兼容） */
async function copyText(text: string): Promise<boolean> {
  try {
    if (navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(text);
      return true;
    }
  } catch {
    // 落到下面的兜底
  }
  try {
    const ta = document.createElement("textarea");
    ta.value = text;
    ta.style.position = "fixed";
    ta.style.opacity = "0";
    document.body.appendChild(ta);
    ta.select();
    const ok = document.execCommand("copy");
    document.body.removeChild(ta);
    return ok;
  } catch {
    return false;
  }
}

export const useNetDiagStore = create<NetDiagStore>((set, get) => ({
  diag: null,
  running: false,
  speed: null,
  speedRunning: false,
  capability: null,
  copied: false,

  init: async () => {
    if (!isTauri()) {
      if (import.meta.env.DEV && get().capability === null) {
        set({ capability: { speedTest: true }, diag: devNetDiag(), speed: devSpeedResult() });
      }
      return;
    }
    if (get().capability) return; // capability 不变，进 tab 只取一次
    try {
      set({ capability: await netDiagCapability() });
    } catch {
      // 拉不到 capability：测速卡不渲染，诊断卡照常
    }
  },

  run: async () => {
    if (get().running) return; // running 态禁用重复点击（§7）
    set({ running: true });
    try {
      const diag = isTauri() ? await netDiagRun() : devNetDiag();
      set({ diag });
    } catch (e) {
      // 整轮调度失败也要可解释，不显示编造数字
      set({
        diag: {
          at: Math.floor(Date.now() / 1000),
          status: "failed",
          note: `诊断失败：${String(e)}`,
          dns: null,
          lan: null,
          wan: null,
          jitterMs: null,
          gateway: null,
          captive: null,
          verdict: null,
          ssid: null,
        },
      });
    } finally {
      set({ running: false });
    }
  },

  runSpeed: async () => {
    if (get().speedRunning) return;
    set({ speedRunning: true });
    try {
      const speed = isTauri() ? await netDiagSpeedRun() : devSpeedResult();
      set({ speed });
    } catch (e) {
      // 测速失败保持原值：宁可显示稍旧的读数，也不清成 0
      console.warn("[net-diag] 测速失败", e);
    } finally {
      set({ speedRunning: false });
    }
  },

  copyReport: async () => {
    try {
      const text = isTauri() ? await netDiagReport() : buildDevReport();
      const ok = await copyText(text);
      set({ copied: ok });
    } catch {
      set({ copied: false });
    }
  },

  resetCopied: () => set({ copied: false }),
}));

function buildDevReport(): string {
  return "# Prune 网络诊断报告\n\n（开发态示例）";
}
