import { listen } from "@tauri-apps/api/event";
import { create } from "zustand";

import { tGlobal } from "../i18n/store";

import { devPortSnapshot } from "../lib/devFixture";
import { isTauri, portProbe, portRefresh, portSetActive, portSnapshot } from "../lib/ipc";
import { isSystemProcess } from "../types/net";
import type { PortEntry, PortSnapshot, ProbeOutcome } from "../types/net";

/** 自动探测开关落盘键。与主题同款 localStorage 约定：纯前端偏好，不进 Rust 设置 */
const AUTO_PROBE_KEY = "prune.ports.autoprobe";

function readAutoProbe(): boolean {
  try {
    return typeof localStorage !== "undefined" && localStorage.getItem(AUTO_PROBE_KEY) === "1";
  } catch {
    return false; // 隐私模式 / 配额满：退回默认关，探测本来就是用户显式授权的功能
  }
}

/** 探针结果的键：`(port, pid, start_time)` 与 Rust 缓存同源 */
export function probeKey(entry: PortEntry): string {
  const b = entry.listeners.find((x) => x.pid != null) ?? entry.listeners[0];
  return `${entry.port}:${b?.pid ?? 0}:${b?.startTime ?? 0}`;
}

interface PortStore {
  /**
   * 最新一轮快照。null 有两种含义，UI 必须区分：
   * - `loading` = 正在扫（第一次）
   * - `!loading` = 还没扫过 / 平台不支持 → 显示原因，**绝不显示「没有端口占用」**
   */
  snapshot: PortSnapshot | null;
  loading: boolean;
  refreshing: boolean;
  error: string | null;
  /** 折叠层是否展开（默认折叠：70% 的内部 IPC 端口是噪音） */
  showFolded: boolean;
  /**
   * 从别处跳进来时要预填的搜索词（应用行点端口徽标 → 端口 tab 直接定位）。
   * 只消费一次，PortBoard 读完就清空，否则用户手动改了搜索又会被拉回去。
   */
  focusQuery: string;
  /** L1 探针结果（确认级）。与快照独立——快照刷新不清空，键稳定时结果仍有效 */
  probeResults: Record<string, ProbeOutcome>;
  /** 探测中的行（键同 probeResults） */
  probing: Record<string, boolean>;
  /** 探测失败的原因（连接失败 / 服务已退出） */
  probeErrors: Record<string, string>;
  /**
   * 设置项「自动探测关注层端口」（P2）。默认关——它发包，
   * 与行内「探测」按钮共享同一纪律：用户显式授权才跑（调研 §5）。
   */
  autoProbe: boolean;
  init: () => Promise<void>;
  refresh: () => Promise<void>;
  setActive: (active: boolean) => Promise<void>;
  toggleFolded: () => void;
  setFocus: (q: string) => void;
  clearFocus: () => void;
  /** 只读已有快照，不触发扫描：给「应用行端口徽标」用，避免为了一枚徽标在启动时跑 lsof */
  hydrate: () => Promise<void>;
  /** L1 探测一行端口。用户显式触发；Rust 侧有缓存，重复点击零网络开销 */
  probe: (entry: PortEntry) => Promise<void>;
  /** 切换自动探测并落盘。开启后立即对当前快照跑一轮 */
  setAutoProbe: (v: boolean) => void;
  /** 对当前快照的关注层 TCP 端口补探缺漏（缓存命中零开销，30 s 轮幂等） */
  runAutoProbe: () => void;
}

/** StrictMode 下 effect 跑两次：初始化与订阅只做一次 */
let initStarted = false;
let subscribed = false;

export const usePortStore = create<PortStore>((set, get) => ({
  snapshot: null,
  loading: true,
  refreshing: false,
  error: null,
  showFolded: false,
  focusQuery: "",
  probeResults: {},
  probing: {},
  probeErrors: {},
  autoProbe: readAutoProbe(),

  init: async () => {
    if (initStarted) return;
    initStarted = true;

    if (!isTauri()) {
      if (import.meta.env.DEV) {
        set({ snapshot: devPortSnapshot(), loading: false });
      } else {
        set({ loading: false, error: tGlobal("common.err.needsDesktop") });
      }
      return;
    }

    try {
      const snapshot = await portSnapshot();
      set({ snapshot, loading: false });
      // 冷启动：一次都没有 → 立即扫一轮（用户已经站在端口 tab 前了）
      if (!snapshot) await get().refresh();
      get().runAutoProbe();
    } catch (e) {
      set({ loading: false, error: String(e) });
      return;
    }

    if (subscribed) return;
    subscribed = true;

    // 30 s 定时轮的产出：定时器只在 tab 可见时跑，事件到来说明用户正在看
    await listen<PortSnapshot>("port://snapshot", (e) => {
      set({ snapshot: e.payload, refreshing: false, loading: false });
      get().runAutoProbe();
    });
  },

  refresh: async () => {
    if (get().refreshing || !isTauri()) return;
    set({ refreshing: true, error: null });
    try {
      await portRefresh();
      // 命中单飞闸门时 Rust 侧直接返回旧快照：这里必须重新拉，不能假设已变
      const snapshot = await portSnapshot();
      set({ snapshot });
      get().runAutoProbe();
    } catch (e) {
      set({ error: String(e) });
    } finally {
      set({ refreshing: false });
    }
  },

  /** 可见性门控：切出 tab 必须关掉，否则 30 s 定时器会一直跑 lsof（验收 §12-9） */
  setActive: async (active) => {
    if (!isTauri()) return;
    try {
      await portSetActive(active);
    } catch {
      // 门控失败只影响耗电，不影响正确性：不打断用户
    }
  },

  toggleFolded: () => set({ showFolded: !get().showFolded }),

  setFocus: (q) => set({ focusQuery: q }),

  clearFocus: () => set({ focusQuery: "" }),

  hydrate: async () => {
    if (get().snapshot || !isTauri()) return;
    try {
      const snapshot = await portSnapshot();
      if (snapshot) set({ snapshot, loading: false });
    } catch {
      // 只读探测失败就保持「还没扫」，不打扰用户
    }
  },

  probe: async (entry) => {
    const key = probeKey(entry);
    if (get().probing[key] || get().probeResults[key]) return;
    const b = entry.listeners.find((x) => x.pid != null) ?? entry.listeners[0];
    set({
      probing: { ...get().probing, [key]: true },
      probeErrors: { ...get().probeErrors, [key]: "" },
    });
    try {
      const out = await portProbe(entry.port, b?.pid ?? null, b?.startTime ?? 0);
      set({ probeResults: { ...get().probeResults, [key]: out } });
    } catch (e) {
      set({ probeErrors: { ...get().probeErrors, [key]: String(e) } });
    } finally {
      set({ probing: { ...get().probing, [key]: false } });
    }
  },

  setAutoProbe: (v) => {
    try {
      if (v) localStorage.setItem(AUTO_PROBE_KEY, "1");
      else localStorage.removeItem(AUTO_PROBE_KEY);
    } catch {
      // 写盘失败不影响本会话：状态已在内存里，下个会话退回默认关
    }
    set({ autoProbe: v });
    if (v) get().runAutoProbe();
  },

  /**
   * 只补关注层 TCP 且有 pid 的行——与行内「探测」按钮同一门控：
   * - 折叠层不自动碰（内部 IPC 是噪音，探它违背最小打扰）；
   * - 系统进程不探（AirPlay 5000 会得到一个没有信息量的「Web ✓」）；
   * - 探测失败过的不重试：服务退出后端口键不变，重试只会每轮空打。
   */
  runAutoProbe: () => {
    const { autoProbe, snapshot, probeResults, probeErrors, probing } = get();
    if (!autoProbe || !snapshot || !isTauri()) return;
    for (const e of snapshot.entries) {
      if (!e.watched || e.proto !== "tcp") continue;
      const b = e.listeners.find((x) => x.pid != null) ?? e.listeners[0];
      if (!b || b.pid == null) continue;
      const owners = e.listeners.filter((x) => x.pid != null);
      if (owners.length > 0 && owners.every((x) => isSystemProcess(x.comm))) continue;
      const key = probeKey(e);
      if (probeResults[key] || probeErrors[key] || probing[key]) continue;
      void get().probe(e);
    }
  },
}));
