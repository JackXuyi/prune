/** 字节/时间格式化。所有数值都按「字节」传入，输出统一带单位。 */

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

/** 数字不带单位（swap 已用/上限共用一个单位时用） */
export function numOf(bytes: number): string {
  return splitBytes(bytes).value;
}

export function unitOf(bytes: number): string {
  return splitBytes(bytes).unit;
}

export function formatClock(tsSec: number | null): string {
  if (!tsSec) return "—";
  return new Date(tsSec * 1000).toLocaleTimeString("zh-CN", {
    hour12: false,
    hour: "2-digit",
    minute: "2-digit",
  });
}

export function formatRelative(tsSec: number | null): string {
  if (!tsSec) return "尚未深探";
  const diff = Math.max(0, Math.floor(Date.now() / 1000) - tsSec);
  if (diff < 60) return "刚刚";
  if (diff < 3600) return `${Math.floor(diff / 60)} 分钟前`;
  if (diff < 86400) return `${Math.floor(diff / 3600)} 小时前`;
  return formatClock(tsSec);
}

/** 速率：保留 1 位小数，0 显示为 0 */
export function formatRate(perSec: number): string {
  if (!Number.isFinite(perSec) || perSec <= 0) return "0";
  return perSec >= 10 ? perSec.toFixed(0) : perSec.toFixed(1);
}
