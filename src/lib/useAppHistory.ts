/**
 * D3/D5：应用小时历史（一次拉 8 天 = 192 h，前端按需聚合）。
 *
 * 为什么不按视图分别请求：区间切换要「即时」，而且 24 h 是 7 天的子集，
 * 一次拉完切换零请求。刷新节拍 5 min（与深探轮同拍），窗口不可见时不刷新。
 */
import { useEffect, useRef, useState } from "react";

import type { HistoryBucket } from "../types/appHistory";
import { appHistory, isTauri } from "./ipc";

/** 拉取窗口：8 天（比 7 天多一天，保证「7 整天」完整） */
export const HISTORY_HOURS = 192;
/** 刷新间隔 */
const REFRESH_MS = 5 * 60 * 1000;

const pad = (n: number) => String(n).padStart(2, "0");

/** 本地小时桶 `YYYY-MM-DDTHH`（与 Rust localtime::hour_string 同格式） */
export function hourKey(d: Date): string {
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}`;
}

export function dayKey(d: Date): string {
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

/** 最近 n 小时（含当前小时），升序 */
export function lastHours(n: number, now = new Date()): string[] {
  const out: string[] = [];
  for (let i = n - 1; i >= 0; i -= 1) {
    out.push(hourKey(new Date(now.getTime() - i * 3600_000)));
  }
  return out;
}

/** 最近 n 天（含今天），升序 */
export function lastDays(n: number, now = new Date()): string[] {
  const out: string[] = [];
  for (let i = n - 1; i >= 0; i -= 1) {
    const d = new Date(now.getFullYear(), now.getMonth(), now.getDate() - i);
    out.push(dayKey(d));
  }
  return out;
}

export function useAppHistory(family: string | null): {
  rows: HistoryBucket[];
  loading: boolean;
  error: string | null;
  refresh: () => void;
} {
  const [rows, setRows] = useState<HistoryBucket[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [nonce, setNonce] = useState(0);
  const alive = useRef(true);

  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);

  useEffect(() => {
    setRows([]);
    setError(null);
    if (!family) return;

    if (!isTauri()) {
      // 浏览器走查：没有 SQLite，给一段「有缺口」的假序列让结构与空态可见
      const now = new Date();
      const fake: HistoryBucket[] = lastHours(48, now)
        .map((bucket, i) => ({
          bucket,
          rssAvg: i % 11 === 0 ? null : 3_600_000_000 + i * 42_000_000,
          rssPeak: i % 11 === 0 ? null : 4_100_000_000 + i * 51_000_000,
          cpuAvg: i % 11 === 0 ? null : 3 + Math.abs(Math.sin(i / 3)) * 9,
          cpuPeak: i % 11 === 0 ? null : 8 + Math.abs(Math.sin(i / 2.2)) * 28,
          netDown: i % 11 === 0 ? 0 : 120_000_000 + i * 900_000,
          netUp: i % 11 === 0 ? 0 : 8_000_000 + i * 40_000,
          samples: i % 11 === 0 ? 0 : 360,
        }))
        .filter((b) => b.samples > 0);
      setRows(fake);
      return;
    }

    let timer: number | null = null;
    const load = async () => {
      setLoading(true);
      try {
        const data = await appHistory(family, HISTORY_HOURS);
        if (alive.current) {
          setRows(data);
          setError(null);
        }
      } catch (e) {
        if (alive.current) setError(e instanceof Error ? e.message : String(e));
      } finally {
        if (alive.current) setLoading(false);
      }
    };

    void load();
    timer = window.setInterval(() => {
      if (document.visibilityState === "visible") void load();
    }, REFRESH_MS);

    return () => {
      if (timer != null) window.clearInterval(timer);
    };
  }, [family, nonce]);

  return { rows, loading, error, refresh: () => setNonce((n) => n + 1) };
}

/** 某天的聚合（7 天视图的一根柱） */
export interface DayAgg {
  key: string;
  cpuPeak: number;
  rssPeak: number;
  netDown: number;
  netUp: number;
  samples: number;
  peakHour: string | null;
}

export function aggregateDays(rows: HistoryBucket[]): Map<string, DayAgg> {
  const out = new Map<string, DayAgg>();
  for (const r of rows) {
    const key = r.bucket.slice(0, 10);
    const cur =
      out.get(key) ??
      { key, cpuPeak: 0, rssPeak: 0, netDown: 0, netUp: 0, samples: 0, peakHour: null };
    if (r.cpuPeak != null && r.cpuPeak >= cur.cpuPeak) {
      cur.cpuPeak = r.cpuPeak;
      cur.peakHour = r.bucket;
    }
    if (r.rssPeak != null) cur.rssPeak = Math.max(cur.rssPeak, r.rssPeak);
    cur.netDown += r.netDown;
    cur.netUp += r.netUp;
    cur.samples += r.samples;
    out.set(key, cur);
  }
  return out;
}
