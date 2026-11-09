/** 图表构件共用：Readout Tone → 填充色类（UX v3 §7.4，一页色阶 ≤ 2） */
import type { Tone } from "../../lib/statsReadout";

export const TONE_FILL: Record<Tone, string> = {
  accent: "bg-accent",
  warn: "bg-warn",
  danger: "bg-danger",
  info: "bg-info",
  neutral: "bg-ink-faint",
};
