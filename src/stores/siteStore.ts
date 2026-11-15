import { listen } from "@tauri-apps/api/event";
import { create } from "zustand";

import { devSiteProbes, devSites } from "../lib/devFixture";
import {
  isTauri,
  netSitesCancel,
  netSitesCapability,
  netSitesCopyReport,
  netSitesList,
  netSitesProbeOne,
  netSitesReset,
  netSitesRun,
  netSitesSave,
} from "../lib/ipc";
import type { Site, SiteCapability, SiteProbe, SiteRun } from "../types/net";

/**
 * 站点检测 store（F15，docs/net-sites-plan.md §7）。
 *
 * 与 netDiagStore 同族（手动短任务、不落库、不常驻），两处差异：
 * - **进度是流式的**：每完成一站由 `net-sites://progress` 推过来，逐行点亮；
 * - **站表是用户资产**：存 settings.json，改完立刻生效，不必重启。
 *
 * 纪律（§12）：`probes` 里没有的站显示「待测」，不是「0 ms」；
 * 一轮开跑先清空，保证「汇总」永远等于「本轮已完成的行」。
 */
interface SiteStore {
  sites: Site[];
  /** 是否自定义过（决定「恢复默认」显示与否） */
  customized: boolean;
  /** 存下的站表读不出来、已回落默认：必须在页面上提示 */
  recovered: boolean;
  capability: SiteCapability | null;
  /** 本轮已完成的结果，按 id 索引 */
  probes: Record<string, SiteProbe>;
  running: boolean;
  /** 正在单站重测的 id 集合（行内 spinner） */
  rerunning: Record<string, boolean>;
  done: number;
  total: number;
  cancelled: boolean;
  ranAt: number | null;
  elapsedMs: number | null;
  copied: boolean;
  copyFailed: boolean;
  /** 站表读写失败的原因（保存/恢复默认），行内红字展示 */
  listError: string | null;
  init: () => Promise<void>;
  runAll: () => Promise<void>;
  rerun: (id: string) => Promise<void>;
  cancel: () => Promise<void>;
  save: (sites: Site[]) => Promise<void>;
  reset: () => Promise<void>;
  copyReport: () => Promise<void>;
  resetCopied: () => void;
}

/** StrictMode 下 effect 跑两次：订阅只做一次 */
let subscribed = false;

function applyList(set: (p: Partial<SiteStore>) => void, list: {
  sites: Site[];
  customized: boolean;
  recovered: boolean;
}) {
  // 站表变了，上一轮的读数就不可信（id 可能已换人）——直接清空，不玩增量合并
  set({
    sites: list.sites,
    customized: list.customized,
    recovered: list.recovered,
    probes: {},
    done: 0,
    total: list.sites.length,
    ranAt: null,
    elapsedMs: null,
    cancelled: false,
  });
}

export const useSiteStore = create<SiteStore>((set, get) => ({
  sites: [],
  customized: false,
  recovered: false,
  capability: null,
  probes: {},
  running: false,
  rerunning: {},
  done: 0,
  total: 0,
  cancelled: false,
  ranAt: null,
  elapsedMs: null,
  copied: false,
  copyFailed: false,
  listError: null,

  init: async () => {
    if (subscribed) return;
    subscribed = true;

    if (!isTauri()) {
      if (import.meta.env.DEV) {
        const sites = devSites();
        const probes = devSiteProbes();
        set({
          capability: { curl: true, version: "curl 8.7.1 (dev preview)" },
          sites,
          probes: Object.fromEntries(probes.map((p) => [p.id, p])),
          total: sites.length,
          done: probes.length,
          ranAt: Math.floor(Date.now() / 1000) - 20,
          elapsedMs: 4_180,
        });
      }
      return;
    }

    await listen<SiteProbe>("net-sites://progress", (e) => {
      const p = e.payload;
      set((s) => ({
        probes: { ...s.probes, [p.id]: p },
        done: s.done + 1,
      }));
    });

    await listen<SiteRun>("net-sites://done", (e) => {
      const run = e.payload;
      // 以整轮结果为准（取消时未跑的站不在 results 里，正好不该出现在汇总里）
      set({
        running: false,
        probes: Object.fromEntries(run.results.map((p) => [p.id, p])),
        done: run.results.length,
        ranAt: run.at,
        elapsedMs: run.elapsedMs,
        cancelled: run.cancelled,
      });
    });

    try {
      const cap = await netSitesCapability();
      set({ capability: cap });
    } catch {
      // 拉不到 capability：按不可用处理（不渲染空表），与 Windows 测速卡同一纪律
      set({ capability: { curl: false, version: null } });
    }
    try {
      applyList(set, await netSitesList());
    } catch (e) {
      set({ listError: String(e) });
    }
  },

  runAll: async () => {
    if (get().running) return;
    const sites = get().sites;
    if (sites.length === 0) return;
    // 开跑先清空：汇总只算本轮，旧读数混进来会得出「11/10」
    set({
      running: true,
      probes: {},
      done: 0,
      total: sites.length,
      cancelled: false,
      listError: null,
    });
    try {
      if (!isTauri()) {
        const probes = devSiteProbes();
        set({
          probes: Object.fromEntries(probes.map((p) => [p.id, p])),
          done: probes.length,
          ranAt: Math.floor(Date.now() / 1000),
          elapsedMs: 4_180,
          running: false,
        });
        return;
      }
      await netSitesRun();
      // 收尾以 `net-sites://done` 事件为准（它带 cancelled / elapsed），这里不重复 set
    } catch (e) {
      // 失败原因（busy / 调度失败）交给页面提示；不清空已完成的读数
      set({ running: false, listError: String(e) });
    }
  },

  rerun: async (id) => {
    if (get().running || get().rerunning[id]) return;
    set((s) => ({ rerunning: { ...s.rerunning, [id]: true } }));
    try {
      const probe = isTauri()
        ? await netSitesProbeOne(id)
        : (devSiteProbes().find((p) => p.id === id) ?? null);
      if (probe) {
        set((s) => ({ probes: { ...s.probes, [probe.id]: probe } }));
      }
    } catch (e) {
      set({ listError: String(e) });
    } finally {
      set((s) => ({ rerunning: { ...s.rerunning, [id]: false } }));
    }
  },

  cancel: async () => {
    if (!isTauri()) {
      set({ running: false, cancelled: true });
      return;
    }
    try {
      await netSitesCancel();
    } catch {
      // 取消失败只影响等待时长，不打断用户
    }
  },

  save: async (sites) => {
    set({ listError: null });
    if (!isTauri()) {
      // 浏览器预览：没有 Rust 侧校验，只做本地回显（生产构建走 Rust，不重复实现规则）
      applyList(set, { sites, customized: true, recovered: false });
      return;
    }
    try {
      applyList(set, await netSitesSave(sites));
    } catch (e) {
      // 校验在 Rust 侧（唯一真相），错误串直接显示在编辑区上方
      set({ listError: String(e) });
      throw e;
    }
  },

  reset: async () => {
    set({ listError: null });
    if (!isTauri()) {
      applyList(set, { sites: devSites(), customized: false, recovered: false });
      return;
    }
    try {
      applyList(set, await netSitesReset());
    } catch (e) {
      set({ listError: String(e) });
    }
  },

  copyReport: async () => {
    if (isTauri()) {
      try {
        await netSitesCopyReport();
        set({ copied: true, copyFailed: false });
      } catch {
        set({ copied: false, copyFailed: true });
      }
      return;
    }
    set({ copied: false, copyFailed: true });
  },

  resetCopied: () => set({ copied: false, copyFailed: false }),
}));
