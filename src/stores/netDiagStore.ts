import { create } from "zustand";

import { devNetDiag, devSpeedResult } from "../lib/devFixture";
import { tGlobal } from "../i18n/store";
import { isTauri, netDiagCapability, netDiagCopyReport, netDiagRun, netDiagSpeedRun } from "../lib/ipc";
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
  /** 复制失败反馈（同 copied 的闪现机制；不再是「点了没反应」） */
  copyFailed: boolean;
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
  copyFailed: false,

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
          note: tGlobal("net.diag.failNote", { err: String(e) }),
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
      // dev 日志保持英文：它不是 UI 文案，也不该出现在门禁的中文扫描里
      console.warn("[net-diag] speed test failed", e);
    } finally {
      set({ speedRunning: false });
    }
  },

  copyReport: async () => {
    // Tauri：报告由 Rust 生成并**原生写入剪贴板**。前端 Web 剪贴板在自定义协议下
    // 常被拒，且 execCommand 兜底要求同步手势（一次 IPC 往返后手势已失效）——
    // 这就是「点了没反应」的根因，所以不在 WebView 里写。
    if (isTauri()) {
      try {
        await netDiagCopyReport();
        set({ copied: true, copyFailed: false });
      } catch (e) {
        console.warn("[net-diag] copy report failed", e);
        set({ copied: false, copyFailed: true });
      }
      return;
    }
    // 浏览器开发态（无 Tauri）：仍走 Web 剪贴板
    const ok = await copyText(buildDevReport());
    set({ copied: ok, copyFailed: !ok });
  },

  resetCopied: () => set({ copied: false, copyFailed: false }),
}));

function buildDevReport(): string {
  // 开发态占位：真正的报告由 Rust 侧生成（report.rs）
  return "# Prune network diagnosis\n\n(dev sample)";
}
