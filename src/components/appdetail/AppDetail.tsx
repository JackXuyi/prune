/**
 * D4：应用详情页（二级页）。
 *
 * 结构：吸顶栏 → Hero → 四联资源卡 → 近 7 天历史 → 进程明细 → 端口 / 工作区 / 泄漏证据。
 * 数据全部来自已有 store / IPC，**不新增采集**（图标与历史除外，见 D1/D3）。
 */
import { useEffect, useMemo, useRef, useState } from "react";

import { AppIcon } from "../AppIcon";
import { EmptyState } from "../EmptyArt";
import { LeakEvidence } from "../LeakCard";
import { AreaChart } from "../charts/AreaChart";
import { StackBar } from "../charts/StackBar";
import { ChevronRightIcon, RefreshIcon } from "../icons";
import { Badge } from "../primitives";
import { HistoryTrack } from "./HistoryTrack";
import { ProcTable } from "./ProcTable";
import { ResourceTile } from "./ResourceTile";
import { buildAppSummary } from "../../lib/appSummary";
import { writeClipboard } from "../../lib/clipboard";
import { devAppDisk } from "../../lib/devFixture";
import { formatBytes, formatPercent, formatRelative } from "../../lib/format";
import { appDisk, isTauri, revealPath } from "../../lib/ipc";
import { useAppHistory } from "../../lib/useAppHistory";
import { useAppIcons } from "../../lib/useAppIcons";
import { useMemStore } from "../../stores/memStore";
import { useNetSpeedStore } from "../../stores/netStore";
import { usePortStore } from "../../stores/portStore";
import { diskEmpty, diskTotal, type AppDisk } from "../../types/appdisk";
import { isExact, totalOf, type ProcMem } from "../../types/mem";
import { exposureMeta, formatSpeed, netSpeedOfPids } from "../../types/net";

export function AppDetail({
  familyName,
  onBack,
  onOpenPorts,
  onOpenProcess,
  onKillMenu,
}: {
  familyName: string;
  onBack: () => void;
  onOpenPorts?: (port: number) => void;
  onOpenProcess?: (pid: number) => void;
  onKillMenu?: (proc: ProcMem, x: number, y: number) => void;
}) {
  const snapshot = useMemStore((s) => s.snapshot);
  const processes = useMemStore((s) => s.processes);
  const netSample = useNetSpeedStore((s) => s.sample);
  const ports = usePortStore((s) => s.snapshot);

  const family = snapshot?.families.find((f) => f.name === familyName) ?? null;
  const members = useMemo(
    () => processes.filter((p) => p.family === familyName).sort((a, b) => b.rss - a.rss),
    [processes, familyName],
  );
  const exe = members.find((m) => m.exe)?.exe ?? null;
  const pids = useMemo(() => members.map((m) => m.pid), [members]);

  // 图标只取这一张（Hero 64px；卡片 28px 由列表自己取）
  const icons = useAppIcons(useMemo(() => [{ family: familyName, exe }], [familyName, exe]));

  const watched = useMemo(() => {
    if (!ports) return [];
    return ports.entries.filter(
      (e) => e.watched && e.listeners.some((b) => b.pid != null && pids.includes(b.pid)),
    );
  }, [ports, pids]);
  const conns = ports ? ports.connsByPid : null;
  const established = useMemo(() => {
    if (!conns) return null;
    return pids.reduce((n, pid) => n + (conns[String(pid)]?.established ?? 0), 0);
  }, [conns, pids]);
  const net = netSample && netSample.status === "ok" ? netSpeedOfPids(netSample, pids) : null;

  // CPU / 网速 60 点缓冲（10 s / 1 s 各一拍；卸载即释放）
  const cpuHist = useRef<number[]>([]);
  const netHist = useRef<number[]>([]);
  const [, tick] = useState(0);
  const lastSnap = useRef<typeof snapshot>(null);
  const lastNet = useRef<typeof netSample>(null);

  useEffect(() => {
    if (!snapshot || lastSnap.current === snapshot) return;
    lastSnap.current = snapshot;
    const v = family?.cpu;
    if (v != null && Number.isFinite(v)) {
      cpuHist.current = [...cpuHist.current, v].slice(-60);
      tick((n) => n + 1);
    }
  }, [snapshot, family]);

  useEffect(() => {
    if (!netSample || lastNet.current === netSample) return;
    lastNet.current = netSample;
    if (netSample.status !== "ok") return;
    const v = net?.downBps ?? 0;
    netHist.current = [...netHist.current, v].slice(-60);
    tick((n) => n + 1);
  }, [netSample, net]);

  // D5：历史区数据在页面级持有——复制摘要也要用它，避免二次请求
  const history = useAppHistory(familyName);

  // D7：复制摘要（按钮态 1.6 s 后复原）
  const [copy, setCopy] = useState<"idle" | "ok" | "fail">("idle");
  const copyTimer = useRef<number | null>(null);
  const onCopy = async () => {
    if (!family) return;
    const text = buildAppSummary({
      family: familyName,
      bundlePath: exe ? exe.slice(0, exe.indexOf(".app") + 4) : null,
      processCount: family.processCount,
      established,
      cpu: family.cpu ?? null,
      mem: totalOf(family),
      swap: family.swapped ?? 0,
      exact: isExact(family),
      deepAvailable: snapshot?.deepAvailable ?? false,
      disk,
      diskBusy,
      net,
      netStatus: netSample?.status ?? null,
      netNote: netSample?.note ?? null,
      ports: watched.map((e) => ({ port: e.port, label: exposureMeta(e.exposure).label })),
      history: history.rows,
      leak: snapshot?.leaks?.items.find((i) => i.family === familyName) ?? null,
      topProcs: members.slice(0, 3).map((m) => ({ name: m.name, bytes: m.footprint ?? m.rss })),
    });
    const ok = await writeClipboard(text);
    setCopy(ok ? "ok" : "fail");
    if (copyTimer.current != null) window.clearTimeout(copyTimer.current);
    copyTimer.current = window.setTimeout(() => setCopy("idle"), 1600);
  };

  // 磁盘三层：按需测量（Rust 侧 30 min TTL），进入详情测一次
  const [disk, setDisk] = useState<AppDisk | null>(null);
  const [diskBusy, setDiskBusy] = useState(false);
  const measureDisk = async (force = false) => {
    setDiskBusy(true);
    try {
      setDisk(isTauri() ? await appDisk(familyName, exe, force) : devAppDisk(familyName));
    } catch {
      setDisk(null);
    } finally {
      setDiskBusy(false);
    }
  };
  useEffect(() => {
    setDisk(null);
    void measureDisk(false);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [familyName, exe]);

  // Esc 返回（输入框聚焦时不拦截）
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const tag = (e.target as HTMLElement | null)?.tagName;
      if (e.key === "Escape" && tag !== "INPUT" && tag !== "TEXTAREA") onBack();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onBack]);

  if (!snapshot) {
    return (
      <div className="flex min-h-0 flex-1 items-center justify-center text-[11px] text-ink-faint">
        正在采样…
      </div>
    );
  }
  if (!family) {
    return (
      <div className="flex min-h-0 flex-1 flex-col">
        <TopBar onBack={onBack} />
        <EmptyState
          title="这个应用已经不在运行"
          note={`「${familyName}」的进程全部退出，详情页没有可显示的数据。`}
        />
      </div>
    );
  }

  const exact = isExact(family);
  const mem = totalOf(family);
  const swap = family.swapped ?? 0;
  const swapRatioValue = mem > 0 ? swap / mem : null;
  const cpuValue = family.cpu;
  const leakItem = snapshot.leaks?.items.find((i) => i.family === familyName) ?? null;
  const bundlePath = exe ? exe.slice(0, exe.indexOf(".app") + 4) : null;

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <TopBar onBack={onBack} onRefresh={() => void measureDisk(true)} busy={diskBusy} />

      <div className="min-h-0 flex-1 overflow-y-auto px-5 py-3">
        <div className="prune-anim space-y-3">
          {/* Hero */}
          <section className="flex items-start gap-3">
            <AppIcon family={familyName} icon={icons[familyName]} size={64} />
            <div className="min-w-0 flex-1">
              <div className="flex items-center gap-2">
                <h1 className="truncate text-[18px] font-semibold tracking-tight text-ink">
                  {familyName}
                </h1>
                <Badge tone={snapshot.deepAvailable ? "accent" : "warn"}>
                  {snapshot.deepAvailable ? "footprint 口径" : "RSS 口径 · 不含换出"}
                </Badge>
              </div>
              <p className="mt-0.5 truncate font-mono text-[10px] text-ink-faint">
                {bundlePath ?? "无 .app（系统进程 / CLI）"} · {family.processCount} 进程 ·{" "}
                {established == null ? "连接未扫" : `${established} 连接`}
              </p>
              <div className="mt-2 flex flex-wrap items-center gap-1.5">
                <button
                  type="button"
                  onClick={() => bundlePath && void revealPath(bundlePath)}
                  disabled={!bundlePath}
                  className="rounded-md border border-hairline bg-white/[0.04] px-2 py-1 text-[10px] text-ink-dim transition-colors hover:text-ink disabled:opacity-40"
                >
                  在 Finder 显示
                </button>
                <button
                  type="button"
                  onClick={(e) => {
                    const main = members[0];
                    if (main) onKillMenu?.(main, e.clientX, e.clientY);
                  }}
                  disabled={!members[0] || !onKillMenu}
                  className="rounded-md border border-danger/30 bg-danger/[0.08] px-2 py-1 text-[10px] text-danger transition-colors hover:bg-danger/[0.14] disabled:opacity-40"
                >
                  结束进程…
                </button>
                <button
                  type="button"
                  onClick={() => void onCopy()}
                  className="rounded-md border border-[color-mix(in_srgb,var(--color-domain-cpu)_32%,transparent)] bg-[color-mix(in_srgb,var(--color-domain-cpu)_10%,transparent)] px-2 py-1 text-[10px] text-domain-cpu transition-colors hover:bg-[color-mix(in_srgb,var(--color-domain-cpu)_16%,transparent)]"
                  title={copy === "fail" ? "复制失败：剪贴板不可用（详见控制台）" : "把当前读数与近 7 天走势粘成 Markdown"}
                >
                  {copy === "ok" ? "✓ 已复制" : copy === "fail" ? "✗ 复制失败" : "复制摘要"}
                </button>
              </div>
            </div>
          </section>

          {/* 四联资源卡 */}
          <div className="grid grid-cols-4 gap-2.5">
            <ResourceTile
              label="CPU"
              dotColor="var(--color-domain-cpu)"
              value={
                cpuValue == null ? (
                  "—"
                ) : (
                  <>
                    {Math.round(cpuValue)}
                    <span className="ml-0.5 text-[10px] text-ink-faint">%</span>
                  </>
                )
              }
              chart={
                <AreaChart
                  values={cpuHist.current}
                  color="var(--color-domain-cpu)"
                  height={26}
                  ariaLabel="近 10 分钟 CPU 走势"
                />
              }
              note="近 10 min · 10 s/点 · 单核口径"
              hint="单核口径：100% = 占满一个核，多线程可超过 100%"
            />
            <ResourceTile
              label="内存"
              dotColor="var(--color-info)"
              value={
                <>
                  {exact ? "" : "≈"}
                  {formatBytes(mem)}
                </>
              }
              chart={
                mem > 0 ? (
                  <StackBar
                    height={5}
                    ariaLabel={`内存 ${formatBytes(mem)}，其中换出 ${formatBytes(swap)}`}
                    segments={[
                      { key: "rss", label: "常驻", value: Math.max(0, mem - swap), tone: "info" },
                      { key: "swap", label: "换出", value: swap, tone: "warn" },
                    ]}
                  />
                ) : null
              }
              note={`footprint 口径${swap > 0 ? ` · ${formatPercent(swapRatioValue ?? swap / Math.max(1, mem))} 在磁盘` : ""}`}
              hint="footprint 含被换出到磁盘的部分；未深探的子进程按 RSS 计入（下界）"
            />
            <ResourceTile
              label="磁盘"
              dotColor="var(--color-accent)"
              value={
                disk == null ? (
                  <span className="text-[13px] text-ink-faint">{diskBusy ? "测量中…" : "未测量"}</span>
                ) : diskEmpty(disk) ? (
                  <span className="text-[13px] text-ink-faint">不可读</span>
                ) : (
                  formatBytes(diskTotal(disk))
                )
              }
              chart={
                disk && !diskEmpty(disk) ? (
                  <StackBar
                    height={5}
                    ariaLabel="磁盘三层构成"
                    segments={[
                      { key: "b", label: "本体", value: disk.bundle?.bytes ?? 0, tone: "accent" },
                      { key: "c", label: "缓存", value: disk.cache?.bytes ?? 0, tone: "info" },
                      { key: "d", label: "数据", value: disk.data?.bytes ?? 0, tone: "neutral" },
                    ]}
                  />
                ) : null
              }
              note={
                disk
                  ? `本体 ${disk.bundle ? formatBytes(disk.bundle.bytes) : "—"} · 缓存 ${disk.cache ? formatBytes(disk.cache.bytes) : "—"} · 数据 ${disk.data ? formatBytes(disk.data.bytes) : "—"}`
                  : "进入页面按需测量（30 min 缓存）"
              }
              hint="du 名义大小之和，APFS 克隆会被重复计入；不等于删了能省这么多"
            />
            <ResourceTile
              label="网速"
              dotColor="var(--color-domain-net)"
              value={
                netSample == null
                  ? "尚未采样"
                  : netSample.status !== "ok"
                    ? netSample.status === "warmup"
                      ? "采样中…"
                      : "不可读"
                    : net
                      ? `↓${formatSpeed(net.downBps)}`
                      : "—"
              }
              chart={
                <AreaChart
                  values={netHist.current}
                  color="var(--color-domain-net)"
                  height={26}
                  ariaLabel="近 60 秒下载走势"
                />
              }
              note={net ? `↑ ${formatSpeed(net.upBps)} · 1 s 差分 · 仅非回环` : "1 s 差分 · 仅非回环"}
              hint="只在「应用」tab 可见时采样；切走不采，读数不会假装在动"
            />
          </div>

          {/* 近 7 天 */}
          <HistoryTrack rows={history.rows} loading={history.loading} error={history.error} />

          {/* 进程明细 */}
          <ProcTable members={members} onOpenProcess={onOpenProcess} onKillMenu={onKillMenu} />

          {/* 端口与连接 */}
          <section className="rounded-xl border border-hairline-soft bg-white/[0.02] px-3 py-2.5">
            <div className="flex items-center gap-2">
              <span className="text-[11px] font-medium text-ink">端口与连接</span>
              <span className="text-[9.5px] text-ink-faint">
                {established == null ? "连接数未扫" : `${established} 条 ESTABLISHED`}
              </span>
              <div className="grow" />
              {onOpenPorts && watched[0] ? (
                <button
                  type="button"
                  onClick={() => watched[0] && onOpenPorts(watched[0].port)}
                  className="flex items-center gap-1 text-[9.5px] text-ink-faint transition-colors hover:text-ink"
                >
                  去端口页 <ChevronRightIcon size={10} />
                </button>
              ) : null}
            </div>
            {!ports ? (
              <p className="mt-1.5 text-[10px] text-ink-faint">端口尚未扫描，进入端口页扫一轮</p>
            ) : watched.length === 0 ? (
              <p className="mt-1.5 text-[10px] text-ink-faint">这个应用当前没有监听端口</p>
            ) : (
              <div className="mt-1.5 flex flex-wrap gap-1.5">
                {watched.map((e) => {
                  const meta = exposureMeta(e.exposure);
                  const color =
                    meta.tone === "warn"
                      ? "var(--color-warn)"
                      : meta.tone === "info"
                        ? "var(--color-info)"
                        : "var(--color-ink-dim)";
                  return (
                    <button
                      key={e.port}
                      type="button"
                      onClick={() => onOpenPorts?.(e.port)}
                      className="rounded-md border px-1.5 py-[3px] font-mono text-[10px] tabular-nums transition-colors"
                      style={{
                        borderColor: `color-mix(in srgb, ${color} 32%, transparent)`,
                        background: `color-mix(in srgb, ${color} 10%, transparent)`,
                        color,
                      }}
                      title={`${e.port} · ${meta.label}：${meta.hint}`}
                    >
                      {e.port} <span className="opacity-70">{meta.label}</span>
                    </button>
                  );
                })}
              </div>
            )}
          </section>

          {/* 工作区 */}
          {family.workspaces.length > 0 ? (
            <section className="rounded-xl border border-hairline-soft bg-white/[0.02] px-3 py-2.5">
              <div className="text-[11px] font-medium text-ink">工作区归属</div>
              <ul className="mt-1 space-y-1">
                {family.workspaces.slice(0, 4).map((w) => (
                  <li key={w.path} className="flex items-baseline gap-2 text-[10px]">
                    <span className="min-w-0 flex-1 truncate text-ink-dim" title={w.path}>
                      {w.path}
                    </span>
                    <span className="shrink-0 font-mono tabular-nums text-ink-faint">
                      {w.processCount} 进程 · {formatBytes(w.footprint ?? w.rss)}
                    </span>
                  </li>
                ))}
              </ul>
            </section>
          ) : null}

          {/* 泄漏证据：只在命中时出现（沉默即正常） */}
          {leakItem ? <LeakEvidence item={leakItem} /> : null}

          <p className="pb-2 text-[9px] leading-relaxed text-ink-faint">
            口径：CPU 单核 · 内存 footprint（含换出）· 磁盘 du 名义大小 · 网速 1 s 差分（仅非回环）·
            历史为 RSS 小时聚合 · 上次深探 {formatRelative(snapshot.deepUpdatedAt ?? null)}
          </p>
        </div>
      </div>
    </div>
  );
}

function TopBar({
  onBack,
  onRefresh,
  busy,
}: {
  onBack: () => void;
  onRefresh?: () => void;
  busy?: boolean;
}) {
  return (
    <div className="flex h-11 shrink-0 items-center gap-2 border-b border-hairline px-4">
      <button
        type="button"
        onClick={onBack}
        className="flex items-center gap-1 rounded-md px-1.5 py-1 text-[11px] text-accent transition-colors hover:bg-white/[0.06]"
      >
        ← 返回应用
      </button>
      <div className="flex-1" />
      <span className="text-[9.5px] text-ink-faint">Esc 返回</span>
      {onRefresh ? (
        <button
          type="button"
          onClick={onRefresh}
          disabled={busy}
          className="grid size-6 place-items-center rounded-md text-ink-faint transition-colors hover:bg-white/[0.06] hover:text-ink disabled:opacity-50"
          title="重新测量磁盘三层"
        >
          <RefreshIcon size={12} className={busy ? "animate-spin" : ""} />
        </button>
      ) : null}
    </div>
  );
}
