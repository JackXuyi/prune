import { useEffect, useState, type ReactNode } from "react";
import { LeakBanner } from "../components/LeakCard";
import { NetDirectionRow, StatsRow } from "../components/stats/StatsRow";
import { formatRelative } from "../lib/format";
import { fireAndForget, openMain } from "../lib/ipc";
import {
  cpuReadout,
  diskReadout,
  memoryReadout,
  netDirectionReadout,
  portReadout,
} from "../lib/statsReadout";
import { useVolumeInfo } from "../lib/useVolumeInfo";
import { useDiskStore } from "../stores/diskStore";
import { useMemStore } from "../stores/memStore";
import { useNetTotalStore } from "../stores/netTotalStore";
import { usePortStore } from "../stores/portStore";
import { ChevronRightIcon, ExpandIcon, RefreshIcon, ScissorsIcon } from "../components/icons";
import { Badge } from "../components/primitives";
import { UpdateCard } from "../components/UpdateCard";

/**
 * 菜单栏面板（380×432）：单页统计。
 *
 * 窗口高度贴合内容（426 + 6 余量，算式见 `tray.rs: PANEL_H`），不是固定 560：
 * 面板只有一屏、没有 tab 切换，多出来的高度只会是底部空白。
 *
 * 原来的「应用」「概况」两个 tab 都已撤掉——应用级明细在主窗口「应用资源」，
 * 内存压力 / swap / 换出受害者在主窗口「内存」。面板是「瞄一眼」场景。
 *
 * 读数与主窗口统计 tab 共用 `lib/statsReadout`，面板只换排版密度（48 px 行），
 * 不重算口径。
 */
export function MenuBarPanel() {
  const [refreshNonce, setRefreshNonce] = useState(0);
  const snapshot = useMemStore((s) => s.snapshot);
  const loading = useMemStore((s) => s.loading);
  const refreshing = useMemStore((s) => s.refreshing);
  const error = useMemStore((s) => s.error);
  const refresh = useMemStore((s) => s.refresh);
  const refreshPorts = usePortStore((s) => s.refresh);
  const refreshNet = useNetTotalStore((s) => s.refresh);

  /** 一个按钮刷三个域：内存常采、端口重扫、网速重取快照（都是只读动作） */
  const onRefresh = () => {
    void refresh();
    void refreshPorts();
    void refreshNet();
    setRefreshNonce((n) => n + 1);
  };

  return (
    <div className="flex h-screen flex-col p-2">
      <div className="flex min-h-0 flex-1 flex-col overflow-hidden rounded-[14px] border border-white/[0.07] bg-[#0f1216]/95 shadow-[0_18px_50px_-12px_rgba(0,0,0,0.8)] backdrop-blur-2xl">
        <header className="flex h-11 shrink-0 items-center gap-2 px-3.5">
          <ScissorsIcon size={14} className="text-accent" />
          <span className="text-[13px] font-semibold tracking-tight text-ink">Prune</span>
          {snapshot ? (
            snapshot.deepAvailable ? (
              <Badge tone="accent" title="footprint 口径：含被换出到磁盘的部分">
                footprint
              </Badge>
            ) : (
              <Badge tone="warn" title="footprint 不可用，当前全部为 RSS 口径，不含换出量">
                RSS 口径
              </Badge>
            )
          ) : null}
          <div className="flex-1" />
          <IconButton label="立即刷新" onClick={onRefresh}>
            <RefreshIcon size={13} className={refreshing ? "animate-spin" : ""} />
          </IconButton>
          <IconButton label="打开主窗口" onClick={() => fireAndForget(openMain(), "open_main")}>
            <ExpandIcon size={13} />
          </IconButton>
        </header>

        {/* pt-1：tab 行删除后补一点呼吸，否则首个读数行紧贴 header */}
        <div className="min-h-0 flex-1 overflow-y-auto px-3.5 pb-3 pt-1">
          {error ? (
            <div className="rounded-lg bg-danger/[0.08] p-3 text-[11px] leading-relaxed text-danger">
              {error}
            </div>
          ) : !snapshot ? (
            <Skeleton loading={loading} />
          ) : (
            <StatsSection refreshNonce={refreshNonce} />
          )}
        </div>

        {/* 自动升级：静默时只有一行低调的「检查更新」，发现新版才占版面 */}
        <UpdateCard />

        <footer className="flex shrink-0 items-center justify-between px-3.5 pb-2.5 pt-1 text-[10px] text-ink-faint/80">
          <span>上次深探 {formatRelative(snapshot?.deepUpdatedAt ?? null)}</span>
          <span>内存 10 s · 网速 2 s</span>
        </footer>
      </div>
    </div>
  );
}

/**
 * 单页统计（M16 + v6）。六行读数与主窗口统计页同源，只是密度不同。
 * 网络从 64 px 例外行拆成下载 / 上传两条 48 px 标准行（v6），面板高度
 * 随之 400 → 432（算式见 `tray.rs: PANEL_H`）。
 *
 * 端口只做一次性 init（有快照就只读），网速只 init 不提速——面板不做任何
 * 轮询，所有数字都来自已有的常采 / 常驻采样。
 */
function StatsSection({ refreshNonce }: { refreshNonce: number }) {
  const snapshot = useMemStore((s) => s.snapshot);
  const report = useDiskStore((s) => s.report);
  const ports = usePortStore((s) => s.snapshot);
  const initPorts = usePortStore((s) => s.init);
  const netSample = useNetTotalStore((s) => s.sample);
  const netHistory = useNetTotalStore((s) => s.history);
  const initNetTotal = useNetTotalStore((s) => s.init);
  const { volume, known } = useVolumeInfo(refreshNonce);

  // 面板一显示就备好端口与网速：两个 init 都幂等（有快照只读 / 已订阅即返回）
  useEffect(() => {
    void initPorts();
    void initNetTotal();
  }, [initPorts, initNetTotal]);

  const mem = memoryReadout(snapshot);
  const cpu = cpuReadout(snapshot?.cpu);
  const disk = diskReadout(volume, report?.totals.reclaimable ?? null, known);
  const port = portReadout(ports);
  // 方向读数（M16 v6）：网络行拆成下载 / 上传两行，口径在 readout 层派生
  const netDown = netDirectionReadout(netSample, netHistory, "down");
  const netUp = netDirectionReadout(netSample, netHistory, "up");
  const families = snapshot?.families.length ?? 0;

  return (
    <section>
      {/* 泄漏嫌疑只在有命中时露头：没命中就是「沉默即正常」，不占版面 */}
      {snapshot?.leaks ? (
        <div className="mb-1">
          <LeakBanner report={snapshot.leaks} onOpen={() => fireAndForget(openMain(undefined, "memory"), "open_main")} />
        </div>
      ) : null}

      <div className="divide-y divide-hairline-soft">
        <StatsRow title="内存" readout={mem} onOpen={() => fireAndForget(openMain(undefined, "memory"), "open_main")} />
        <StatsRow
          icon={<span className="text-accent">⚡</span>}
          title="存储"
          readout={disk}
          onOpen={() => fireAndForget(openMain(undefined, "disk"), "open_main")}
        />
        <StatsRow title="CPU" readout={cpu} />
        <NetDirectionRow readout={netDown} onOpen={() => fireAndForget(openMain(undefined, "net"), "open_main")} />
        <NetDirectionRow readout={netUp} onOpen={() => fireAndForget(openMain(undefined, "net"), "open_main")} />
        <StatsRow title="端口" readout={port} onOpen={() => fireAndForget(openMain(undefined, "ports"), "open_main")} />
      </div>

      {/* 应用列表退化为一行入口：面板是「瞄一眼」，对比工作台在主窗口 */}
      <button
        type="button"
        onClick={() => fireAndForget(openMain(undefined, "resources"), "open_main")}
        className="group mt-1 flex w-full items-center gap-1 rounded-md px-1 py-1.5 text-left text-[10px] text-ink-faint transition-colors hover:bg-white/[0.04] hover:text-ink-dim"
      >
        <span className="min-w-0 flex-1 truncate">
          查看全部 {families} 个应用（CPU / 磁盘 / 网速对比）
        </span>
        <ChevronRightIcon
          size={11}
          className="shrink-0 transition-transform group-hover:translate-x-0.5"
        />
      </button>
    </section>
  );
}

function Skeleton({ loading }: { loading: boolean }) {
  return (
    <div className="space-y-3">
      <div className="h-3 w-24 animate-pulse rounded bg-white/[0.06]" />
      <div className="h-20 animate-pulse rounded-lg bg-white/[0.04]" />
      <div className="h-3 w-20 animate-pulse rounded bg-white/[0.06]" />
      <div className="h-24 animate-pulse rounded-lg bg-white/[0.04]" />
      <p className="text-[10px] text-ink-faint">{loading ? "正在采样…" : "无数据"}</p>
    </div>
  );
}

function IconButton({
  label,
  onClick,
  children,
}: {
  label: string;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      onClick={onClick}
      className="grid size-6 place-items-center rounded-md text-ink-faint transition-colors hover:bg-white/[0.06] hover:text-ink"
    >
      {children}
    </button>
  );
}
