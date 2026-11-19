import { useI18n } from "../../i18n/store";
import { formatBytes } from "../../lib/format";
import { formatSpeed, type NetSpeedSample } from "../../types/net";
import type { SortKey } from "../../types/appsort";
import {
  AppsIcon,
  DownloadIcon,
  LinkIcon,
  MemoryCloudIcon,
  PortJackIcon,
  StackIcon,
  UploadIcon,
} from "../icons";
import { RibbonChip, RibbonSeg, RibbonShell, RibbonValue } from "../Ribbon";

/**
 * 应用列表页摘要带（与首页 StatusRibbon 同形同源）：
 * 应用数 / 进程 / 内存合计 / 下载 / 上传 / 端口，全部「图标 + mono 读数」，
 * 零文字标题——口径进 tooltip，点段直接切到对应排序（排序即透镜）。
 *
 * 诚实红线：
 * - 网速未采样 / 采样中 / 不可读一律 `—`，绝不显示 0；
 * - 端口未扫描 `—`（≠ 没有端口）；有对外暴露时数值转警示色 + tooltip 明文；
 * - 内存合计是各应用 footprint 求和，共享内存会重复计入——tooltip 里写明。
 */
export function AppSummaryRibbon({
  apps,
  procs,
  memBytes,
  net,
  ports,
  exposed,
  conns,
  searching,
  onSort,
}: {
  apps: number;
  procs: number;
  /** 各应用 footprint 求和，仅作量级参考（共享页重复计入） */
  memBytes: number;
  net: NetSpeedSample | null;
  /** 关注层端口合计；null = 端口快照未就绪 */
  ports: number | null;
  /** 对外暴露端口合计（>0 时数值转警示色） */
  exposed: number;
  /** ESTABLISHED 连接合计；null = 端口未扫描 */
  conns: number | null;
  /** 搜索中时计数按筛选结果算，tooltip 里说明 */
  searching: boolean;
  onSort: (key: SortKey) => void;
}) {
  const { t } = useI18n();

  const netOk = net?.status === "ok";
  const down = netOk ? formatSpeed(net.totalDownBps) : "—";
  const up = netOk ? formatSpeed(net.totalUpBps) : "—";
  const netWhy =
    net == null
      ? t("resources.netNotSampled")
      : net.status === "warmup"
        ? t("resources.netSampling")
        : net.status === "unavailable"
          ? t("resources.netUnreadable")
          : null;

  const lens = "var(--color-domain-cpu)"; // 应用域的域色（与 rail 的「应用」项一致）

  return (
    <RibbonShell ariaLabel={t("resources.ribbonAria")}>
      {/* 应用数：页级计数，不切排序 */}
      <RibbonSeg title={searching ? t("resources.ribbon.appsFiltered", { n: apps }) : t("resources.ribbon.apps", { n: apps })}>
        <RibbonChip color={lens} icon={<AppsIcon size={13} />} glow />
        <RibbonValue value={String(apps)} />
      </RibbonSeg>

      {/* 进程 → 按进程数排序 */}
      <RibbonSeg title={t("resources.ribbon.procs", { n: procs })} onClick={() => onSort("procs")}>
        <RibbonChip color="var(--color-ink-dim)" icon={<StackIcon size={13} />} />
        <RibbonValue value={String(procs)} />
      </RibbonSeg>

      {/* 内存合计 → 按内存排序 */}
      <RibbonSeg title={t("resources.ribbon.mem", { size: formatBytes(memBytes) })} onClick={() => onSort("mem")}>
        <RibbonChip color="var(--color-info)" icon={<MemoryCloudIcon size={13} />} />
        <RibbonValue value={formatBytes(memBytes)} />
      </RibbonSeg>

      {/* 下载 / 上传：全机合计（含非应用流量），各自切到下载排序 */}
      <RibbonSeg
        title={netWhy ? `${netWhy}` : t("resources.ribbon.down", { speed: down })}
        onClick={() => onSort("down")}
      >
        <RibbonChip color="var(--color-domain-net)" icon={<DownloadIcon size={13} />} />
        <RibbonValue value={down} />
      </RibbonSeg>
      <RibbonSeg
        title={netWhy ? `${netWhy}` : t("resources.ribbon.up", { speed: up })}
        onClick={() => onSort("down")}
      >
        <RibbonChip color="var(--color-domain-net)" icon={<UploadIcon size={13} />} />
        <RibbonValue value={up} />
      </RibbonSeg>

      {/* 端口 → 按端口数排序；对外暴露时警示色 */}
      <RibbonSeg
        title={ports == null ? t("resources.portsUnscannedTip") : t("resources.ribbon.ports", { n: ports, exposed })}
        onClick={() => onSort("ports")}
      >
        <RibbonChip color="var(--color-domain-port)" icon={<PortJackIcon size={13} />} />
        <RibbonValue value={ports == null ? "—" : String(ports)} tone={ports != null && exposed > 0 ? "warn" : undefined} />
      </RibbonSeg>

      {/* 连接：ESTABLISHED 合计（端口未扫描时 —） */}
      <RibbonSeg
        title={conns == null ? t("resources.connsUnscannedTip") : t("resources.ribbon.conns", { n: conns })}
        onClick={() => onSort("ports")}
      >
        <RibbonChip color="var(--color-domain-net)" icon={<LinkIcon size={13} />} />
        <RibbonValue value={conns == null ? "—" : String(conns)} />
      </RibbonSeg>
    </RibbonShell>
  );
}
