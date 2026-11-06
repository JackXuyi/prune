import {
  cpuReadout,
  diskReadout,
  memoryReadout,
  netReadout,
  portReadout,
} from "../lib/statsReadout";
import { useVolumeInfo } from "../lib/useVolumeInfo";
import { useDiskStore } from "../stores/diskStore";
import { useMemStore } from "../stores/memStore";
import { useNetTotalStore } from "../stores/netTotalStore";
import { usePortStore } from "../stores/portStore";
import { NetReading } from "./stats/NetReading";
import { StatsCard } from "./stats/StatsCard";

/** 下钻目标：统计卡只导航，不做任何清理/结束动作（M9 §4） */
export type DrillTab = "overview" | "disk" | "ports" | "net";

/**
 * 统计 tab（M9 + M16）：系统级总览，作为默认页。
 *
 * 五个域各一张卡：内存 / CPU / 硬盘 / 端口 / 网络。读数一律来自
 * `lib/statsReadout.ts` —— 菜单栏面板的紧凑版用的是同一批函数，所以两边的
 * 数字、口径、空值文案天然一致，不存在「两份实现各算一遍」的分叉。
 *
 * 布局：2×2 四卡 + 第 3 行网络卡横跨两列。卡高 `min-h-[140px]` 是算出来的：
 * 三行卡 + 标题块 + 页脚 = 574 px，刚好放进 592 px 内容区（方案 §5.1）。
 *
 * 假事实红线（方案 §8）：没扫描/读不到一律「尚未扫描 / 不可读」，绝不填 0。
 */
export function StatsBoard({
  onDrill,
  refreshNonce,
}: {
  onDrill: (tab: DrillTab) => void;
  /** 头部「刷新」按钮的递增信号：变了就重取卷容量（内存/端口由各自 store 刷新） */
  refreshNonce: number;
}) {
  const snapshot = useMemStore((s) => s.snapshot);
  const ports = usePortStore((s) => s.snapshot);
  const report = useDiskStore((s) => s.report);
  const netSample = useNetTotalStore((s) => s.sample);
  const netHistory = useNetTotalStore((s) => s.history);
  // 卷容量：进入 tab、点刷新时各取一次（statvfs 微秒级，无需缓存）
  const { volume, known } = useVolumeInfo(refreshNonce);

  const mem = memoryReadout(snapshot);
  const cpu = cpuReadout(snapshot?.cpu);
  const disk = diskReadout(volume, report?.totals.reclaimable ?? null, known);
  const port = portReadout(ports);
  const net = netReadout(netSample, netHistory);

  return (
    <div className="min-h-0 flex-1 overflow-y-auto px-5 py-4">
      <div className="flex items-baseline justify-between">
        <div>
          <h1 className="text-[15px] font-semibold tracking-tight">本机资源总览</h1>
          <p className="mt-0.5 text-[11px] text-ink-faint">
            系统级读数 · 点击卡片看明细 · 只读，不在这里做任何清理
          </p>
        </div>
        <span className="text-[10px] text-ink-faint">内存 / CPU 每 10 s · 网速每 2 s</span>
      </div>

      <div className="mt-4 grid grid-cols-2 gap-3">
        <StatsCard
          title="内存"
          caption="physicalUsed / 总量 · 含被压缩与换出的部分"
          readout={mem}
          onOpen={() => onDrill("overview")}
        />
        <StatsCard
          title="CPU"
          caption="整机每核平均（0–100%）· 与进程的单核口径不同，不能相减"
          readout={cpu}
        />
        <StatsCard
          title="硬盘"
          caption="home 所在卷 · 已用 = 总量 − 用户可用（statvfs 粗略口径）"
          readout={disk}
          onOpen={() => onDrill("disk")}
        />
        <StatsCard
          title="端口"
          caption="去重后的监听端口 · 关注层口径见端口 tab"
          readout={port}
          onOpen={() => onDrill("ports")}
        />
        <div className="col-span-2">
          <StatsCard
            title="网络"
            caption="物理网卡合计 · 虚拟接口单列不计入，VPN 不会双计"
            readout={net}
            body={<NetReading readout={net} />}
            onOpen={() => onDrill("net")}
          />
        </div>
      </div>

      <p className="mt-4 text-[10px] leading-relaxed text-ink-faint">
        统计只呈现事实读数与口径。清理入口在「存储」tab（走白名单与确认），
        结束进程在进程明细上——统计页不放破坏性按钮。
      </p>
    </div>
  );
}
