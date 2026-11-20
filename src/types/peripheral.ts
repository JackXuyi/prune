/**
 * 外设类型契约（m21，docs/m21-peripherals-plan.md §5.3）。
 *
 * 与 Rust `ep_sampler.rs` serde camelCase 对齐；None/失败统一为 null/空数组，
 * 前端「读不到不画 0」。电量字段 None = 该部件没上报（不画环/条）。
 */

/** 蓝牙外设（耳机 / 键盘 / 鼠标 / 触控板…） */
export interface BTDevice {
  name: string;
  /** minorType / type（"Headphones" / "Keyboard"…）；读不到为空串 */
  deviceType: string;
  connected: boolean;
  /** vendorID == 0x004C（Apple）——据此选图标 */
  isApple: boolean;
  /** 第三方设备的主电量；Apple 音频设备常无此字段 */
  batteryMain: number | null;
  /** AirPods 分件电量：只有左/右/盒，没有主电量 */
  batteryLeft: number | null;
  batteryRight: number | null;
  batteryCase: number | null;
}

/** USB 叶子设备（hub 自身不上报，扁平化输出） */
export interface USBDevice {
  name: string;
  vendor: string | null;
  /** "high_speed" / "super_speed"…（system_profiler 原文） */
  speed: string | null;
  /** 总线供电能力（mA） */
  powerMa: number | null;
}

/** CoreAudio 设备；双向设备（聚合器）会拆成两条 */
export interface AudioDevice {
  name: string;
  direction: "output" | "input";
  isDefault: boolean;
}

/** 外设快照：三源互相独立，单源失败保持旧列表与旧时间戳 */
export interface EpSnapshot {
  bt: BTDevice[];
  usb: USBDevice[];
  audio: AudioDevice[];
  /** 各源最近一次成功采集（Unix 秒）；null = 从未成功 */
  btAt: number | null;
  usbAt: number | null;
  audioAt: number | null;
}
