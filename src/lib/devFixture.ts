import type { MemSnapshot, ProcMem, WorkspaceRef } from "../types/mem";

/**
 * 仅开发态使用（import.meta.env.DEV && 非 Tauri）：让 UI 可以脱离桌面端
 * 在浏览器里迭代。生产构建不包含此路径的执行结果。
 */

let nextPid = 1000;

function proc(
  family: string,
  name: string,
  rss: number,
  footprint: number | null,
  swapped: number | null,
  exe?: string,
  workspace: WorkspaceRef | null = null,
): ProcMem {
  nextPid += 1;
  return {
    pid: nextPid,
    ppid: family === "Cursor" ? 1001 : 1,
    name,
    exe: exe ?? `/Applications/${family}.app/Contents/MacOS/${name}`,
    family,
    rss,
    footprint,
    swapped,
    source: footprint == null ? "rss" : "footprint",
    startTime: 1_700_000_000 + nextPid * 7,
    workspace,
  };
}

/** 固定工作区，让「按项目分组」和「未归属」两种状态都能在浏览器里看到 */
const WS_PROJECTS: WorkspaceRef[] = [
  { label: "meetily", path: "/Users/xuyi/Desktop/git/meetily", source: "label" },
  { label: "prune", path: "/Users/xuyi/Desktop/git/prune", source: "handle" },
  { label: "AI", path: "/Users/xuyi/Desktop/git/AI", source: "inherited" },
];

function procs(): ProcMem[] {
  const out: ProcMem[] = [];
  out.push(proc("Cursor", "Cursor", 512 * 1e6, 1.42e9, 0.31e9));
  // 渲染进程：拿不到项目线索 → 未归属（真实场景下 Cursor 的大部分内存都在这里）
  for (let i = 0; i < 40; i += 1) {
    out.push(
      proc("Cursor", `Cursor Helper (${i % 3 === 0 ? "Renderer" : "Utility"})`, 18e6 * (1 + (i % 5)), 62e6, 9e6),
    );
  }
  // 扩展宿主 / 语言服务器：能归属到具体项目
  for (let i = 0; i < 26; i += 1) {
    const ws = i < 9 ? WS_PROJECTS[i % WS_PROJECTS.length] : null;
    out.push(proc("Cursor", `Cursor Helper (Plugin)`, 11e6, 38e6, 5e6, undefined, ws));
  }
  out.push(proc("Chrome", "Google Chrome", 640e6, 1.28e9, 0.42e9));
  for (let i = 0; i < 30; i += 1) {
    out.push(proc("Chrome", "Google Chrome Helper", 24e6 * (1 + (i % 7)), 58e6, 22e6));
  }
  out.push(proc("Node", "next-server", 113e6, 3.35e9, 3.25e9, "/usr/local/bin/node"));
  out.push(proc("Node", "tsserver", 96e6, 2.42e9, 2.4e9, "/usr/local/bin/node"));
  out.push(proc("Node", "followup", 71e6, 1.77e9, 1.72e9, "/usr/local/bin/node"));
  for (let i = 0; i < 18; i += 1) {
    out.push(proc("Node", `vite`, 30e6 * (1 + (i % 4)), 120e6, 44e6));
  }
  for (let i = 0; i < 11; i += 1) {
    // 前 4 个已深探、其余未探 → 覆盖「下界估算」状态
    out.push(
      proc("WeChat", "WeChat", 38e6 * (1 + (i % 3)), i < 4 ? 96e6 : null, i < 4 ? 31e6 : null),
    );
  }
  out.push(proc("kernel_task", "kernel_task", 612e6, null, null, undefined));
  for (let i = 0; i < 8; i += 1) {
    out.push(proc("Xcode", "swift-frontend", 44e6 * (1 + (i % 4)), 180e6, 60e6));
  }
  for (let i = 0; i < 6; i += 1) {
    out.push(proc("Docker", "com.docker.backend", 40e6 * (1 + (i % 3)), 150e6, 70e6));
  }
  for (let i = 0; i < 5; i += 1) {
    out.push(proc("Slack", "Slack Helper", 28e6 * (1 + (i % 3)), 88e6, 26e6));
  }
  for (let i = 0; i < 9; i += 1) {
    out.push(proc(`sysdaemon-${i}`, `sysdaemon-${i}`, 4e6 * (1 + (i % 6)), null, null, undefined));
  }
  return out;
}

export function devSnapshot(): MemSnapshot {
  const all = devProcesses();
  const deep = all.filter((p) => p.footprint != null);
  const now = Math.floor(Date.now() / 1000);
  return {
    physicalTotal: 17_179_869_184,
    compressorBytes: 6.66e9,
    pagesFreeBytes: 62 * 1e6,
    swap: {
      total: 12_288 * 1e6,
      used: 10_930 * 1e6,
      free: 1_358 * 1e6,
      fileCount: 12,
      newToday: 2,
    },
    pressure: { level: "warning", raw: 2 },
    thrash: { swapinsPerSec: 0.4, swapoutsPerSec: 0.7 },
    topSwapped: [...deep]
      .sort((a, b) => (b.swapped ?? 0) - (a.swapped ?? 0))
      .slice(0, 5),
    fastAt: now,
    deepUpdatedAt: now - 95,
    deepAvailable: true,
    families: aggregateDev(all),
  };
}

function aggregateDev(list: ProcMem[]) {
  type Acc = {
    rss: number;
    fp: number;
    unprobed: number;
    sw: number;
    n: number;
    covered: number;
    ws: Map<string, { label: string; source: WorkspaceRef["source"]; n: number; fp: number; unprobed: number; rss: number }>;
    wsNone: { n: number; fp: number; unprobed: number };
  };
  const map = new Map<string, Acc>();
  for (const p of list) {
    const acc =
      map.get(p.family) ??
      { rss: 0, fp: 0, unprobed: 0, sw: 0, n: 0, covered: 0, ws: new Map(), wsNone: { n: 0, fp: 0, unprobed: 0 } };
    acc.rss += p.rss;
    if (p.footprint == null) {
      acc.unprobed += p.rss;
    } else {
      acc.fp += p.footprint;
      acc.covered += 1;
    }
    acc.sw += p.swapped ?? 0;
    acc.n += 1;

    if (p.workspace) {
      const w = acc.ws.get(p.workspace.path) ?? {
        label: p.workspace.label,
        source: p.workspace.source,
        n: 0,
        fp: 0,
        unprobed: 0,
        rss: 0,
      };
      w.n += 1;
      w.rss += p.rss;
      if (p.footprint == null) w.unprobed += p.rss;
      else w.fp += p.footprint;
      acc.ws.set(p.workspace.path, w);
    } else {
      acc.wsNone.n += 1;
      if (p.footprint == null) acc.wsNone.unprobed += p.rss;
      else acc.wsNone.fp += p.footprint;
    }
    map.set(p.family, acc);
  }
  return [...map.entries()]
    .map(([name, a]) => ({
      name,
      processCount: a.n,
      deepCovered: a.covered,
      rss: a.rss,
      footprint: a.covered > 0 ? a.fp + a.unprobed : null,
      swapped: a.sw > 0 ? a.sw : null,
      workspaces: [...a.ws.entries()]
        .map(([path, w]) => ({
          path,
          label: w.label,
          source: w.source,
          processCount: w.n,
          rss: w.rss,
          footprint: a.covered > 0 ? w.fp + w.unprobed : null,
        }))
        .sort((x, y) => (y.footprint ?? y.rss) - (x.footprint ?? x.rss)),
      wsUnattributedCount: a.wsNone.n,
      wsUnattributedFootprint: a.covered > 0 ? a.wsNone.fp + a.wsNone.unprobed : null,
    }))
    .sort((a, b) => (b.footprint ?? b.rss) - (a.footprint ?? a.rss));
}

/** 两个视图共用同一份数据，pid 才能对上 */
let cached: ProcMem[] | null = null;

export function devProcesses(): ProcMem[] {
  cached ??= procs();
  return cached;
}
