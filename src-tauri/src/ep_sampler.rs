//! 外设采样（m21，docs/m21-peripherals-plan.md）。
//!
//! 三个独立数据源，一个快照：
//! - 蓝牙外设：`system_profiler SPBluetoothDataType -json` —— AirPods 的
//!   左/右/盒分件电量只有它给（IOPS 只有一个总百分比，见 ps_sampler）；
//! - USB 设备：`system_profiler SPUSBDataType -json` —— 总线树的叶子设备；
//! - 音频设备：CoreAudio 公开 API —— 输出/输入设备 + 默认设备标记。
//!
//! 链路（对齐 ps_sampler / disk_io_sampler 纪律）：
//! - 15 s 低频轮询，`active` 门控（外设 tab 不可见时不采，常驻开销 0）；
//! - system_profiler 单次 1–2 s，放 spawn_blocking + 6 s 超时，超时当本轮失败；
//! - 三源互相独立：单源失败 → 该源保持上一轮（时间戳不动），其它源照常更新。
//!
//! 失败语义：JSON 结构漂移 / 命令失败 / CoreAudio 出错都不产生假数据；
//! 前端按 `*_at` 时间戳与空数组自行决定渲染（读不到不画 0）。

use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{Manager, State};

use crate::settings;

/// 采样间隔（方案 §三：15 s 低频）
pub const EP_INTERVAL_SECS: u64 = 15;
/// system_profiler 冷启动可能 1–2 s，偶发更久；6 s 兜底，超时当失败
const PROFILER_TIMEOUT: Duration = Duration::from_secs(6);

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BTDevice {
    pub name: String,
    /// minorType / type（"Headphones" / "Keyboard"…）；读不到为空串
    pub device_type: String,
    pub connected: bool,
    /// vendorID == 0x004C（Apple）——前端据此选图标
    pub is_apple: bool,
    /// None = 该部件读不到电量（前端不画环/条）
    pub battery_main: Option<u8>,
    /// AirPods 独有
    pub battery_left: Option<u8>,
    pub battery_right: Option<u8>,
    pub battery_case: Option<u8>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct USBDevice {
    pub name: String,
    pub vendor: Option<String>,
    pub speed: Option<String>,
    /// 总线供电能力（mA）
    pub power_ma: Option<u32>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AudioDevice {
    pub name: String,
    /// "output" | "input"
    pub direction: String,
    pub is_default: bool,
}

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct EpSnapshot {
    pub bt: Vec<BTDevice>,
    pub usb: Vec<USBDevice>,
    pub audio: Vec<AudioDevice>,
    /// 各源最近一次成功采集（Unix 秒）；失败时保持旧值，前端判新鲜度
    pub bt_at: Option<u64>,
    pub usb_at: Option<u64>,
    pub audio_at: Option<u64>,
}

pub struct EpState {
    snapshot: Mutex<Option<EpSnapshot>>,
    active: AtomicBool,
    last_at: Mutex<Option<Instant>>,
}

impl Default for EpState {
    fn default() -> Self {
        Self::new()
    }
}

impl EpState {
    pub fn new() -> Self {
        Self {
            snapshot: Mutex::new(None),
            active: AtomicBool::new(false),
            last_at: Mutex::new(None),
        }
    }

    pub fn snapshot(&self) -> Option<EpSnapshot> {
        self.snapshot
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn set_active(&self, active: bool) {
        self.active.store(active, Ordering::Relaxed);
        // 激活时清掉上次时间戳：下一个 tick（≤5 s）立刻采一轮，
        // 用户进 tab 不用干等 15 s（与 ps_sampler 同策略）
        if active {
            *self.last_at.lock().unwrap_or_else(|e| e.into_inner()) = None;
        }
    }

    fn is_active(&self) -> bool {
        self.active.load(Ordering::Relaxed)
    }
}

// ---------------------------------------------------------------- 蓝牙 / USB（system_profiler）

/// 跑一次 system_profiler 并解析 JSON。None = 命令失败 / 超时 / 非法 JSON。
fn run_profiler(data_type: &str) -> Option<serde_json::Value> {
    let out = Command::new("/usr/sbin/system_profiler")
        .arg(data_type)
        .arg("-json")
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    serde_json::from_slice(&out.stdout).ok()
}

/// 电量键值：字符串常见（"92" / "92%"），也可能是数字；0–100 之外丢弃
fn parse_battery(v: &serde_json::Value) -> Option<u8> {
    let n: i64 = match v {
        serde_json::Value::Number(n) => n.as_i64()?,
        serde_json::Value::String(s) => {
            let t = s.trim().trim_end_matches('%').trim();
            t.parse().ok()?
        }
        _ => return None,
    };
    u8::try_from(n).ok().filter(|p| (1..=100).contains(p))
}

fn str_field(obj: &serde_json::Value, key: &str) -> Option<String> {
    obj.get(key)
        .and_then(serde_json::Value::as_str)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn parse_bt_device(name: &str, props: &serde_json::Value, connected: bool) -> BTDevice {
    let battery = |k: &str| props.get(k).and_then(parse_battery);
    BTDevice {
        name: name.to_string(),
        device_type: str_field(props, "device_minorType")
            .or_else(|| str_field(props, "device_type"))
            .unwrap_or_default(),
        connected,
        // "0x004C (Apple)" / "0x004C" 两种写法都盖住
        is_apple: str_field(props, "device_vendorID")
            .map(|v| v.starts_with("0x004C"))
            .unwrap_or(false),
        battery_main: battery("device_batteryLevelMain"),
        battery_left: battery("device_batteryLevelLeft"),
        battery_right: battery("device_batteryLevelRight"),
        battery_case: battery("device_batteryLevelCase"),
    }
}

/// 蓝牙解析（真机 2026-11-20 实测结构）：
/// `SPBluetoothDataType[] → controller → device_connected / device_not_connected`
/// → 条目 `{ "<设备名>": { 属性 } }`。分组名即连接状态（字段漂移时也不产生假数据）。
fn parse_bt(v: &serde_json::Value) -> Vec<BTDevice> {
    let mut out = Vec::new();
    let Some(arr) = v.get("SPBluetoothDataType").and_then(serde_json::Value::as_array) else {
        return out;
    };
    for ctrl in arr {
        for (connected, key) in [(true, "device_connected"), (false, "device_not_connected")] {
            let Some(list) = ctrl.get(key).and_then(serde_json::Value::as_array) else {
                continue;
            };
            for item in list {
                let Some(obj) = item.as_object() else { continue };
                for (name, props) in obj {
                    if !name.is_empty() {
                        out.push(parse_bt_device(name, props, connected));
                    }
                }
            }
        }
    }
    // 已连接在前（同类内稳定，不额外排序破坏 system_profiler 顺序）
    out.sort_by_key(|d| !d.connected);
    out
}

/// USB 叶子判据：有 manufacturer / product_id 的是设备（hub 层只有 _name + _items）。
/// 只取叶子、扁平化，忽略 hub 自身（方案 R7）。
fn walk_usb(node: &serde_json::Value, out: &mut Vec<USBDevice>) {
    let is_leaf = node.get("manufacturer").is_some() || node.get("product_id").is_some();
    if is_leaf {
        out.push(USBDevice {
            name: str_field(node, "_name").unwrap_or_else(|| "USB Device".to_string()),
            vendor: str_field(node, "manufacturer"),
            speed: str_field(node, "device_speed"),
            power_ma: node
                .get("bus_power")
                .and_then(|v| v.as_u64().map(|n| n.min(u32::MAX as u64) as u32)),
        });
    }
    if let Some(items) = node.get("_items").and_then(serde_json::Value::as_array) {
        for item in items {
            walk_usb(item, out);
        }
    }
}

fn parse_usb(v: &serde_json::Value) -> Vec<USBDevice> {
    let mut out = Vec::new();
    let Some(arr) = v.get("SPUSBDataType").and_then(serde_json::Value::as_array) else {
        return out;
    };
    for bus in arr {
        walk_usb(bus, &mut out);
    }
    out
}

// ---------------------------------------------------------------- 音频（CoreAudio）

#[cfg(target_os = "macos")]
mod audio {
    //! CoreAudio 只读查询：设备列表 + 输出/输入方向 + 默认设备。
    //! 手工 extern（与 ps_sampler 的 CF 手工声明同风格），只用 GetPropertyData 读。

    use std::ffi::c_void;

    use super::AudioDevice;

    type AudioObjectID = u32;
    const K_ENCODING_UTF8: u32 = 0x0800_0100;

    /// 四字符码常量（'dev ' / 'glob' / 'outp' / 'inpt' / 'str#' / 'dOut' / 'dIn ' / 'lnam'）
    const SEL_DEVICES: u32 = 0x6465_7620;
    const SCOPE_GLOBAL: u32 = 0x676C_6F62;
    const SCOPE_OUTPUT: u32 = 0x6F75_7470;
    const SCOPE_INPUT: u32 = 0x696E_7074;
    const SEL_STREAMS: u32 = 0x7374_7223;
    const SEL_DEFAULT_OUTPUT: u32 = 0x644F_7574;
    const SEL_DEFAULT_INPUT: u32 = 0x6449_6E20;
    const SEL_NAME: u32 = 0x6C6E_616D;
    /// kAudioObjectSystemObject / kAudioObjectPropertyElementMain
    const SYSTEM_OBJECT: AudioObjectID = 1;
    const ELEMENT_MAIN: u32 = 0;

    #[repr(C)]
    struct PropAddr {
        selector: u32,
        scope: u32,
        element: u32,
    }

    #[link(name = "CoreAudio", kind = "framework")]
    extern "C" {
        fn AudioObjectGetPropertyDataSize(
            id: AudioObjectID,
            addr: *const PropAddr,
            qualifier_size: u32,
            qualifier: *const c_void,
            out_size: *mut u32,
        ) -> i32;
        fn AudioObjectGetPropertyData(
            id: AudioObjectID,
            addr: *const PropAddr,
            qualifier_size: u32,
            qualifier: *const c_void,
            io_size: *mut u32,
            out_data: *mut c_void,
        ) -> i32;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFStringGetCString(s: *const c_void, buf: *mut i8, buf_size: isize, encoding: u32) -> u8;
        fn CFRelease(cf: *const c_void);
    }

    fn addr(selector: u32, scope: u32) -> PropAddr {
        PropAddr {
            selector,
            scope,
            element: ELEMENT_MAIN,
        }
    }

    /// 读取一个 AudioObjectID 属性（4 字节 ID / u32 数组通用）
    fn prop_ids(id: AudioObjectID, a: &PropAddr) -> Option<Vec<AudioObjectID>> {
        let mut size: u32 = 0;
        let st = unsafe {
            AudioObjectGetPropertyDataSize(id, a, 0, std::ptr::null(), &mut size)
        };
        if st != 0 || size == 0 {
            return None;
        }
        let n = (size as usize) / 4;
        let mut buf = vec![0u32; n];
        let mut io = size;
        let st = unsafe {
            AudioObjectGetPropertyData(id, a, 0, std::ptr::null(), &mut io, buf.as_mut_ptr() as *mut c_void)
        };
        if st != 0 {
            return None;
        }
        Some(buf)
    }

    fn device_name(id: AudioObjectID) -> Option<String> {
        let a = addr(SEL_NAME, SCOPE_GLOBAL);
        let mut io: u32 = 8; // CFStringRef 指针宽度按 8 处理（64-bit）
        let mut cf: *const c_void = std::ptr::null();
        let st = unsafe {
            AudioObjectGetPropertyData(id, &a, 0, std::ptr::null(), &mut io, &mut cf as *mut _ as *mut c_void)
        };
        if st != 0 || cf.is_null() {
            return None;
        }
        let mut buf = [0i8; 256];
        let ok = unsafe { CFStringGetCString(cf, buf.as_mut_ptr(), 256, K_ENCODING_UTF8) };
        unsafe { CFRelease(cf) };
        if ok == 0 {
            return None;
        }
        let bytes: Vec<u8> = buf
            .iter()
            .take_while(|&&c| c != 0)
            .map(|&c| c as u8)
            .collect();
        Some(String::from_utf8_lossy(&bytes).to_string())
    }

    /// 某方向有没有流（size ≥ 4 即至少一个 stream）
    fn has_streams(id: AudioObjectID, scope: u32) -> bool {
        let a = addr(SEL_STREAMS, scope);
        let mut size: u32 = 0;
        let st = unsafe {
            AudioObjectGetPropertyDataSize(id, &a, 0, std::ptr::null(), &mut size)
        };
        st == 0 && size >= 4
    }

    fn default_device(sel: u32) -> Option<AudioObjectID> {
        let a = addr(sel, SCOPE_GLOBAL);
        let mut io: u32 = 4;
        let mut out: AudioObjectID = 0;
        let st = unsafe {
            AudioObjectGetPropertyData(SYSTEM_OBJECT, &a, 0, std::ptr::null(), &mut io, &mut out as *mut _ as *mut c_void)
        };
        if st != 0 {
            return None;
        }
        Some(out)
    }

    /// 一轮 CoreAudio 读取。None = OS 调用失败（保持上轮）。
    pub fn read_audio() -> Option<Vec<AudioDevice>> {
        let ids = prop_ids(SYSTEM_OBJECT, &addr(SEL_DEVICES, SCOPE_GLOBAL))?;
        let def_out = default_device(SEL_DEFAULT_OUTPUT);
        let def_in = default_device(SEL_DEFAULT_INPUT);

        let mut out = Vec::new();
        for id in ids {
            let Some(name) = device_name(id) else { continue };
            // 有输出流 → 输出设备；有输入流 → 输入设备；双向各报一条
            if has_streams(id, SCOPE_OUTPUT) {
                out.push(AudioDevice {
                    name: name.clone(),
                    direction: "output".to_string(),
                    is_default: def_out == Some(id),
                });
            }
            if has_streams(id, SCOPE_INPUT) {
                out.push(AudioDevice {
                    name: name.clone(),
                    direction: "input".to_string(),
                    is_default: def_in == Some(id),
                });
            }
        }
        Some(out)
    }
}

#[cfg(not(target_os = "macos"))]
mod audio {
    use super::AudioDevice;
    pub fn read_audio() -> Option<Vec<AudioDevice>> {
        None
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 一轮采样：三源并发，各自成功才算数（失败保持旧时间戳与旧列表）。
async fn round(handle: &tauri::AppHandle, state: &EpState) {
    if !settings::get_bool(handle, "hw.periph.enabled").unwrap_or(true) {
        return;
    }

    let bt_task = tauri::async_runtime::spawn_blocking(|| run_profiler("SPBluetoothDataType").map(|v| parse_bt(&v)));
    let usb_task = tauri::async_runtime::spawn_blocking(|| run_profiler("SPUSBDataType").map(|v| parse_usb(&v)));
    let audio_task = tauri::async_runtime::spawn_blocking(audio::read_audio);

    // 三个任务共用 6 s 超时兜底（system_profiler 冷启动慢 / CoreAudio 卡死都算本轮失败）
    let (bt, usb, audio) = tokio::join!(
        timed(bt_task),
        timed(usb_task),
        timed(audio_task),
    );

    let mut next = state.snapshot().unwrap_or_default();
    let ts = now_secs();
    if let Some(bt) = bt {
        next.bt = bt;
        next.bt_at = Some(ts);
    }
    if let Some(usb) = usb {
        next.usb = usb;
        next.usb_at = Some(ts);
    }
    if let Some(audio) = audio {
        next.audio = audio;
        next.audio_at = Some(ts);
    }
    *state.snapshot.lock().unwrap_or_else(|e| e.into_inner()) = Some(next);
}

/// 等单个后台任务并打平错误：超时 / JoinError / 源失败 → None（保持上轮）。
/// spawn_blocking 闭包返回 Option<T>（None = 源失败），join 再包一层 tauri::Result。
async fn timed<T: Send + 'static>(
    h: tauri::async_runtime::JoinHandle<Option<T>>,
) -> Option<T> {
    tokio::time::timeout(PROFILER_TIMEOUT, h).await.ok()?.ok()?
}

/// 15 s 低频轮（5 s tick 门控，激活瞬间立即补采）
pub async fn run_ep(handle: tauri::AppHandle) {
    let mut tick = tokio::time::interval(Duration::from_secs(5));
    loop {
        tick.tick().await;
        let state = handle.state::<Arc<EpState>>();
        if !state.is_active() {
            continue;
        }
        let due = {
            let last = state.last_at.lock().unwrap_or_else(|e| e.into_inner());
            last.is_none()
                || last
                    .map(|t| t.elapsed() >= Duration::from_secs(EP_INTERVAL_SECS))
                    .unwrap_or(false)
        };
        if !due {
            continue;
        }
        *state.last_at.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
        round(&handle, &state).await;
    }
}

#[tauri::command]
pub fn ep_snapshot(state: State<'_, Arc<EpState>>) -> Option<EpSnapshot> {
    state.snapshot()
}

#[tauri::command]
pub fn ep_set_active(state: State<'_, Arc<EpState>>, active: bool) {
    state.set_active(active);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 真机 2026-11-20 结构（AirPods 已连接 + iPhone 未连接）固化为 fixture：
    /// 结构漂移时这里先红，而不是线上静默丢数据
    #[test]
    fn bt_parses_connected_and_parts() {
        let v = serde_json::json!({
            "SPBluetoothDataType": [{
                "controller_properties": { "controller_state": "attrib_on" },
                "device_connected": [{
                    "许一的AirPods": {
                        "device_minorType": "Headphones",
                        "device_vendorID": "0x004C",
                        "device_batteryLevelLeft": "92",
                        "device_batteryLevelRight": "88%",
                        "device_batteryLevelCase": 100
                    }
                }],
                "device_not_connected": [
                    { "iPhone (2)": { "device_address": "AA:BB" } },
                    { "许一的Apple Watch": {} }
                ]
            }]
        });
        let out = parse_bt(&v);
        assert_eq!(out.len(), 3);
        // 已连接排前
        let pods = &out[0];
        assert_eq!(pods.name, "许一的AirPods");
        assert!(pods.connected);
        assert!(pods.is_apple);
        assert_eq!(pods.device_type, "Headphones");
        assert_eq!(pods.battery_left, Some(92));
        assert_eq!(pods.battery_right, Some(88));
        assert_eq!(pods.battery_case, Some(100));
        assert_eq!(pods.battery_main, None);
        // 未连接设备无电量字段
        for d in &out[1..] {
            assert!(!d.connected);
            assert_eq!(d.battery_main, None);
        }
    }

    #[test]
    fn bt_rejects_dirty_battery_and_missing_group() {
        // 电量 0 / 超范围 / 空串 → 全部丢弃（脏数据不进 UI）
        let v = serde_json::json!({
            "SPBluetoothDataType": [{
                "device_connected": [{
                    "奇怪设备": {
                        "device_batteryLevelMain": "0",
                        "device_batteryLevelLeft": "150",
                        "device_batteryLevelRight": ""
                    }
                }]
            }]
        });
        let out = parse_bt(&v);
        assert_eq!(out.len(), 1);
        let d = &out[0];
        assert_eq!(d.battery_main, None);
        assert_eq!(d.battery_left, None);
        assert_eq!(d.battery_right, None);

        // 整个分组缺失 → 空列表（None 语义留给调用方：不更新时间戳）
        assert!(parse_bt(&serde_json::json!({})).is_empty());
    }

    #[test]
    fn usb_walks_leaves_only() {
        let v = serde_json::json!({
            "SPUSBDataType": [
                { "_name": "USB31Bus", "host_controller": "AppleXHCI" },
                { "_name": "USB31Bus", "_items": [
                    { "_name": "USB Hub", "_items": [
                        { "_name": "SanDisk U盘", "manufacturer": "SanDisk", "device_speed": "high_speed", "bus_power": 224 }
                    ]}
                ]}
            ]
        });
        let out = parse_usb(&v);
        // hub 自身（无 manufacturer/product_id）不进列表，只留叶子
        assert_eq!(out.len(), 1);
        let d = &out[0];
        assert_eq!(d.name, "SanDisk U盘");
        assert_eq!(d.vendor.as_deref(), Some("SanDisk"));
        assert_eq!(d.speed.as_deref(), Some("high_speed"));
        assert_eq!(d.power_ma, Some(224));
    }
}
