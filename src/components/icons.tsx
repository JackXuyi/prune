/** 内联图标（Lucide 路径，避免为几个 icon 引入整个图标库） */
import type { ReactNode } from "react";

import type { DeviceKind } from "../types/lan";

interface IconProps {
  size?: number;
  className?: string;
}

function Svg({
  size = 14,
  className = "",
  children,
}: IconProps & { children: ReactNode }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.75}
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
    >
      {children}
    </svg>
  );
}

export function ScissorsIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <circle cx="6" cy="6" r="3" />
      <circle cx="6" cy="18" r="3" />
      <path d="M20 4 8.12 15.88" />
      <path d="M14.47 14.48 20 20" />
      <path d="M8.12 8.12 12 12" />
    </Svg>
  );
}

export function RefreshIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8" />
      <path d="M21 3v5h-5" />
      <path d="M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16" />
      <path d="M3 21v-5h5" />
    </Svg>
  );
}

/** 电池（状态带/设备磁贴）：机身 + 极耳 + 内芯电量块 */
export function BatteryIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <rect x="2" y="8" width="17" height="9" rx="2.5" />
      <path d="M22 11v3" />
      <rect x="4.5" y="10.5" width="10" height="4" rx="1" fill="currentColor" stroke="none" />
    </Svg>
  );
}

/** 统一的下载箭头（无横杠）：更新卡片与下载卡共用；上传走 UploadIcon，不再 rotate-180 */
export function DownloadIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M12 5v14" />
      <path d="m19 12-7 7-7-7" />
    </Svg>
  );
}

/** 统一的上传箭头：与 DownloadIcon 同形镜像（箭头向上），语义成对 */
export function UploadIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M12 19V5" />
      <path d="m5 12 7-7 7 7" />
    </Svg>
  );
}

/** Lucide check：更新弹窗「已是最新」 */
export function CheckIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M20 6 9 17l-5-5" />
    </Svg>
  );
}

/** Lucide x：更新弹窗「关闭 / 失败」 */
export function CloseIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M18 6 6 18" />
      <path d="m6 6 12 12" />
    </Svg>
  );
}

export function ChevronRightIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="m9 18 6-6-6-6" />
    </Svg>
  );
}

export function ExpandIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M15 3h6v6" />
      <path d="M9 21H3v-6" />
      <path d="M21 3 14 10" />
      <path d="M3 21l7-7" />
    </Svg>
  );
}

export function DiskIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M22 12H2" />
      <path d="M5.45 5.11 2 12v6a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-6l-3.45-6.89A2 2 0 0 0 16.76 4H7.24a2 2 0 0 0-1.79 1.11z" />
      <path d="M6 16h.01" />
      <path d="M10 16h.01" />
    </Svg>
  );
}

export function ChipIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <rect width="16" height="16" x="4" y="4" rx="2" />
      <rect width="6" height="6" x="9" y="9" rx="1" />
      <path d="M9 2v2M15 2v2M9 20v2M15 20v2M2 9h2M2 15h2M20 9h2M20 15h2" />
    </Svg>
  );
}

export function FolderIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2z" />
    </Svg>
  );
}

export function FileIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
      <path d="M14 2v6h6" />
    </Svg>
  );
}

/* ---- UI v4 图标 rail 导航（ui-redesign-v4-plan.md §4.1） ---- */

/** 总览：仪表盘四格（大小不一，暗示「汇总」而非「列表」） */
export function OverviewIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <rect x="3" y="3" width="7" height="9" rx="2" />
      <rect x="14" y="3" width="7" height="5" rx="2" />
      <rect x="14" y="12" width="7" height="9" rx="2" />
      <rect x="3" y="16" width="7" height="5" rx="2" />
    </Svg>
  );
}

/** 应用：四宫格 */
export function AppsIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <rect x="3" y="3" width="8" height="8" rx="2.5" />
      <rect x="13" y="3" width="8" height="8" rx="2.5" />
      <rect x="3" y="13" width="8" height="8" rx="2.5" />
      <rect x="13" y="13" width="8" height="8" rx="2.5" />
    </Svg>
  );
}

/** 进程：层叠（家族 = 主进程 + 子进程；列表页头部进程数用） */
export function StackIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M12 3 3 7.5l9 4.5 9-4.5L12 3Z" />
      <path d="m3 12.5 9 4.5 9-4.5" />
      <path d="m3 17 9 4.5 9-4.5" />
    </Svg>
  );
}

/** 连接：链环（ESTABLISHED 连接数用） */
export function LinkIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M10 13a5 5 0 0 0 7.5.5l3-3a5 5 0 0 0-7-7l-1.7 1.7" />
      <path d="M14 11a5 5 0 0 0-7.5-.5l-3 3a5 5 0 0 0 7 7l1.7-1.7" />
    </Svg>
  );
}

/** 搜索 */
export function SearchIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <circle cx="11" cy="11" r="7" />
      <path d="m21 21-4.3-4.3" />
    </Svg>
  );
}

/** 内存：云（冷静监控的隐喻，域色 info 蓝） */
export function MemoryCloudIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M6 19a3 3 0 1 1 0-6 4 4 0 0 1 7.7-1.6A3.5 3.5 0 1 1 18 19Z" />
    </Svg>
  );
}

/** 内存可用：三条渐弱横线（水位线语义，accent 绿） */
export function MemoryFreeIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M3 18h18" />
      <path d="M3 13h18" opacity=".45" />
      <path d="M3 8h18" opacity=".22" />
    </Svg>
  );
}

/** 压缩内存：两条横线 + 对向汇聚箭头（info 蓝） */
export function CompressIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M4 9h16" />
      <path d="M4 15h16" />
      <path d="m9 5 3 4 3-4" />
      <path d="m9 19 3-4 3 4" />
    </Svg>
  );
}

/** Swap 交换：两根带箭头的反向横线（accent 绿，百分比位套环用） */
export function SwapIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M4 8h13l-3-3" />
      <path d="M20 16H7l3 3" />
    </Svg>
  );
}

/** 换页颠簸：上下对向箭头（warn 琥珀） */
export function ThrashIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M7 4v14m0 0-3-3m3 3 3-3" />
      <path d="M17 20V6m0 0-3 3m3-3 3 3" />
    </Svg>
  );
}

/** 泄漏嫌疑：三角警示（warn 琥珀；文案永远「疑似」，不说「泄漏了」） */
export function WarnTriangleIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M12 3 2.5 20h19L12 3Z" />
      <path d="M12 9v4.5" />
      <circle cx="12" cy="17" r="0.7" fill="currentColor" stroke="none" />
    </Svg>
  );
}

/** 网络：地球 */
export function GlobeIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <circle cx="12" cy="12" r="9" />
      <path d="M3 12h18" />
      <path d="M12 3c3 3.5 3 14.5 0 18-3-3.5-3-14.5 0-18Z" />
    </Svg>
  );
}

/** 端口：插孔两端（谁占着 3000） */
export function PortJackIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M4 17v-6a8 8 0 0 1 16 0v6" />
      <path d="M4 17h4v3H4z" />
      <path d="M16 17h4v3h-4z" />
    </Svg>
  );
}

/** Wi-Fi 信号（链路结论 / 网络语义位）：两道弧 + 底点，与 rail 网络图标同形 */
export function WifiIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M5 12a10 10 0 0 1 14 0" />
      <path d="M8.5 15.5a5.5 5.5 0 0 1 7 0" />
      <circle cx="12" cy="19" r="1.4" fill="currentColor" stroke="none" />
    </Svg>
  );
}

/** 活动脉冲（响应度 RPM / 速率类读数的心电曲线） */
export function ActivityIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M3 12h4l3-7 4 14 3-7h4" />
    </Svg>
  );
}

/** 路由器（网关 / LAN 层语义位）：机身 + 两颗指示灯 + 天线口 */
export function RouterIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <rect x="3" y="9" width="18" height="9" rx="2" />
      <circle cx="7" cy="13.5" r="1.2" fill="currentColor" stroke="none" />
      <circle cx="11" cy="13.5" r="1.2" fill="currentColor" stroke="none" />
      <path d="M17 12.5h2" />
      <path d="M17 15h2" />
    </Svg>
  );
}

/** 设置齿轮（原 MainWindow 内联 svg 提为组件，rail 复用） */
export function GearIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <circle cx="12" cy="12" r="3" />
      <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09a1.65 1.65 0 0 0-1-1.51 1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09a1.65 1.65 0 0 0 1.51-1 1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33h.01a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51h.01a1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82v.01a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
    </Svg>
  );
}

/** 设备：显示器 + 机身（本机硬件 + 局域网设备共用一个语义位） */
export function DeviceIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <rect x="3" y="4" width="18" height="12" rx="2" />
      <path d="M12 16v3" />
      <path d="M8 21h8" />
      <path d="M8.5 11.2a5 5 0 0 1 7 0" />
      <circle cx="12" cy="13" r="0.9" fill="currentColor" stroke="none" />
    </Svg>
  );
}

/** 存储：数据库圆柱（比「硬盘盒」更贴近「卷」的语义） */
export function DatabaseIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <ellipse cx="12" cy="5.5" rx="8" ry="2.8" />
      <path d="M4 5.5v13c0 1.5 3.6 2.8 8 2.8s8-1.3 8-2.8v-13" />
      <path d="M4 12c0 1.5 3.6 2.8 8 2.8s8-1.3 8-2.8" />
    </Svg>
  );
}

/* ---- M18-LAN 设备类型图标（LanDeviceCard tile 用，plan §4） ---- */

export function CopyIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <rect x="9" y="9" width="13" height="13" rx="2" />
      <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
    </Svg>
  );
}

export function DeviceKindIcon({
  kind,
  ...props
}: IconProps & { kind: DeviceKind }) {
  switch (kind) {
    case "router":
      return (
        <Svg {...props}>
          <rect x="2" y="14" width="20" height="8" rx="2" />
          <path d="M6.01 18H6" />
          <path d="M10.01 18H10" />
          <path d="M15 14V9" />
          <path d="M18.5 14V9" />
        </Svg>
      );
    case "computer":
      return (
        <Svg {...props}>
          <rect x="3" y="4" width="18" height="12" rx="2" />
          <path d="M2 20h20" />
        </Svg>
      );
    case "phone":
      return (
        <Svg {...props}>
          <rect x="7" y="2" width="10" height="20" rx="2.5" />
          <path d="M11 18h2" />
        </Svg>
      );
    case "tablet":
      return (
        <Svg {...props}>
          <rect x="4" y="2" width="16" height="20" rx="2.5" />
          <path d="M11 18h2" />
        </Svg>
      );
    case "tv":
      return (
        <Svg {...props}>
          <rect x="2" y="4" width="20" height="13" rx="2" />
          <path d="M7 21h10" />
          <path d="m17 2-5 2-5-2" />
        </Svg>
      );
    case "speaker":
      return (
        <Svg {...props}>
          <rect x="4" y="2" width="16" height="20" rx="2" />
          <circle cx="12" cy="14" r="3" />
          <path d="M12 6.5h.01" />
        </Svg>
      );
    case "printer":
      return (
        <Svg {...props}>
          <path d="M6 18H4a2 2 0 0 1-2-2v-5a2 2 0 0 1 2-2h16a2 2 0 0 1 2 2v5a2 2 0 0 1-2 2h-2" />
          <rect x="6" y="14" width="12" height="8" rx="1" />
          <path d="M6 9V3a1 1 0 0 1 1-1h10a1 1 0 0 1 1 1v6" />
        </Svg>
      );
    case "nas":
      return (
        <Svg {...props}>
          <rect x="3" y="4" width="18" height="7" rx="1.5" />
          <rect x="3" y="13" width="18" height="7" rx="1.5" />
          <path d="M7 7.5h.01" />
          <path d="M7 16.5h.01" />
        </Svg>
      );
    case "camera":
      return (
        <Svg {...props}>
          <path d="m22 8-6 4 6 4V8Z" />
          <rect x="2" y="6" width="14" height="12" rx="2" />
        </Svg>
      );
    case "iot":
      return (
        <Svg {...props}>
          <rect width="16" height="16" x="4" y="4" rx="2" />
          <rect width="6" height="6" x="9" y="9" rx="1" />
          <path d="M9 2v2M15 2v2M9 20v2M15 20v2M2 9h2M2 15h2M20 9h2M20 15h2" />
        </Svg>
      );
    default:
      return (
        <Svg {...props}>
          <circle cx="12" cy="12" r="10" />
          <path d="M9.09 9a3 3 0 0 1 5.83 1c0 2-3 3-3 3" />
          <path d="M12 17h.01" />
        </Svg>
      );
  }
}

/** 耳机（外设页蓝牙音频设备行） */
export function HeadphonesIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M3 14h3a2 2 0 0 1 2 2v3a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-7a9 9 0 0 1 18 0v7a2 2 0 0 1-2 2h-1a2 2 0 0 1-2-2v-3a2 2 0 0 1 2-2h3" />
    </Svg>
  );
}

/** 插头（外设页 USB 区 / rail「外设」导航项） */
export function UsbIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M12 22v-5" />
      <path d="M9 8V2" />
      <path d="M15 8V2" />
      <path d="M18 8v5a4 4 0 0 1-4 4h-4a4 4 0 0 1-4-4V8Z" />
    </Svg>
  );
}
