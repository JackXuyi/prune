/** 与 prune-core::net::lan 一一对应（camelCase）。改这里必须同步 Rust 侧。 */

export type NeighborState =
  | "reachable"
  | "stale"
  | "delay"
  | "probe"
  | "incomplete"
  | "permanent"
  | "unknown";

export type DeviceKind =
  | "router"
  | "computer"
  | "phone"
  | "tablet"
  | "tv"
  | "speaker"
  | "printer"
  | "nas"
  | "camera"
  | "iot"
  | "unknown";

export interface LanDevice {
  ip: string;
  /** `null` = 查不到（显示 `—`，不是「未知 MAC」） */
  mac: string | null;
  /** `null` = 查不到（含随机化 MAC——后者 UI 整列省略，不是「未知厂商」） */
  vendor: string | null;
  hostname: string | null;
  kind: DeviceKind;
  state: NeighborState;
  online: boolean;
  /** mDNS 服务类型 + SSDP 归一化（`ssdp:<ST>`），去重升序 */
  services: string[];
  /** 对端连我的本机端口（ESTABLISHED 且对端在网段内） */
  inbound: number[];
  isGateway: boolean;
  isSelf: boolean;
  /** 随机化 MAC：本机实测约一半设备都是（spec §9），UI 加 `· 随机` 小标 */
  randomizedMac: boolean;
}

export interface LanSnapshot {
  at: number;
  iface: string | null;
  cidr: string | null;
  /** 本轮实际探测的地址数（默认快照为 0 = 只读邻居表） */
  scanned: number;
  /** 网段主机总数（截断时不缩水，UI 显示 N/M） */
  total: number;
  devices: LanDevice[];
  /** 我对同网段开放的 TCP 端口（BindScope::Any 口径，升序） */
  exposedPorts: number[];
  /** 异常与口径解释；空列表 = 一切正常 */
  notes: string[];
  scanMs: number;
}

/** `lan://progress` 事件 payload（spec §10） */
export interface LanProgress {
  phase: "neighbors" | "probe" | "discover" | "merge";
  done: number;
  total: number;
}

export const LAN_PHASE_LABEL: Record<LanProgress["phase"], string> = {
  discover: "正在发现服务（mDNS / SSDP）",
  probe: "正在探测网段",
  neighbors: "正在读取邻居表",
  merge: "正在合成结果",
};

export function kindLabel(kind: DeviceKind): string {
  const map: Record<DeviceKind, string> = {
    router: "路由器",
    computer: "电脑",
    phone: "手机",
    tablet: "平板",
    tv: "电视",
    speaker: "音箱",
    printer: "打印机",
    nas: "NAS",
    camera: "摄像头",
    iot: "IoT",
    unknown: "未知",
  };
  return map[kind];
}

/** 设备行的单字图标（列表行小图，不需要完整图标集） */
export function kindGlyph(kind: DeviceKind): string {
  const map: Record<DeviceKind, string> = {
    router: "⌂",
    computer: "▣",
    phone: "▯",
    tablet: "▭",
    tv: "▶",
    speaker: "◉",
    printer: "⎙",
    nas: "⛁",
    camera: "◎",
    iot: "✦",
    unknown: "·",
  };
  return map[kind];
}
