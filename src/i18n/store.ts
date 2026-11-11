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
import { fetchLocale, LOCAL_PREF_KEY, fromTag, type LocaleState } from "./detect";
import { i18nFor, type I18n } from "./index";
import type { Locale, LocalePref, LocaleSource } from "./types";

interface LocaleStore {
  locale: Locale;
  source: LocaleSource;
  /** 系统首选语言原始 tag；浏览器 dev / 后端读不到时为 null（设置页据此写「无法读取」） */
  system: string | null;
  /** 此刻选「跟随系统」会落到哪个（manual 时与 locale 无关，但仍有意义） */
  autoResolved: Locale;
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
  system: null,
  autoResolved: "zh",
  ready: false,
  error: null,

  init: async () => {
    if (get().ready) return;
    if (!bootPromise) {
      bootPromise = (async () => {
        const st = await fetchLocale();
        // 显式映射而不是 `...st`：`current` 是后端字段名，前端不读它，
        // 展开进来只会让 store 里多一个没人知道该不该用的键
        set({
          locale: st.current,
          source: st.source,
          system: st.system ?? null,
          autoResolved: st.autoResolved ?? st.current,
          ready: true,
        });
        if (isTauri()) {
          // 事件：切语言后所有窗口同步。失败（无监听者）不算错误。
          // 新字段必须一并写入：漏了 system 的话，切完语言副标题会停在旧值
          await listen<LocaleState>("locale://changed", (e) =>
            set({
              locale: e.payload.current,
              source: e.payload.source,
              system: e.payload.system ?? null,
              autoResolved: e.payload.autoResolved ?? e.payload.current,
            }),
          ).catch(() => undefined);
        }
      })();
    }
    await bootPromise;
  },

  setLocale: async (next) => {
    set({ error: null });
    if (!isTauri()) {
      // 浏览器 dev：localStorage 兜底，auto 按 navigator 解析。
      // 没有真实系统语言可读 → system 恒 null，autoResolved 用 navigator 的结果
      if (next === "auto") localStorage.removeItem(LOCAL_PREF_KEY);
      else localStorage.setItem(LOCAL_PREF_KEY, next);
      const resolved = fromTag(navigator.language || "en");
      set({
        locale: next === "auto" ? resolved : next,
        source: next === "auto" ? "auto" : "manual",
        system: null,
        autoResolved: resolved,
      });
      return;
    }
    try {
      const st = await invoke<LocaleState>("locale_set", {
        locale: next,
      });
      set({
        locale: st.current,
        source: st.source,
        system: st.system ?? null,
        autoResolved: st.autoResolved ?? st.current,
      });
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
 *
 * 类型直接取 `I18n["t"]`（而不是 `Parameters<I18n["t"]>`）：后者在重载函数上
 * 只会取最后一个签名，带参 key 全被当成 `WithParamKey`，`tGlobal("…noParam")`
 * 与 `tGlobal("…withParam", {err})` 都会报错。
 */
export const tGlobal: I18n["t"] = ((key: string, params?: Record<string, string | number>) =>
  i18nFor(currentLocale()).t(key as never, params as never)) as I18n["t"];


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
