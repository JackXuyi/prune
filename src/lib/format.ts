/**
 * 字节/时间格式化。所有数值都按「字节」传入，输出统一带单位。
 *
 * i18n 约定（docs/i18n-spec.md §2.6）：locale 是**可选参数**，缺省取当前界面语言。
 * 40+ 个调用点因此不必全部改写；只有「要按另一种语言渲染」的场景（比如把
 * 报告按用户选择导出）才显式传。
 *
 * 这里不 import `tGlobal`：`src/lib/**` 读全局会让「切语言后已生成的旧串」
 * 不更新，测试语言也不可控。需要文案就走 `i18nFor(locale)`，它是纯函数。
 */
import { i18nFor } from "../i18n";
import { currentLocale } from "../i18n/store";
import type { Locale } from "../i18n/types";

const UNITS = ["B", "KB", "MB", "GB", "TB"] as const;

export interface SplitBytes {
  value: string;
  unit: string;
}

/** 拆成数值与单位，便于「13.4 / 14.0 GB」这类共用单位的排版 */
export function splitBytes(bytes: number, minDigits = 1): SplitBytes {
  if (!Number.isFinite(bytes) || bytes <= 0) return { value: "0", unit: "B" };
  let v = bytes;
  let i = 0;
  while (v >= 1024 && i < UNITS.length - 1) {
    v /= 1024;
    i += 1;
  }
  return { value: v.toFixed(i >= 3 ? minDigits : 0), unit: UNITS[i] };
}

export function formatBytes(bytes: number): string {
  const { value, unit } = splitBytes(bytes);
  return `${value} ${unit}`;
}

/** 百分比（0~1 → 整数百分比字符串） */
export function formatPercent(ratio: number | null): string {
  if (ratio == null || !Number.isFinite(ratio)) return "—";
  return `${Math.round(ratio * 100)}%`;
}

/**
 * CPU 百分比（单核口径，可 >100%）。null → "—"（首见/采集失败不是 0）；
 * ≥100% 时附「约 N 核」换算，帮助用户理解多线程进程的读数。
 */
export function formatCpu(v: number | null | undefined, locale: Locale = currentLocale()): string {
  if (v == null || !Number.isFinite(v)) return "—";
  if (v < 1) return "<1%";
  if (v < 100) return `${Math.round(v)}%`;
  const cores = i18nFor(locale).t("common.unit.coresApprox", { n: (v / 100).toFixed(1) });
  return `${Math.round(v)}% (${cores})`;
}

/** 数字不带单位（swap 已用/上限共用一个单位时用） */
export function numOf(bytes: number): string {
  return splitBytes(bytes).value;
}

export function unitOf(bytes: number): string {
  return splitBytes(bytes).unit;
}

/**
 * 时钟（24 小时制）。
 *
 * 英文用 `en-GB` 而不是 `en-US`：`en-US` + `hour12:false` 在部分 WebKit 版本会
 * 渲染出 `24:05` 这种非法值，`en-GB` 稳定。
 */
export function formatClock(tsSec: number | null, locale: Locale = currentLocale()): string {
  if (!tsSec) return "—";
  return new Date(tsSec * 1000).toLocaleTimeString(locale === "zh" ? "zh-CN" : "en-GB", {
    hour12: false,
    hour: "2-digit",
    minute: "2-digit",
  });
}

const relativeFormatters: Record<Locale, Intl.RelativeTimeFormat> = {
  zh: new Intl.RelativeTimeFormat("zh", { numeric: "auto" }),
  // numeric:"auto" 才会给「yesterday」而不是「1 day ago」
  en: new Intl.RelativeTimeFormat("en", { numeric: "auto" }),
};

/**
 * 相对时间：<60s → 刚刚 / <1h → N 分钟前 / <24h → N 小时前 / 否则时钟。
 *
 * 分档不变（有意如此：档位是交互约定，不是语言习惯），只有措辞跟 locale 走。
 */
export function formatRelative(tsSec: number | null, locale: Locale = currentLocale()): string {
  const { t } = i18nFor(locale);
  if (!tsSec) return t("common.time.notDeepScanned");
  const diff = Math.max(0, Math.floor(Date.now() / 1000) - tsSec);
  if (diff < 60) return t("common.time.justNow");
  const rtf = relativeFormatters[locale];
  if (diff < 3600) return rtf.format(-Math.floor(diff / 60), "minute");
  if (diff < 86400) return rtf.format(-Math.floor(diff / 3600), "hour");
  return formatClock(tsSec, locale);
}

/** 速率：保留 1 位小数，0 显示为 0 */
export function formatRate(perSec: number): string {
  if (!Number.isFinite(perSec) || perSec <= 0) return "0";
  return perSec >= 10 ? perSec.toFixed(0) : perSec.toFixed(1);
}

/**
 * 本地日期 `YYYY-MM-DD`。
 *
 * 必须与 Rust `prune_core::localtime::day_string` 同格式：7 日柱的键就是它，
 * 差一个字符就整排灰条（而且看起来像「没记到数据」，很难查）。
 */
export function dayString(d: Date): string {
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

/**
 * 最近 n 天的日期键（升序，含今天）。
 *
 * 锚在当天 12:00 再回退整日：直接减 86400000 在夏令时切换日会跳过或重复一天，
 * 而 12:00 离两次切换点都够远。
 */
export function recentDays(n: number): string[] {
  const anchor = new Date();
  anchor.setHours(12, 0, 0, 0);
  const out: string[] = [];
  for (let i = n - 1; i >= 0; i -= 1) {
    out.push(dayString(new Date(anchor.getTime() - i * 86_400_000)));
  }
  return out;
}
