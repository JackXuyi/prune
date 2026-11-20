import { useEffect, useRef, useState, type ReactNode } from "react";
import { LeakBanner } from "../components/LeakCard";
import { NetDirectionRow } from "../components/stats/StatsRow";
import { PanelPortBand, PanelRings } from "../components/panel/PanelHero";
import { formatBytes, formatRelative } from "../lib/format";
import { useI18n } from "../i18n/store";
import { fireAndForget, openMain, panelFitHeight } from "../lib/ipc";
import {
  cpuReadout,
  diskReadout,
  memoryReadout,
  netDirectionReadout,
} from "../lib/statsReadout";
import { useVolumeInfo } from "../lib/useVolumeInfo";
import { useDiskStore } from "../stores/diskStore";
import { useMemStore } from "../stores/memStore";
import { useNetTotalStore } from "../stores/netTotalStore";
import { usePortStore } from "../stores/portStore";
import { portConflicts } from "../types/net";
import type { PortSnapshot } from "../types/net";
import { ChevronRightIcon, ExpandIcon, GearIcon, RefreshIcon, ScissorsIcon } from "../components/icons";
import { Badge } from "../components/primitives";
import { UpdateCard } from "../components/UpdateCard";

/**
 * 菜单栏面板（宽 380，高随内容）。
 *
 * 高度贴合内容：W2/D-07 加「本地服务」段后内容高不再是常量（该段按
 * 冲突/关注存在与否渲染，更新卡也按需占版面），所以改为前端量取内容
 * 自然高、经 `panel_fit_height` 上报缩放（popover contentSize / 降级窗口
 * set_size）。内容区不滚动——菜单栏「瞄一眼」场景里滚动条是噪音，
 * 空间不够就让窗口长高。表面容器按内容自然高测量，不加 padding、不加
 * 余量：卡片与 popover 外框贴合（见 `tray.rs: PANEL_H` 注释）。
 *
 * 原来的「应用」「概况」两个 tab 都已撤掉——应用级明细在主窗口「应用资源」，
 * 内存压力 / swap / 换出受害者在主窗口「内存」。面板是「瞄一眼」场景。
 *
 * 读数与主窗口统计 tab 共用 `lib/statsReadout`，面板只换排版密度（48 px 行），
 * 不重算口径。
 */
export function MenuBarPanel() {
  const { t, locale } = useI18n();
  const [refreshNonce, setRefreshNonce] = useState(0);
  /** 面板表面容器：内容自然高的测量源（offsetHeight 直接 = 上报值） */
  const surfaceRef = useRef<HTMLDivElement | null>(null);
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

  /**
   * 贴高上报：表面容器是「自然高」（不撑满、不裁切），内容任何变化
   * （骨架 → 读数、本地服务段出现/消失、更新卡占版面、语言切换）都会
   * 改变 offsetHeight，ResizeObserver 每次变化上报一次。上报值 = 表面高，
   * 外层不再有 p-2 padding —— 卡片与 popover 外框贴合，测量也不再加 16。
   * 关键：容器不能用 h-screen 固定高，否则 flex-shrink 会把表面压到视口高
   * 并裁掉底部（历史遮挡根因），测量值反而永远追不上内容自然高。
   * 钳位与抖动抑制在 Rust 侧再做，前端只保证不重复发同一个值。
   */
  useEffect(() => {
    const el = surfaceRef.current;
    if (!el) return;
    let last = -1;
    const report = (force = false) => {
      const h = el.offsetHeight;
      if (!force && h === last) return;
      last = h;
      void fireAndForget(panelFitHeight(h), "panel_fit_height");
    };
    const ro = new ResizeObserver(() => report());
    ro.observe(el);
    // 视口变化兜底：popover init 会把 webview 重新嵌框（视口尺寸跳变），
    // 但表面「自然高」没变——RO 不触发，此前上报过的值必须强制重发一次，
    // 否则面板停在初始兜底高，内容高超出部分被裁（真机遮挡的根因之一）。
    let raf = 0;
    const onViewportResize = () => {
      cancelAnimationFrame(raf);
      raf = requestAnimationFrame(() => report(true));
    };
    window.addEventListener("resize", onViewportResize);
    return () => {
      ro.disconnect();
      window.removeEventListener("resize", onViewportResize);
      cancelAnimationFrame(raf);
    };
  }, []);

  return (
    <div>
      <div
        ref={surfaceRef}
        className="flex shrink-0 flex-col overflow-hidden rounded-[14px] border border-hairline bg-surface-raised/95 shadow-[0_18px_50px_-12px_rgba(0,0,0,0.8)] backdrop-blur-2xl"
      >
        <header className="flex h-11 shrink-0 items-center gap-2 px-3.5">
          <ScissorsIcon size={14} className="text-accent" />
          <span className="text-[13px] font-semibold tracking-tight text-ink">Prune</span>
          {snapshot ? (
            snapshot.deepAvailable ? (
              <Badge tone="accent" title={t("panel.badge.footprintTitle")}>
                footprint
              </Badge>
            ) : (
              <Badge tone="warn" title={t("panel.badge.rssTitle")}>
                {t("panel.badge.rss")}
              </Badge>
            )
          ) : null}
          <div className="flex-1" />
          <IconButton label={t("panel.action.refresh")} onClick={onRefresh}>
            <RefreshIcon size={13} className={refreshing ? "animate-spin" : ""} />
          </IconButton>
          <IconButton
            label={t("panel.action.openMain")}
            onClick={() => fireAndForget(openMain(), "open_main")}
          >
            <ExpandIcon size={13} />
          </IconButton>
          {/* 只开面板的用户原先到不了设置（齿轮只长在主窗口 rail 上）。
              这里是「到设置」的跳板，不是面板内切语言 —— 语言切换仍在设置页一处。 */}
          <IconButton
            label={t("panel.action.settings")}
            onClick={() => fireAndForget(openMain(undefined, undefined, true), "open_settings")}
          >
            <GearIcon size={13} />
          </IconButton>
        </header>

        {/* pt-1：tab 行删除后补一点呼吸，否则首个读数行紧贴 header。
            不滚动不撑满：内容自然高经 surfaceRef 上报贴高（见上方 effect） */}
        <div className="px-3.5 pb-3 pt-1">
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
          <span>
            {t("panel.footer.lastDeep", {
              when: formatRelative(snapshot?.deepUpdatedAt ?? null, locale),
            })}
          </span>
          <span>{t("panel.footer.cadence")}</span>
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
  const { t } = useI18n();
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
    ? { label: t("panel.cta.scanning"), action: t("panel.cta.viewProgress") }
    : reclaim == null
      ? { label: t("panel.cta.notScanned"), action: t("panel.cta.goScan") }
      : !hasReclaim
        ? { label: t("panel.cta.nothing"), action: t("panel.cta.details") }
        : {
            label: t("panel.cta.reclaim", { size: formatBytes(reclaim) }),
            action: t("panel.cta.goClean"),
          };

  return (
    <section>
      {/* spec §5.1 序：CTA 紧跟头部（序 2）。可回收是面板第一诉求，
          之前排在六行统计之后，「瞄一眼」场景里根本滚不到。高度 30 px 预算不变，
          面板 432 贴合高度不变（算式见 tray.rs: PANEL_H）。 */}
      <div className="mb-1 flex items-center gap-1">
        <button
          type="button"
          onClick={() => fireAndForget(openMain(undefined, "disk"), "open_main")}
          title={t("panel.cta.title", { label: cta.label })}
          className={`group flex h-7 min-w-0 flex-1 items-center gap-1 rounded-lg border pl-2 pr-1.5 text-left transition-colors ${
            hasReclaim
              ? "border-accent/25 bg-accent/[0.10] hover:border-accent/45 hover:bg-accent/[0.16]"
              : "border-hairline-soft bg-chip hover:bg-chip-hover"
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
          title={t("panel.apps.title")}
          className="group flex h-7 shrink-0 items-center gap-0.5 rounded-lg px-1.5 text-[10px] text-ink-faint transition-colors hover:bg-chip hover:text-ink-dim"
        >
          <span className="leading-none">{t("panel.apps.label", { n: families })}</span>
          <ChevronRightIcon
            size={10}
            className="shrink-0 transition-transform group-hover:translate-x-0.5"
          />
        </button>
      </div>
      {/* 泄漏嫌疑只在有命中时露头：没命中就是「沉默即正常」，不占版面 */}
      {snapshot?.leaks ? (
        <div className="mb-1">
          <LeakBanner report={snapshot.leaks} onOpen={() => fireAndForget(openMain(undefined, "memory"), "open_main")} />
        </div>
      ) : null}

      {/* 首屏读数区（menubar-panel-eul-mockup W2）：三环 Hero 取代原来
          内存/存储/CPU 三行 40 px 文字行——三个都是百分比，套环合法且省 24 px；
          环色吃 loadTone（同源，不另造配色） */}
      <PanelRings
        memory={mem}
        disk={disk}
        cpu={cpu}
        onMemory={() => fireAndForget(openMain(undefined, "memory"), "open_main")}
        onDisk={() => fireAndForget(openMain(undefined, "disk"), "open_main")}
      />

      <div className="divide-y divide-hairline-soft">
        <NetDirectionRow readout={netDown} onOpen={() => fireAndForget(openMain(undefined, "net"), "open_main")} />
        <NetDirectionRow readout={netUp} onOpen={() => fireAndForget(openMain(undefined, "net"), "open_main")} />
      </div>

      {/* 端口带：域色三段（监听/关注/暴露）。端口不是百分比所以不套环；
          暴露 0 该段不渲染，未扫描整块退化成一行（不画 0 个芯片） */}
      <PanelPortBand
        ports={ports}
        onOpen={() => fireAndForget(openMain(undefined, "ports"), "open_main")}
      />

      {/* W2/D-07 本地服务段（§4.2 ②）：只在存在冲突或关注端口被占时出现——
          菜单栏是稀缺空间，常驻空栏会被读成噪音。读数与主窗口同一份快照派生 */}
      <LocalServiceRows ports={ports} />

    </section>
  );
}

/**
 * W2/D-07 菜单栏「本地服务」三条（§4.2 ②）：冲突行置顶，其余取关注层前几行。
 * 点任意行 → open_main 带 focus 深链，主窗口端口 tab 直接定位到该行。
 */
function LocalServiceRows({ ports }: { ports: PortSnapshot | null }) {
  const { t } = useI18n();
  if (!ports || ports.coverage.note) return null; // 采集失败不占版面（口径说明在端口页）

  const conflicts = portConflicts(ports.entries);
  const watched = ports.entries.filter((e) => e.watched);
  if (conflicts.length === 0 && watched.length === 0) return null;

  const conflictPorts = new Set(conflicts.map((c) => c.port));
  const ordered = [...watched].sort((a, b) => {
    const ca = conflictPorts.has(a.port) ? 0 : 1;
    const cb = conflictPorts.has(b.port) ? 0 : 1;
    return ca - cb || a.port - b.port;
  });
  const shown = ordered.slice(0, 3);
  const open = (port: number) =>
    fireAndForget(openMain(undefined, "ports", undefined, String(port)), "open_main");

  return (
    <div className="mt-1 rounded-lg border border-hairline-soft bg-chip/60 px-2 py-1.5">
      <div className="flex items-baseline justify-between px-1">
        <span className="text-[10px] font-medium tracking-wide text-ink-dim">
          {t("panel.ports.title")}
        </span>
        {conflicts.length > 0 ? (
          <span className="rounded-full bg-danger/12 px-1.5 py-px text-[9px] font-medium text-danger">
            {t("panel.ports.conflictBadge")} {conflicts.length}
          </span>
        ) : null}
      </div>
      <ul className="mt-0.5">
        {shown.map((e) => {
          const isConflict = conflictPorts.has(e.port);
          const owners = new Set(e.listeners.map((b) => b.pid).filter((p) => p != null)).size;
          const comm = e.listeners.find((b) => b.pid != null)?.comm ?? "—";
          return (
            <li key={`${e.port}-${e.proto}`}>
              <button
                type="button"
                onClick={() => open(e.port)}
                className="flex w-full items-center gap-1.5 rounded-md px-1 py-1 text-left transition-colors hover:bg-chip-hover"
              >
                <span
                  className={`shrink-0 font-mono text-[11px] tabular-nums ${
                    isConflict ? "text-danger" : "text-ink"
                  }`}
                >
                  :{e.port}
                </span>
                <span className="min-w-0 flex-1 truncate text-[10px] text-ink-dim">
                  {/* 端口号已在左侧 .pt 渲染，行文案只补 comm / 冲突倍数，不重复端口 */}
                  {isConflict
                    ? t("panel.ports.conflict", { comm, n: owners })
                    : comm}
                </span>
                {e.listeners[0]?.projectLabel ? (
                  <span className="max-w-[110px] shrink-0 truncate text-[9.5px] text-ink-faint">
                    {e.listeners[0].projectLabel}
                  </span>
                ) : null}
              </button>
            </li>
          );
        })}
        {ordered.length > shown.length ? (
          <li className="px-1 pt-0.5 text-[9.5px] text-ink-faint">
            {t("panel.ports.more", { n: ordered.length - shown.length })}
          </li>
        ) : null}
      </ul>
    </div>
  );
}

function Skeleton({ loading }: { loading: boolean }) {
  const { t } = useI18n();
  return (
    <div className="space-y-3">
      <div className="h-3 w-24 animate-pulse rounded bg-chip-hover" />
      <div className="h-20 animate-pulse rounded-lg bg-chip" />
      <div className="h-3 w-20 animate-pulse rounded bg-chip-hover" />
      <div className="h-24 animate-pulse rounded-lg bg-chip" />
      <p className="text-[10px] text-ink-faint">
        {loading ? t("panel.skeleton.sampling") : t("panel.skeleton.none")}
      </p>
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
      className="grid size-6 place-items-center rounded-md text-ink-faint transition-colors hover:bg-chip hover:text-ink"
    >
      {children}
    </button>
  );
}
