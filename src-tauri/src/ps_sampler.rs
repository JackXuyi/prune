//! 电源采样（f26 G-05 + G-06，docs/f26-hardware-modules-plan.md D1）。
//!
//! **一个数据源两个能力**：macOS 的 IOPowerSources 把内置电池和蓝牙配件
//! （键盘/鼠标/触控板/耳机）都列为电源源，一套 API 覆盖电池卡与配件电量。
//!
//! 链路（与 port_sampler 同纪律）：
//! - 30 s 低频轮询，`active` 门控（概览 tab 不可见时不采，常驻开销 0）；
//! - IOKit IOPS 是公开 API（IOPSCopyPowerSourcesInfo），读一次 ≈ 毫秒级；
//! - 时间剩余走 `IOPSGetTimeRemainingEstimate`（-1 = 未知，-2 = 接电不限）。
//!
//! 失败语义：任何一步失败 → 快照保持上一轮（或 None）→ 前端不渲染，
//! 绝不用 0 冒充读数。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{Manager, State};

use crate::settings;

/// 采样间隔（方案 §三：30 s 低频常采）
pub const PS_INTERVAL_SECS: u64 = 30;

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PowerSource {
    /// "battery" = 内置电池；"accessory" = 蓝牙等外接配件
    pub kind: String,
    pub name: String,
    /// 0–100；IOPS 不给 MaxCapacity 的源就是已经的百分比
    pub percent: Option<f64>,
    pub charging: bool,
    /// 内置电池的剩余分钟数；配件恒 None；未知 = None
    pub time_to_empty_min: Option<i64>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PowerSnapshot {
    pub sources: Vec<PowerSource>,
    /// 采集时间（Unix 秒）
    pub collected_at: u64,
}

pub struct PowerState {
    snapshot: Mutex<Option<PowerSnapshot>>,
    active: AtomicBool,
    last_at: Mutex<Option<Instant>>,
}

impl Default for PowerState {
    fn default() -> Self {
        Self::new()
    }
}

impl PowerState {
    pub fn new() -> Self {
        Self {
            snapshot: Mutex::new(None),
            active: AtomicBool::new(false),
            last_at: Mutex::new(None),
        }
    }

    pub fn snapshot(&self) -> Option<PowerSnapshot> {
        self.snapshot
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn set_active(&self, active: bool) {
        self.active.store(active, Ordering::Relaxed);
        // 激活时清掉上次时间戳：下一个 tick（≤5 s）立刻采一轮，
        // 用户进 tab 不用干等 30 s
        if active {
            *self.last_at.lock().unwrap_or_else(|e| e.into_inner()) = None;
        }
    }

    fn is_active(&self) -> bool {
        self.active.load(Ordering::Relaxed)
    }
}

#[cfg(target_os = "macos")]
mod ffi {
    //! 手工声明 CoreFoundation / IOKit externs：CF 只用到查字典的四个函数，
    //! 引 typed crate 反而引入版本摩擦。所有句柄 get/create 语义见各函数注释。

    use std::ffi::c_void;

    use super::PowerSource;

    const K_CF_ENCODING_UTF8: u32 = 0x0800_0100;
    /// kCFNumberSInt64Type
    const K_CF_NUMBER_SINT64: i32 = 4;

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFStringCreateWithCString(
            alloc: *const c_void,
            c_str: *const i8,
            encoding: u32,
        ) -> *const c_void;
        fn CFStringGetCString(s: *const c_void, buf: *mut i8, buf_size: isize, encoding: u32)
            -> u8;
        fn CFDictionaryGetValue(dict: *const c_void, key: *const c_void) -> *const c_void;
        fn CFNumberGetValue(number: *const c_void, the_type: i32, value_ptr: *mut c_void) -> u8;
        fn CFBooleanGetValue(b: *const c_void) -> u8;
        fn CFRelease(cf: *const c_void);
    }

    #[link(name = "IOKit", kind = "framework")]
    extern "C" {
        fn IOPSCopyPowerSourcesInfo() -> *const c_void;
        fn IOPSCopyPowerSourcesList(snapshot: *const c_void) -> *const c_void;
        fn IOPSGetPowerSourceDescription(
            snapshot: *const c_void,
            ps: *const c_void,
        ) -> *const c_void;
        fn IOPSGetTimeRemainingEstimate() -> f64;
    }

    extern "C" {
        fn CFArrayGetCount(arr: *const c_void) -> isize;
        fn CFArrayGetValueAtIndex(arr: *const c_void, idx: isize) -> *const c_void;
    }

    /// 创建常驻键 CFString（进程生命周期持有，不 release——键是常量字符串）
    fn key(s: &str) -> *const c_void {
        let c = std::ffi::CString::new(s).unwrap_or_default();
        unsafe { CFStringCreateWithCString(std::ptr::null(), c.as_ptr(), K_CF_ENCODING_UTF8) }
    }

    fn dict_str(dict: *const c_void, k: *const c_void) -> Option<String> {
        let v = unsafe { CFDictionaryGetValue(dict, k) };
        if v.is_null() {
            return None;
        }
        let mut buf = [0i8; 256];
        if unsafe { CFStringGetCString(v, buf.as_mut_ptr(), 256, K_CF_ENCODING_UTF8) } == 0 {
            return None;
        }
        let bytes: Vec<u8> = buf
            .iter()
            .take_while(|&&c| c != 0)
            .map(|&c| c as u8)
            .collect();
        Some(String::from_utf8_lossy(&bytes).to_string())
    }

    fn dict_i64(dict: *const c_void, k: *const c_void) -> Option<i64> {
        let v = unsafe { CFDictionaryGetValue(dict, k) };
        if v.is_null() {
            return None;
        }
        let mut out: i64 = 0;
        if unsafe { CFNumberGetValue(v, K_CF_NUMBER_SINT64, &mut out as *mut i64 as *mut c_void) }
            == 0
        {
            return None;
        }
        Some(out)
    }

    fn dict_bool(dict: *const c_void, k: *const c_void) -> Option<bool> {
        let v = unsafe { CFDictionaryGetValue(dict, k) };
        if v.is_null() {
            return None;
        }
        Some(unsafe { CFBooleanGetValue(v) } != 0)
    }

    /// 一轮 IOPS 读取。返回 None = IOKit 调用失败（上一轮快照保持）。
    pub fn read_sources() -> Option<Vec<PowerSource>> {
        // 常驻键（进程生命周期内复用）
        struct Keys {
            name: *const c_void,
            typ: *const c_void,
            transport: *const c_void,
            current: *const c_void,
            max: *const c_void,
            charging: *const c_void,
        }
        let keys = Keys {
            name: key("Name"),
            typ: key("Type"),
            transport: key("Transport Type"),
            current: key("Current Capacity"),
            max: key("Max Capacity"),
            charging: key("Is Charging"),
        };

        let snapshot = unsafe { IOPSCopyPowerSourcesInfo() };
        if snapshot.is_null() {
            return None;
        }
        let list = unsafe { IOPSCopyPowerSourcesList(snapshot) };
        if list.is_null() {
            unsafe { CFRelease(snapshot) };
            return None;
        }

        let mut out = Vec::new();
        let count = unsafe { CFArrayGetCount(list) };
        for i in 0..count {
            let src = unsafe { CFArrayGetValueAtIndex(list, i) };
            let dict = unsafe { IOPSGetPowerSourceDescription(snapshot, src) };
            if dict.is_null() {
                continue;
            }
            // get 语义：返回 snapshot 内部字典，不 release

            let name = dict_str(dict, keys.name).unwrap_or_default();
            let typ = dict_str(dict, keys.typ).unwrap_or_default();
            let transport = dict_str(dict, keys.transport).unwrap_or_default();

            // 内置电池：Type = InternalBattery；蓝牙配件：Transport Type = Bluetooth
            let kind = if typ == "InternalBattery" {
                "battery"
            } else if transport == "Bluetooth" {
                "accessory"
            } else {
                continue; // 其余 UPS 等电源源与两个能力无关，直接跳过
            };

            // Current Capacity 是 0–100 的整数；有 Max Capacity 的源用比例更准
            let current = dict_i64(dict, keys.current).map(|v| v as f64);
            let max = dict_i64(dict, keys.max).map(|v| v as f64);
            let percent = match (current, max) {
                (Some(c), Some(m)) if m > 0.0 => Some((c / m) * 100.0),
                (Some(c), None) => Some(c),
                _ => None,
            };
            // 0–100 之外的读数丢弃（脏数据不该进 UI）
            let percent = percent.filter(|p| (0.0..=100.0).contains(p));

            let charging = dict_bool(dict, keys.charging).unwrap_or(false);

            out.push(PowerSource {
                kind: kind.to_string(),
                name,
                percent,
                charging,
                time_to_empty_min: None,
            });
        }
        unsafe { CFRelease(list) };
        unsafe { CFRelease(snapshot) };

        // 剩余时间只对内置电池有意义；IOPS 全局一次估计，挂到 battery 上
        if out.iter().any(|s| s.kind == "battery") {
            let secs = unsafe { IOPSGetTimeRemainingEstimate() };
            // -1 = 未知（正在计算）；-2 = 接电（不限时）
            let mins = if secs >= 0.0 {
                Some((secs / 60.0).round() as i64)
            } else {
                None
            };
            for s in out.iter_mut() {
                if s.kind == "battery" {
                    s.time_to_empty_min = mins;
                }
            }
        }
        Some(out)
    }
}

#[cfg(not(target_os = "macos"))]
mod ffi {
    use super::PowerSource;
    pub fn read_sources() -> Option<Vec<PowerSource>> {
        None
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 一轮采样。开关关闭 / IOKit 失败时保持快照不动。
fn round(handle: &tauri::AppHandle, state: &PowerState) {
    if !settings::get_bool(handle, "hw.power.enabled").unwrap_or(true) {
        return;
    }
    if let Some(sources) = ffi::read_sources() {
        let snap = PowerSnapshot {
            sources,
            collected_at: now_secs(),
        };
        *state.snapshot.lock().unwrap_or_else(|e| e.into_inner()) = Some(snap);
    }
}

/// 30 s 低频轮（5 s tick 门控，激活瞬间立即补采）
pub async fn run_power(handle: tauri::AppHandle) {
    let mut tick = tokio::time::interval(Duration::from_secs(5));
    loop {
        tick.tick().await;
        let state = handle.state::<Arc<PowerState>>();
        if !state.is_active() {
            continue;
        }
        let due = {
            let last = state.last_at.lock().unwrap_or_else(|e| e.into_inner());
            last.is_none()
                || last
                    .map(|t| t.elapsed() >= Duration::from_secs(PS_INTERVAL_SECS))
                    .unwrap_or(false)
        };
        if !due {
            continue;
        }
        *state.last_at.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
        round(&handle, &state);
    }
}

#[tauri::command]
pub fn power_snapshot(state: State<'_, Arc<PowerState>>) -> Option<PowerSnapshot> {
    state.snapshot()
}

#[tauri::command]
pub fn power_set_active(state: State<'_, Arc<PowerState>>, active: bool) {
    state.set_active(active);
}
