/**
 * `lan` namespace：局域网分段（LanBoard / LanDeviceCard / 设备详情弹窗）
 * 与设备类型、扫描阶段词表。
 *
 * 两条边界写进文案、翻译时不能删（m18-lan-devices-spec §11）：
 * 1. **进入分段不扫描** —— 首次扫描会触发 macOS 本地网络授权；
 * 2. 只探测、不访问设备上的服务 —— 这句话是用户敢点按钮的前提。
 *
 * 「随机 MAC」不是错误：约一半设备都是，文案要解释它对**聚合口径**的影响，
 * 而不是暗示设备有问题。
 */
import type { MessageTree } from "../../types";

const zh = {
  title: "局域网",

  phase: {
    preparing: "正在准备…",
    withProgress: "{label}（{done}/{total}）",
    discover: "正在发现服务（mDNS / SSDP）",
    probe: "正在探测网段",
    neighbors: "正在读取邻居表",
    merge: "正在合成结果",
  },

  scanningNote: "一轮约 30 秒（mDNS 发现窗口）。期间不会访问任何设备上的服务。",
  scan: "扫描局域网",
  rescan: "重扫",
  rescanTitle: "重新扫描一轮（约 30 秒）",

  empty: {
    neverScannedTitle: "还没有扫描过局域网",
    neverScannedNote:
      "只探测本网段设备（TCP connect + mDNS / SSDP），不访问设备上的任何服务。首次扫描 macOS 会请求本地网络授权。",
    noDevicesTitle: "本轮没有发现设备",
    noDevicesNote:
      "探测完成才是这个结论——不是没扫。如果邻居表为空，说明本机最近没有与网段内设备通信过。",
    noInboundTitle: "没有设备主动连过我",
    noInboundNote:
      "入站连接（ESTABLISHED 且对端在网段内）为零。可能只是没有长期服务与这台机器保持连接——不代表本轮扫描没跑。",
    noOnlineTitle: "没有在线设备",
    noOnlineNote: "扫描过了才是这个结论——邻居表里只有离线记录。",
  },

  filter: {
    aria: "设备筛选",
    all: "全部",
    online: "在线",
    inbound: "连了我",
  },

  metric: {
    online: "在线设备",
    onlineUnit: "/ {n} 台",
    exposed: "对我开放的端口",
    exposedUnit: "个",
    elapsed: "扫描耗时",
    elapsedUnit: "s",
    probed: "探测 {scanned}/{total}",
    neighborsOnly: "只读邻居表",
    noExposed: "我对同网段开放 0 个端口",
  },

  randomMacNote:
    "{n} 台设备使用随机化 MAC——无法识别厂商，跨轮次聚合以 IP + 网段为准。",

  section: {
    inbound: "连了我的设备",
    pinned: "本机与网关",
    online: "在线设备",
  },

  offline: {
    toggle: "已离线 {n} 台",
    hint: "点击展开 · 置灰展示，不占用首屏",
  },

  footer:
    "{devices} 台设备 · 扫描 {seconds} s · {clock} · 本轮快照语义：跨轮次的「同一台设备」以稳定 MAC 优先；随机 MAC / 无 MAC 以 IP + 网段为准",

  /** 设备卡 / 详情弹窗共用的身份与状态词 */
  card: {
    self: "本机",
    gateway: "网关",
    randomMac: "随机 MAC",
    stateOnline: "在线",
    stateStale: "最近在线",
    stateOffline: "不可达",
    openTitle: "点击查看设备详情",
    badgeSelf: "自己",
    badgeUplink: "上联",
    badgeNew: "新",
    lastActive: "活跃 {ago}",
    inbound: "连了我 {ports}",
    detailAria: "设备详情：{title}",
  },

  field: {
    ip: "IP 地址",
    mac: "MAC 地址",
    macRandom: "随机",
    macMissing: "查不到（邻居表无记录）",
    vendor: "厂商",
    vendorUnknown: "未知",
    kind: "设备类型",
    hostname: "主机名",
    hostnameMissing: "未广播（mDNS 无记录）",
    services: "发现的服务",
    servicesNone: "未发现",
    inbound: "入站连接",
    inboundNone: "无",
    firstSeen: "加入时间",
    lastSeen: "最后活跃",
    timeMissing: "—（旧版快照无记录）",
  },

  modal: {
    randomMacNote:
      "随机化 MAC：设备为防跟踪轮换硬件地址，厂商识别与跨轮次聚合（以 IP + 网段为准）均受影响。",
    footer: "Esc 或点空白处关闭 · 数据来自本轮扫描快照",
    close: "关闭",
    copy: "复制",
    copied: "已复制",
  },

  /** 设备类型（`kindLabel`）：查不到 hostname 时它就是主标题 */
  kind: {
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
  },
} as const;

const en: MessageTree<typeof zh> = {
  title: "LAN",

  phase: {
    preparing: "Getting ready…",
    withProgress: "{label} ({done}/{total})",
    discover: "Discovering services (mDNS / SSDP)",
    probe: "Probing the subnet",
    neighbors: "Reading the neighbor table",
    merge: "Merging results",
  },

  scanningNote:
    "One round takes about 30 seconds (the mDNS discovery window). Nothing on any device is accessed during it.",
  scan: "Scan the LAN",
  rescan: "Rescan",
  rescanTitle: "Run another scan (about 30 seconds)",

  empty: {
    neverScannedTitle: "The LAN hasn't been scanned yet",
    neverScannedNote:
      "Only devices on your own subnet are probed (TCP connect + mDNS / SSDP); no service on any device is accessed. The first scan triggers macOS's local-network permission prompt.",
    noDevicesTitle: "No devices found this round",
    noDevicesNote:
      "That conclusion holds because the probe finished — not because nothing ran. An empty neighbor table means this machine hasn't talked to anything on the subnet recently.",
    noInboundTitle: "Nothing has connected to me",
    noInboundNote:
      "Inbound connections (ESTABLISHED with a peer on the subnet) are zero. It may simply be that no long-lived service keeps a connection to this machine — it doesn't mean the scan didn't run.",
    noOnlineTitle: "No devices online",
    noOnlineNote:
      "That conclusion holds because a scan ran — the neighbor table only has offline entries.",
  },

  filter: {
    aria: "Device filter",
    all: "All",
    online: "Online",
    inbound: "Connected to me",
  },

  metric: {
    online: "Devices online",
    onlineUnit: "/ {n}",
    exposed: "Ports open to me",
    exposedUnit: "",
    elapsed: "Scan time",
    elapsedUnit: "s",
    probed: "probed {scanned}/{total}",
    neighborsOnly: "neighbor table only",
    noExposed: "I expose 0 ports to this subnet",
  },

  randomMacNote:
    "{n} devices use a randomized MAC — the vendor can't be identified, so rounds are aggregated by IP + subnet.",

  section: {
    inbound: "Devices connected to me",
    pinned: "This machine & the gateway",
    online: "Devices online",
  },

  offline: {
    toggle: "{n} offline",
    hint: "Click to expand · greyed out, kept off the first screen",
  },

  footer:
    "{devices} devices · scanned in {seconds} s · {clock} · Snapshot semantics: the same device across rounds is keyed on a stable MAC first; randomized or missing MACs fall back to IP + subnet",

  card: {
    self: "This machine",
    gateway: "Gateway",
    randomMac: "Random MAC",
    stateOnline: "Online",
    stateStale: "Recently online",
    stateOffline: "Unreachable",
    openTitle: "Click for device details",
    badgeSelf: "this one",
    badgeUplink: "uplink",
    badgeNew: "new",
    lastActive: "Active {ago}",
    inbound: "Connected to me {ports}",
    detailAria: "Device details: {title}",
  },

  field: {
    ip: "IP address",
    mac: "MAC address",
    macRandom: "random",
    macMissing: "Not found (no neighbor-table entry)",
    vendor: "Vendor",
    vendorUnknown: "Unknown",
    kind: "Device type",
    hostname: "Hostname",
    hostnameMissing: "Not broadcast (no mDNS record)",
    services: "Discovered services",
    servicesNone: "None",
    inbound: "Inbound connections",
    inboundNone: "None",
    firstSeen: "First seen",
    lastSeen: "Last active",
    timeMissing: "— (not in this snapshot)",
  },

  modal: {
    randomMacNote:
      "Randomized MAC: the device rotates its hardware address to avoid tracking, which affects both vendor identification and cross-round aggregation (which keys on IP + subnet).",
    footer: "Press Esc or click outside to close · data comes from this round's snapshot",
    close: "Close",
    copy: "Copy",
    copied: "Copied",
  },

  kind: {
    router: "Router",
    computer: "Computer",
    phone: "Phone",
    tablet: "Tablet",
    tv: "TV",
    speaker: "Speaker",
    printer: "Printer",
    nas: "NAS",
    camera: "Camera",
    iot: "IoT",
    unknown: "Unknown",
  },
};

export const lan = { zh, en } as const;
