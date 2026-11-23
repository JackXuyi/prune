import { useEffect } from "react";

import { useHardwareReadouts } from "./stats/useHardwareReadouts";
import { DeviceRibbon, lowestPowerSource } from "./device/DeviceRibbon";
import { DeviceTiles } from "./device/DeviceTiles";
import { useDiskTelemetry } from "./device/useDiskTelemetry";
import { LanBoard } from "./LanBoard";
import { LanHero } from "./lan/LanHero";
import { useLanStore } from "../stores/lanStore";

/**
 * 设备页（M-设备栏）eul 式重设计（device-eul-mockup.html）：
 * 一页两域（本机硬件 + 局域网），首屏读数区 + 变长设备区走整页滚动。
 *
 * 结构：
 * - ① DeviceRibbon 7 段整页摘要（本机硬件 5 + 局域网 2）；
 * - ② LanHero 设备点阵（tone=device 域色）——未扫描（snapshot 为 null）时不渲染，
 *   LanBoard 内的空态插画 + 扫描按钮接管，绝不渲染成「0 台设备」；
 * - ③ DeviceTiles 磁贴 ×2（电源配件 / 磁盘 I/O·体检）；
 * - ④ LanBoard（showHeader=false：状态带与点阵由页面级承担，不重复画；
 *   showTopoToggle=false：拓扑入口隐藏——2026-11-20 产品决策，2026-11-23 M22
 *   确认延续：端口页局域网子页已下线，拓扑代码休眠保留、无 UI 入口，
 *   详见 docs/m22-port-lan-dedup-plan.md D1）。
 *
 * 采样纪律不变：useHardwareReadouts / useDiskTelemetry 挂载时开、卸载时停；
 * 进入页面绝不自动扫描——权限弹窗只由用户点「扫描局域网」触发（诚实红线）。
 * 页面无全局刷新按钮：硬件低频自轮询、局域网有显式扫描按钮，一个
 * 「看起来在刷但什么都没刷」的按钮比没有更糟。
 */
export function DeviceBoard() {
  const snapshot = useLanStore((s) => s.snapshot);
  const scanning = useLanStore((s) => s.scanning);
  const scan = useLanStore((s) => s.scan);
  const init = useLanStore((s) => s.init);

  const { battery, accessories, temp } = useHardwareReadouts();
  const disk = useDiskTelemetry();

  // LanBoard 内部也会 init（幂等）：设备页要先拿到 snapshot 才能渲染点阵 Hero
  useEffect(() => {
    void init();
  }, [init]);

  const lowest = lowestPowerSource(battery, accessories);

  return (
    <div className="min-h-0 flex-1 overflow-x-hidden overflow-y-auto px-5 pt-3 pb-3">
      <div className="flex flex-col gap-[9px]">
        <DeviceRibbon temp={temp} lowest={lowest} disk={disk} snapshot={snapshot} />
        {snapshot ? (
          <LanHero snapshot={snapshot} scanning={scanning} onScan={() => void scan()} tone="device" />
        ) : null}
        <DeviceTiles battery={battery} accessories={accessories} disk={disk} />
        <LanBoard showHeader={false} showTopoToggle={false} />
      </div>
    </div>
  );
}
