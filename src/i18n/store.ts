/**
 * 语言 store（docs/i18n-spec.md §2.2 / §2.4）。
 *
 * 三条纪律：
 * 1. `init()` 先 resolve 再 render（main.tsx 里 await），避免「中文闪一下变英文」
 * 2. `locale://changed` 回调**只更新 state**，不回写、不再 invoke —— 事件是通知不是命令
 * 3. 组件只用 `useI18n()`，不要直接 `i18nFor()`（绕过订阅会切语言不重渲染）
 */
import { useEffect, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { create } from "zustand";

import { isTauri } from "../lib/ipc";
import { fetchLocale, LOCAL_PREF_KEY, fromTag } from "./detect";
import { i18nFor, type I18n } from "./index";
import type { Locale, LocalePref, LocaleSource } from "./types";

interface LocaleStore {
  locale: Locale;
  source: LocaleSource;
  ready: boolean;
  /** 最近一次切换失败的原因（设置页 toast 用） */
  error: string | null;
  init: () => Promise<void>;
  setLocale: (next: LocalePref) => Promise<void>;
}

/** StrictMode / 三窗口各自 init：模块级 promise 保证只跑一次真实探测 */
let bootPromise: Promise<void> | null = null;

export const useLocaleStore = create<LocaleStore>((set, get) => ({
  // 首帧先给 zh：catalog 的 source of truth 是中文，缺 key 时也回退中文；
  // 真正的语言在 boot() 里 resolve 完才 render，所以这个初值几乎不会被看到
  locale: "zh",
  source: "auto",
  ready: false,
  error: null,

  init: async () => {
    if (get().ready) return;
    if (!bootPromise) {
      bootPromise = (async () => {
        const st = await fetchLocale();
        set({ ...st, ready: true });
        if (isTauri()) {
          // 事件：切语言后所有窗口同步。失败（无监听者）不算错误
          await listen<{ current: Locale; source: LocaleSource }>(
            "locale://changed",
            (e) => set({ locale: e.payload.current, source: e.payload.source }),
          ).catch(() => undefined);
        }
      })();
    }
    await bootPromise;
  },

  setLocale: async (next) => {
    set({ error: null });
    if (!isTauri()) {
      // 浏览器 dev：localStorage 兜底，auto 按 navigator 解析
      if (next === "auto") localStorage.removeItem(LOCAL_PREF_KEY);
      else localStorage.setItem(LOCAL_PREF_KEY, next);
      const locale = next === "auto" ? fromTag(navigator.language || "en") : next;
      set({ locale, source: next === "auto" ? "auto" : "manual" });
      return;
    }
    try {
      const st = await invoke<{ current: Locale; source: LocaleSource }>("locale_set", {
        locale: next,
      });
      set({ locale: st.current, source: st.source });
    } catch (e) {
      // spec §1.5：写盘失败 → 界面回滚（store 没被改过，天然是旧值）+ 报错给 UI
      set({ error: String(e) });
      throw e;
    }
  },
}));

/** 组件入口：按当前 locale 取实例（记忆化，切语言才换引用） */
export function useI18n(): I18n {
  const locale = useLocaleStore((s) => s.locale);
  return useMemo(() => i18nFor(locale), [locale]);
}

/** 当前 locale（纯函数 / 非组件上下文用） */
export function currentLocale(): Locale {
  return useLocaleStore.getState().locale;
}

/**
 * 无 React 上下文的最小出口。**只允许 store 层造 toast / 错误文案时用**；
 * `src/lib/**` 的纯函数禁止 import（会造成「切语言后旧串不更新」，
 * 而且测试语言不可控）—— 门禁脚本会拦。
 */
export function tGlobal(...args: Parameters<I18n["t"]>) {
  return i18nFor(currentLocale()).t(...args);
}

/** 让组件在切语言时重渲染的最小钩子（只取 locale 引用） */
export function useLocale(): Locale {
  return useLocaleStore((s) => s.locale);
}

/** 挂一次 html lang，供 CSS / 屏幕阅读器用（运行时随 locale 变） */
export function useDocumentLang(): void {
  const locale = useLocale();
  useEffect(() => {
    document.documentElement.lang = locale === "zh" ? "zh-Hans" : "en";
  }, [locale]);
}
