/**
 * 语言探测（docs/i18n-spec.md §1.5 降级链）。
 *
 * 为什么不在前端读 `navigator.language`：
 * WKWebView 的语言取自 App bundle 的 `CFBundleLocalizations`。没声明中文本地化时，
 * 系统是中文也会返回 `en-US` —— 「默认跟随系统」直接失效。所以 Tauri 环境下
 * **一律问 Rust**（它读的是 NSLocale / GetUserDefaultUILanguage），
 * 只有浏览器 dev 才退回 navigator。
 */
import { invoke } from "@tauri-apps/api/core";

import { isTauri } from "../lib/ipc";
import type { Locale, LocaleSource } from "./types";

export interface LocaleState {
  current: Locale;
  source: LocaleSource;
}

/** 浏览器 dev 下的 localStorage 键（B12：设置页语言段仍然可用） */
export const LOCAL_PREF_KEY = "prune.locale";

/** BCP-47 → Locale，与 Rust `Locale::from_tag` 同规则：只做前缀匹配 */
export function fromTag(tag: string): Locale {
  return tag.toLowerCase().startsWith("zh") ? "zh" : "en";
}

function navigatorLocale(): Locale {
  if (typeof navigator === "undefined") return "en";
  return fromTag(navigator.language || "en");
}

/** 启动时问一次当前语言。任何失败都降级为「跟随系统 / navigator」 */
export async function fetchLocale(): Promise<LocaleState> {
  if (isTauri()) {
    try {
      return await invoke<LocaleState>("locale_get");
    } catch (e) {
      console.warn("[i18n] locale_get failed, falling back to navigator:", e);
    }
  }
  // 浏览器 dev：读 localStorage 里手动选过的偏好
  const stored = typeof localStorage !== "undefined" ? localStorage.getItem(LOCAL_PREF_KEY) : null;
  if (stored === "zh" || stored === "en") return { current: stored, source: "manual" };
  return { current: navigatorLocale(), source: "auto" };
}
