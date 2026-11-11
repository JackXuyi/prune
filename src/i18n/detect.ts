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
  /** 系统首选语言原始 tag（如 `zh-Hans-CN`）；浏览器 dev 或后端读不到时为 null */
  system?: string | null;
  /** 若此刻选「跟随系统」会解析成哪个（Rust `Resolved::auto_resolved`） */
  autoResolved?: Locale;
}

/** 浏览器 dev 下的 localStorage 键（B12：设置页语言段仍然可用） */
export const LOCAL_PREF_KEY = "prune.locale";

/**
 * 繁体判定：`hant` / `-TW` / `-HK` / `-MO`，且 `hans` 优先。
 *
 * **与 Rust `locale.rs::is_traditional` 一字不差** —— Tauri 走 Rust，浏览器 dev
 * 走这里，只改一边会让 dev 与真机的 auto 结果打架（最容易在「以为已经修了」时漏掉）。
 *
 * 为什么外面也用它：`systemLangName` 要按同一个规则决定显示「简体中文」还是
 * 「繁體中文」，另写一套正则会出现「名字说繁体、解析成简体」的名实相悖。
 */
export function isTraditionalTag(tag: string): boolean {
  const low = tag.toLowerCase();
  if (low.includes("hans")) return false;
  return (
    low.includes("hant") ||
    low.includes("-tw") ||
    low.includes("-hk") ||
    low.includes("-mo")
  );
}

/**
 * BCP-47 → Locale，与 Rust `Locale::from_tag` 同规则：
 * 中文且非繁体 → zh，其余（含繁体、日语、空串）→ en。
 *
 * 繁体落 en 而不是 zh：catalog 没有繁体，给简体等于让繁体用户读简体。
 */
export function fromTag(tag: string): Locale {
  const low = tag.toLowerCase();
  return low.startsWith("zh") && !isTraditionalTag(low) ? "zh" : "en";
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
  // 浏览器 dev：读 localStorage 里手动选过的偏好。没有真实系统语言可读，
  // `system` 恒为 null（设置页据此显示「系统语言无法读取」），
  // `autoResolved` 用 navigator 的解析结果，让副标题不至于整句消失
  const fallback = navigatorLocale();
  const stored = typeof localStorage !== "undefined" ? localStorage.getItem(LOCAL_PREF_KEY) : null;
  if (stored === "zh" || stored === "en") {
    return { current: stored, source: "manual", system: null, autoResolved: fallback };
  }
  return { current: fallback, source: "auto", system: null, autoResolved: fallback };
}
