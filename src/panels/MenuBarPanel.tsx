import { useEffect, useState, type ReactNode } from "react";
import { LeakBanner } from "../components/LeakCard";
import { NetDirectionRow, StatsRow } from "../components/stats/StatsRow";
import { formatBytes, formatRelative } from "../lib/format";
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
  const scanning = useDiskStore((s) => s.scanning);
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

  /**
   * 主 CTA 的文案与强调度（M25）。
   *
   * 产品主张是「可回收 GB」（README 首屏主数字），但此前面板只把它当存储行的
   * 副读数——用户看得到数字、找不到动作。这里把读数升成按钮，只给一个去处：
   * 存储 tab 的清理流（dry-run → 确认 → 废纸篓）。
   *
   * 三个阶段都受「不造假事实」红线约束：没扫过就是「尚未深探」，扫出 0 就是
   * 「暂无可回收」，绝不把 null 渲染成 0 GB。
   */
  const reclaim = report?.totals.reclaimable ?? null;
  const hasReclaim = reclaim != null && reclaim > 0;
  const cta = scanning
    ? { label: "正在深探…", action: "看进度" }
    : reclaim == null
      ? { label: "尚未深探", action: "去扫描" }
      : !hasReclaim
        ? { label: "暂无可回收", action: "看明细" }
        : { label: `可回收 ${formatBytes(reclaim)}`, action: "去清理" };

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

      {/* 底部一条：主 CTA（可回收 → 去清理）+ 应用入口。
          两个目标分开成两个按钮，不做嵌套；高度沿用原入口的 30 px 预算，
          面板 432 的贴合高度不变（算式见 tray.rs: PANEL_H）。 */}
      <div className="mt-0.5 flex items-center gap-1">
        <button
          type="button"
          onClick={() => fireAndForget(openMain(undefined, "disk"), "open_main")}
          title={`${cta.label} · 打开存储页`}
          className={`group flex h-7 min-w-0 flex-1 items-center gap-1 rounded-lg border pl-2 pr-1.5 text-left transition-colors ${
            hasReclaim
              ? "border-accent/25 bg-accent/[0.10] hover:border-accent/45 hover:bg-accent/[0.16]"
              : "border-white/[0.07] bg-white/[0.03] hover:bg-white/[0.06]"
          }`}
        >
          <span
            className={`shrink-0 text-[11px] leading-none ${
              hasReclaim ? "text-accent" : "text-ink-faint"
            }`}
          >
            ⚡
          </span>
          <span
            className={`min-w-0 truncate text-[11px] leading-none font-semibold ${
              hasReclaim ? "text-accent" : "text-ink-dim"
            }`}
          >
            {cta.label}
          </span>
          <span className="flex-1" />
          <span
            className={`shrink-0 text-[10px] leading-none ${
              hasReclaim
                ? "text-accent/75 group-hover:text-accent"
                : "text-ink-faint group-hover:text-ink-dim"
            }`}
          >
            {cta.action}
          </span>
          <ChevronRightIcon
            size={11}
            className={`shrink-0 transition-transform group-hover:translate-x-0.5 ${
              hasReclaim ? "text-accent/75" : "text-ink-faint"
            }`}
          />
        </button>

        {/* 应用对比是主窗口的活，这里只留一个不抢焦点的入口 */}
        <button
          type="button"
          onClick={() => fireAndForget(openMain(undefined, "resources"), "open_main")}
          title="应用资源：按应用看进程 / CPU / 内存 / 磁盘 / 端口 / 网速"
          className="group flex h-7 shrink-0 items-center gap-0.5 rounded-lg px-1.5 text-[10px] text-ink-faint transition-colors hover:bg-white/[0.06] hover:text-ink-dim"
        >
          <span className="leading-none">应用 {families}</span>
          <ChevronRightIcon
            size={10}
            className="shrink-0 transition-transform group-hover:translate-x-0.5"
          />
        </button>
      </div>
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
