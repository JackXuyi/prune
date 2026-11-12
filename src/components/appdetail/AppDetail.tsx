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
import { Badge, Btn } from "../primitives";
import { HistoryTrack } from "./HistoryTrack";
import { ProcTable } from "./ProcTable";
import { ResourceTile } from "./ResourceTile";
import { useI18n } from "../../i18n/store";
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
  const { t, locale } = useI18n();

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
      locale,
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
      ports: watched.map((e) => ({ port: e.port, label: exposureMeta(e.exposure, locale).label })),
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
        {t("common.progress.sampling")}
      </div>
    );
  }
  if (!family) {
    // 已退出不是错误：历史还在，读数只是「退出前最后一次快照」（口径必须说清楚）
    return (
      <div className="flex min-h-0 flex-1 flex-col">
        <TopBar onBack={onBack} />
        <EmptyState
          art="no-record"
          title={t("resources.detail.notRunningTitle")}
          description={t("resources.detail.notRunningNote", { name: familyName })}
          footnote={t("resources.detail.exitedFootnote")}
          action={
            <Btn variant="ghost" onClick={onBack}>
              {t("resources.detail.backToList")}
            </Btn>
          }
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
                  {snapshot.deepAvailable
                    ? t("resources.detail.badgeFootprint")
                    : t("resources.detail.badgeRss")}
                </Badge>
              </div>
              <p className="mt-0.5 truncate font-mono text-[10px] text-ink-faint">
                {t("resources.detail.heroMeta", {
                  path: bundlePath ?? t("resources.detail.pathNone"),
                  procs: family.processCount,
                  conns:
                    established == null
                      ? t("resources.detail.connsUnscanned")
                      : t("resources.detail.conns", { n: established }),
                })}
              </p>
              <div className="mt-2 flex flex-wrap items-center gap-1.5">
                <button
                  type="button"
                  onClick={() => bundlePath && void revealPath(bundlePath)}
                  disabled={!bundlePath}
                  className="rounded-md border border-hairline bg-chip px-2 py-1 text-[10px] text-ink-dim transition-colors hover:text-ink disabled:opacity-40"
                >
                  {t("common.action.revealAndSelect")}
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
                  {t("resources.detail.kill")}
                </button>
                <button
                  type="button"
                  onClick={() => void onCopy()}
                  className="rounded-md border border-[color-mix(in_srgb,var(--color-domain-cpu)_32%,transparent)] bg-[color-mix(in_srgb,var(--color-domain-cpu)_10%,transparent)] px-2 py-1 text-[10px] text-domain-cpu transition-colors hover:bg-[color-mix(in_srgb,var(--color-domain-cpu)_16%,transparent)]"
                  title={
                    copy === "fail"
                      ? t("resources.detail.copyFailTip")
                      : t("resources.detail.copyTip")
                  }
                >
                  {copy === "ok"
                    ? t("resources.detail.copyOk")
                    : copy === "fail"
                      ? t("resources.detail.copyFail")
                      : t("resources.detail.copy")}
                </button>
              </div>
            </div>
          </section>

          {/* 四联资源卡 */}
          <div className="grid grid-cols-4 gap-2.5">
            <ResourceTile
              label={t("resources.tile.cpu")}
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
                  ariaLabel={t("resources.tile.cpuAria")}
                />
              }
              note={t("resources.tile.cpuNote")}
              hint={t("resources.tile.cpuHint")}
            />
            <ResourceTile
              label={t("resources.tile.mem")}
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
                    ariaLabel={t("resources.tile.memAria", {
                      mem: formatBytes(mem),
                      swap: formatBytes(swap),
                    })}
                    segments={[
                      {
                        key: "rss",
                        label: t("resources.tile.rssSeg"),
                        value: Math.max(0, mem - swap),
                        tone: "info",
                      },
                      { key: "swap", label: t("resources.tile.swapSeg"), value: swap, tone: "warn" },
                    ]}
                  />
                ) : null
              }
              note={
                swap > 0
                  ? t("resources.tile.memNoteSwap", {
                      pct: formatPercent(swapRatioValue ?? swap / Math.max(1, mem)),
                    })
                  : t("resources.tile.memNote")
              }
              hint={t("resources.tile.memHint")}
            />
            <ResourceTile
              label={t("resources.tile.disk")}
              dotColor="var(--color-accent)"
              value={
                disk == null ? (
                  <span className="text-[13px] text-ink-faint">
                    {diskBusy ? t("common.progress.measuring") : t("resources.tile.diskUnmeasured")}
                  </span>
                ) : diskEmpty(disk) ? (
                  <span className="text-[13px] text-ink-faint">{t("common.state.unavailable")}</span>
                ) : (
                  formatBytes(diskTotal(disk))
                )
              }
              chart={
                disk && !diskEmpty(disk) ? (
                  <StackBar
                    height={5}
                    ariaLabel={t("resources.tile.diskAria")}
                    segments={[
                      {
                        key: "b",
                        label: t("resources.tile.bundleSeg"),
                        value: disk.bundle?.bytes ?? 0,
                        tone: "accent",
                      },
                      {
                        key: "c",
                        label: t("resources.tile.cacheSeg"),
                        value: disk.cache?.bytes ?? 0,
                        tone: "info",
                      },
                      {
                        key: "d",
                        label: t("resources.tile.dataSeg"),
                        value: disk.data?.bytes ?? 0,
                        tone: "neutral",
                      },
                    ]}
                  />
                ) : null
              }
              note={
                disk
                  ? t("resources.tile.diskNote", {
                      bundle: disk.bundle ? formatBytes(disk.bundle.bytes) : "—",
                      cache: disk.cache ? formatBytes(disk.cache.bytes) : "—",
                      data: disk.data ? formatBytes(disk.data.bytes) : "—",
                    })
                  : t("resources.tile.diskNoteIdle")
              }
              hint={t("resources.tile.diskHint")}
            />
            <ResourceTile
              label={t("resources.tile.net")}
              dotColor="var(--color-domain-net)"
              value={
                netSample == null
                  ? t("resources.netShort.notSampled")
                  : netSample.status !== "ok"
                    ? netSample.status === "warmup"
                      ? t("resources.netShort.sampling")
                      : t("resources.netShort.unreadable")
                    : net
                      ? `↓${formatSpeed(net.downBps)}`
                      : "—"
              }
              chart={
                <AreaChart
                  values={netHist.current}
                  color="var(--color-domain-net)"
                  height={26}
                  ariaLabel={t("resources.tile.netAria")}
                />
              }
              note={
                net
                  ? t("resources.tile.netNote", { up: formatSpeed(net.upBps) })
                  : t("resources.tile.netNoteIdle")
              }
              hint={t("resources.tile.netHint")}
            />
          </div>

          {/* 历史走势（24 h / 7 天 / 30 天，M25） */}
          <HistoryTrack
            rows={history.rows}
            daily={history.daily}
            loading={history.loading}
            error={history.error}
          />

          {/* 进程明细 */}
          <ProcTable members={members} onOpenProcess={onOpenProcess} onKillMenu={onKillMenu} />

          {/* 端口 / 工作区：两列并排，信息密度更高也更省滚动 */}
          <div className={`grid gap-2.5 ${family.workspaces.length > 0 ? "grid-cols-2" : "grid-cols-1"}`}>
          {/* 端口与连接 */}
          <section className="rounded-card border border-hairline-soft bg-surface-raised px-3 py-2.5">
            <div className="flex items-center gap-2">
              <span className="text-[11px] font-medium text-ink">{t("resources.ports.title")}</span>
              <span className="text-[9.5px] text-ink-faint">
                {established == null
                  ? t("resources.detail.connsUnscanned")
                  : t("resources.ports.established", { n: established })}
              </span>
              <div className="grow" />
              {onOpenPorts && watched[0] ? (
                <button
                  type="button"
                  onClick={() => watched[0] && onOpenPorts(watched[0].port)}
                  className="flex items-center gap-1 text-[9.5px] text-ink-faint transition-colors hover:text-ink"
                >
                  {t("resources.ports.goToPorts")} <ChevronRightIcon size={10} />
                </button>
              ) : null}
            </div>
            {!ports ? (
              <p className="mt-1.5 text-[10px] text-ink-faint">{t("resources.ports.unscanned")}</p>
            ) : watched.length === 0 ? (
              <p className="mt-1.5 text-[10px] text-ink-faint">{t("resources.ports.none")}</p>
            ) : (
              <div className="mt-1.5 flex flex-wrap gap-1.5">
                {watched.map((e) => {
                  const meta = exposureMeta(e.exposure, locale);
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
            <section className="rounded-card border border-hairline-soft bg-surface-raised px-3 py-2.5">
              <div className="text-[11px] font-medium text-ink">{t("resources.workspace.title")}</div>
              <ul className="mt-1 space-y-1">
                {family.workspaces.slice(0, 4).map((w) => (
                  <li key={w.path} className="flex items-baseline gap-2 text-[10px]">
                    <span className="min-w-0 flex-1 truncate text-ink-dim" title={w.path}>
                      {w.path}
                    </span>
                    <span className="shrink-0 font-mono tabular-nums text-ink-faint">
                      {t("resources.workspace.line", {
                        n: w.processCount,
                        size: formatBytes(w.footprint ?? w.rss),
                      })}
                    </span>
                  </li>
                ))}
              </ul>
            </section>
          ) : null}
          </div>

          {/* 泄漏证据：只在命中时出现（沉默即正常） */}
          {leakItem ? <LeakEvidence item={leakItem} /> : null}

          <p className="pb-2 text-[9px] leading-relaxed text-ink-faint">
            {t("resources.detail.footnote", {
              when: formatRelative(snapshot.deepUpdatedAt ?? null, locale),
            })}
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
  const { t } = useI18n();
  return (
    <div className="flex h-11 shrink-0 items-center gap-2 border-b border-hairline px-4">
      <button
        type="button"
        onClick={onBack}
        className="flex items-center gap-1 rounded-md px-1.5 py-1 text-[11px] text-accent transition-colors hover:bg-chip-hover"
      >
        {t("resources.detail.back")}
      </button>
      <div className="flex-1" />
      <span className="text-[9.5px] text-ink-faint">{t("resources.detail.esc")}</span>
      {onRefresh ? (
        <button
          type="button"
          onClick={onRefresh}
          disabled={busy}
          className="grid size-6 place-items-center rounded-md text-ink-faint transition-colors hover:bg-chip-hover hover:text-ink disabled:opacity-50"
          title={t("resources.detail.refreshDiskTip")}
          aria-label={t("resources.detail.refreshDiskTip")}
        >
          <RefreshIcon size={12} className={busy ? "animate-spin" : ""} />
        </button>
      ) : null}
    </div>
  );
}
