/** 与 prune-core::net::lan 一一对应（camelCase）。改这里必须同步 Rust 侧。 */
import { i18nFor } from "../i18n";
import { currentLocale } from "../i18n/store";
import type { Locale, NoParamKey } from "../i18n/types";

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
  /** 首次发现时间（Unix 秒；来自跨轮次历史层，旧版缓存 = 0 显示 `—`）。M19 spec §3.1。 */
  firstSeen: number;
  /** 最后活跃时间（本轮仍在网 → = 本轮 at；离线遗留 = 上次在线轮次；旧缓存 = 0） */
  lastSeen: number;
  /** 新设备标识（Rust 侧算好：`snapshot.at - firstSeen <= 600s` 且非首扫基线） */
  isNew: boolean;
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

/**
 * 阶段文案的 key（渲染时才 `t()`，不在模块加载期固化语言）。
 *
 * 存 key 而不是文案：模块常量若存中文，切换语言后它不会跟着变 ——
 * 这正是「切了语言还有旧串」的典型来源。
 */
export const LAN_PHASE_KEY = {
  discover: "lan.phase.discover",
  probe: "lan.phase.probe",
  neighbors: "lan.phase.neighbors",
  merge: "lan.phase.merge",
} as const satisfies Record<LanProgress["phase"], NoParamKey>;

/** 设备类型展示词（查不到 hostname 时它就是主标题） */
export function kindLabel(kind: DeviceKind, locale: Locale = currentLocale()): string {
  const { t } = i18nFor(locale);
  switch (kind) {
    case "router":
      return t("lan.kind.router");
    case "computer":
      return t("lan.kind.computer");
    case "phone":
      return t("lan.kind.phone");
    case "tablet":
      return t("lan.kind.tablet");
    case "tv":
      return t("lan.kind.tv");
    case "speaker":
      return t("lan.kind.speaker");
    case "printer":
      return t("lan.kind.printer");
    case "nas":
      return t("lan.kind.nas");
    case "camera":
      return t("lan.kind.camera");
    case "iot":
      return t("lan.kind.iot");
    default:
      return t("lan.kind.unknown");
  }
}
