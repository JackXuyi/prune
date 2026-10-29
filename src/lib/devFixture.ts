import type {
  CacheGroup,
  DeadweightItem,
  DetailItem,
  DiskReport,
  PlatformStat,
  ReclaimPreview,
} from "../types/disk";
import type { LeakItem, LeakReport, MemSnapshot, ProcMem, WorkspaceRef } from "../types/mem";

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
    physicalUsedBytes: 13_800_000_000,
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
    leaks: devLeaks(all, now),
  };
}

/**
 * 泄漏判定的浏览器 fixture。
 *
 * 数字刻意做成真实形态：Cursor 主进程 30 min 涨 480 MB（占比 51%、R² 0.86、
 * 最大单点跃升只占 15% → 持续爬升），next-server 走 GC 语言口径（门槛 300 MB / 60 min）。
 * pid 取自 devProcesses，保证点进去能看到对应进程。
 */
function devLeaks(all: ProcMem[], now: number): LeakReport {
  const pick = (family: string, name: string) =>
    all.find((p) => p.family === family && p.name === name);

  const items: LeakItem[] = [];
  const cursor = pick("Cursor", "Cursor");
  if (cursor?.footprint != null) {
    items.push({
      pid: cursor.pid,
      startTime: cursor.startTime,
      family: "Cursor",
      name: cursor.name,
      footprint: cursor.footprint,
      growth: 480e6,
      growthRatio: 0.51,
      windowSecs: 1800,
      points: 7,
      slopeMbPerHour: 960,
      r2: 0.86,
      stepRatio: 0.15,
      gcRuntime: false,
    });
  }
  const server = pick("Node", "next-server");
  if (server?.footprint != null) {
    items.push({
      pid: server.pid,
      startTime: server.startTime,
      family: "Node",
      name: server.name,
      footprint: server.footprint,
      growth: 1.4e9,
      growthRatio: 0.72,
      windowSecs: 3600,
      points: 13,
      slopeMbPerHour: 1400,
      r2: 0.91,
      stepRatio: 0.11,
      gcRuntime: true,
    });
  }

  return {
    items,
    observed: 95,
    insufficient: 14,
    benign: 23,
    below: 95 - 14 - 23 - items.length,
    windowSecs: 1800,
    updatedAt: now - 95,
    note: null,
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

// ---------------------------------------------------------------- 磁盘（M1）
//
// 数字取自 2026-10-26 真机扫描（release 构建 17.6 s / 主数字 55.26 GB），
// 让浏览器里能看到与桌面端一致的量级与分层，避免「演示 3 GB、真机 55 GB」的失真。

const GB = 1e9;

function devGroup(
  key: string,
  label: string,
  level: "l1" | "l2" | "l3" | "l4",
  totalBytes: number,
  entryCount: number,
  deadweight: CacheGroup["deadweight"] = null,
  cleanable = true,
  details: DetailItem[] = [],
): CacheGroup {
  return { key, label, root: `/Users/xuyi/${key}`, level, totalBytes, entryCount, cleanable, details, deadweight };
}

export function devDiskReport(): DiskReport {
  const platformStats: PlatformStat[] = [
    { platform: "linux", entryCount: 134, bytes: 4.22 * GB },
    { platform: "win32", entryCount: 76, bytes: 2.24 * GB },
    { platform: "android", entryCount: 25, bytes: 0.279 * GB },
    { platform: "freebsd", entryCount: 21, bytes: 0.191 * GB },
  ];

  const top: DeadweightItem[] = [
    ["npm-@next-swc-linux-x64-musl-14.2.33", "linux", 0.207],
    ["npm-@next-swc-win32-x64-msvc-15.5.13", "win32", 0.195],
    ["npm-@next-swc-win32-x64-msvc-15.5.19", "win32", 0.195],
    ["npm-@next-swc-linux-x64-gnu-15.5.13", "linux", 0.191],
    ["npm-@next-swc-linux-x64-gnu-15.5.19", "linux", 0.191],
    ["npm-@next-swc-linux-x64-musl-15.5.13", "linux", 0.190],
    ["npm-@next-swc-linux-arm64-musl-14.2.33", "linux", 0.187],
    ["npm-@next-swc-linux-arm64-musl-15.5.13", "linux", 0.181],
  ].map(([name, platform, gb]) => ({
    name: name as string,
    path: `/Users/xuyi/Library/Caches/Yarn/v6/${name}`,
    bytes: (gb as number) * GB,
    platform: platform as string,
    verdict: "deadweight" as const,
  }));

  const yarn = devGroup("yarn", "Yarn 缓存", "l2", 15.34 * GB, 6156, {
    bytes: 6.94 * GB,
    nativeBytes: 1.0 * GB,
    otherBytes: 7.4 * GB,
    entryCount: 256,
    byPlatform: platformStats,
    items: top,
    ratio: 6.91,
  });

  // A4/A5 明细：age 天数覆盖 <30 / >180 两档，UI 标灰逻辑在浏览器里可验证
  const nmDetails: DetailItem[] = [
    ["Desktop/git/prune/node_modules", 3.1 * GB, 3],
    ["Desktop/git/wcs-homepage/node_modules", 2.4 * GB, 12],
    ["Documents/approval/node_modules", 1.8 * GB, 210],
    ["code/legacy/node_modules", 1.2 * GB, 420],
    ["work/demo/node_modules", 0.6 * GB, 33],
  ].map(([p, bytes, days]) => ({
    name: p as string,
    path: `/Users/xuyi/${p}`,
    bytes: bytes as number,
    ageDays: days as number,
    note: null,
  }));
  const simDetails: DetailItem[] = [
    { name: "04BD6806-22EE-4BCD-B99C-7E2253408723", path: "/Users/xuyi/Library/Developer/CoreSimulator/Devices/04BD6806", bytes: 2.1 * GB, ageDays: 400, note: "iPad mini (6th generation) · iOS 17.5" },
    { name: "1A2B3C4D-1111-2222-3333-444455556666", path: "/Users/xuyi/Library/Developer/CoreSimulator/Devices/1A2B3C4D", bytes: 1.0 * GB, ageDays: 8, note: "iPhone 15 Pro · iOS 18.0" },
  ];
  const pwDetails: DetailItem[] = [
    { name: "chromium-1179", path: "/Users/xuyi/Library/Caches/ms-playwright/chromium-1179", bytes: 0.42 * GB, ageDays: 5, note: null },
    { name: "chromium-1155", path: "/Users/xuyi/Library/Caches/ms-playwright/chromium-1155", bytes: 0.4 * GB, ageDays: 260, note: null },
    { name: "chromium_headless_shell-1179", path: "/Users/xuyi/Library/Caches/ms-playwright/chromium_headless_shell-1179", bytes: 0.21 * GB, ageDays: 5, note: null },
  ];

  return {
    scannedAt: Math.floor(Date.now() / 1000) - 42,
    scanElapsedMs: 17560,
    groups: [
      yarn,
      devGroup("npm", "npm 缓存", "l2", 8.53 * GB, 3),
      devGroup("pnpm", "pnpm store", "l2", 7.72 * GB, 3),
      devGroup("cargo", "Cargo registry", "l2", 1.37 * GB, 4),
      devGroup("homebrew", "Homebrew 下载缓存", "l2", 0.176 * GB, 63),
      devGroup("deriveddata", "DerivedData（编译产物）", "l2", 0.071 * GB, 5),
      devGroup("bun", "Bun 安装缓存", "l2", 0.4e6, 3),
      // 聚合项：真机上 root 是 home，整项不可清理；A4 明细逐项确认
      devGroup("node-modules", "项目依赖（55 个 node_modules）", "l3", 17.02 * GB, 55, null, false, nmDetails),
      devGroup("simulator", "Simulator 设备", "l3", 3.75 * GB, 12, null, true, simDetails),
      devGroup("playwright", "Playwright 浏览器", "l3", 1.29 * GB, 10, null, true, pwDetails),
      devGroup("docker-vm", "Docker VM 磁盘", "l4", 3.17 * GB, 1),
    ],
    dup: {
      managers: [
        ["Yarn 缓存", 15.34 * GB],
        ["npm 缓存", 8.53 * GB],
        ["pnpm store", 7.72 * GB],
        ["Bun 安装缓存", 0.4e6],
      ],
      totalBytes: 31.59 * GB,
      advice:
        "4 个包管理器并存（Yarn 缓存 / npm 缓存 / pnpm store / Bun 安装缓存），同一批依赖各存一份；保留正在用的一个即可，其余可整体移入废纸篓",
    },
    totals: {
      deadweight: 6.94 * GB,
      l2: 26.27 * GB,
      l3: 22.05 * GB,
      l4: 3.17 * GB,
      reclaimable: 55.26 * GB,
      cacheTotal: 58.43 * GB,
    },
  };
}

export function devDiskPreview(): ReclaimPreview {
  const report = devDiskReport();
  const yarn = report.groups[0];
  const items = (yarn.deadweight?.items ?? []).map((it) => ({
    path: it.path,
    label: `Yarn 缓存 · ${it.name}`,
    bytes: it.bytes,
    level: "l1" as const,
  }));
  return {
    items,
    totalBytes: items.reduce((sum, i) => sum + i.bytes, 0),
    needsConfirm: false,
    notes: ["3 个 darwin-x64 条目依赖 Rosetta，已保留在清单之外"],
  };
}
