import { HardwareStrip } from "./stats/HardwareStrip";
import { DiskHardwareStrip } from "./DiskHardwareStrip";
import { LanBoard } from "./LanBoard";

/**
 * 设备页（M-设备栏）：本机硬件 + 局域网设备聚合一页。
 *
 * - 上半「本机」：硬件读数条（电池 / 蓝牙配件 / CPU 温度）+ 磁盘 I/O / SMART。
 *   两块 strip 各自带可见性门控与失败即隐藏纪律（f26），这里只做排版拼装，
 *   不重复采样逻辑——谁可见谁负责的边界留在组件内部。
 * - 下半「局域网」：LanBoard 原样复用（端口页仍保留同一块，store 共享，
 *   两处可见性互不干扰）。铁律不变：进入页面不扫描，权限弹窗只由
 *   用户点「扫描局域网」触发。
 *
 * 页面无全局刷新按钮：硬件是低频自轮询、局域网有显式扫描按钮，
 * 一个「看起来在刷但什么都没刷」的按钮比没有更糟（诚实红线）。
 */
export function DeviceBoard() {
  return (
    <div className="min-h-0 flex-1 overflow-y-auto px-5 pt-3 pb-3">
      <div className="space-y-2.5">
        <HardwareStrip />
        <DiskHardwareStrip />
      </div>

      {/* 与硬件块之间留一档呼吸位：两段语义不同（本机 vs 局域网），
          用间距分组而不是加分割线（格式塔·亲密性） */}
      <div className="mt-5">
        <LanBoard />
      </div>
    </div>
  );
}
