/**
 * `chart` namespace：图表层的无障碍与兜底文案（Sparkline / BarSeries /
 * MeterBar / StackBar）。
 *
 * 这一组只有**组件自己才知道**的句子（aria-label、缺数据提示、占比描述），
 * 数值与图例文本一律由调用方传入 —— 图表组件不猜业务语义。
 *
 * 「缺数据」与「值为 0」是两件事，这个区别在三个组件里各有一句，翻译时
 * 不能合并成同一句 “no data”（否则读不出「Prune 没运行」这层含义）。
 */
import type { MessageTree } from "../../types";

const zh = {
  sparkline: {
    aria: "走势",
    emptySeries: "暂无序列 · 深探轮次开始后每 5 分钟一个点",
    accumulating: "序列积累中",
  },
  bars: {
    noRecord: "{label} · 无记录（Prune 未运行）",
  },
  meter: {
    ratio: "占比 {pct}%",
    unavailable: "读数不可用",
  },
  stack: {
    mutedSuffix: "（不计入）",
    /** 图例连接符：中文用全角逗号，英文用逗号加空格 */
    join: "，",
  },
} as const;

const en: MessageTree<typeof zh> = {
  sparkline: {
    aria: "Trend",
    emptySeries: "No series yet · one point every 5 minutes once a deep scan starts",
    accumulating: "Collecting series…",
  },
  bars: {
    noRecord: "{label} · no record (Prune wasn't running)",
  },
  meter: {
    ratio: "{pct}% of max",
    unavailable: "Reading unavailable",
  },
  stack: {
    mutedSuffix: " (not counted)",
    join: ", ",
  },
};

export const chart = { zh, en } as const;
