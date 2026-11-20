//! SMC 私有接口读取（f26 G-07 阶段三，docs/f26-hardware-modules-plan.md D4）。
//!
//! 协议参照（交叉核对）：macmon sources.rs、yolo-labz/fand SMC-PROTOCOL.md、
//! smcFanControl smc-command。要点：
//! - IOServiceMatching("AppleSMC") → IOServiceOpen；Apple Silicon 上要打开的
//!   子服务名是 **AppleSMCKeysEndpoint**（Intel 是 AppleSMC 本体）；
//! - IOConnectCallStructMethod(conn, **2**, …) 是读写唯一入口；
//! - data8 命令：9 = 取键元信息（size/type），5 = 读值，8 = 按索引枚举；
//! - 结构体 80 字节（repr(C) 自然对齐与 C 一致，macmon 实测可用）；
//! - **Apple Silicon 温度是 `flt`（f32 LE）**，Intel 是 `sp78`（BE i16/256）。
//!
//! 失败语义（D4）：键表探测失败 → 全部返回 None → 前端整卡不渲染。

use std::collections::HashMap;
use std::ffi::c_void;

/// CPU 温度候选键。键存在性因芯片代际而异，读到即用，全失败 = 不可用。
/// - Intel：TC0P（CPU 近端）等，类型 sp78
/// - Apple Silicon：Tp0x（P 核）/ Te0x（E 核）/ Tg0x（GPU 近端），类型 flt
pub const TEMP_KEYS: &[&str] = &[
    "TC0P", "TC0D", "Tp01", "Tp02", "Tp03", "Tp04", "Tp05", "Tp06", "Tp07", "Tp08", "Te05", "Te06",
    "Te07", "Te08", "Tm01", "Tm02", "Tm03", "Tm04", "Tg05", "Tg06",
];

/// 读值合理性区间（方案 D4）：-10~120 ℃ 之外丢弃
const TEMP_MIN: f64 = -10.0;
const TEMP_MAX: f64 = 120.0;

#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
struct KeyDataVer {
    major: u8,
    minor: u8,
    build: u8,
    reserved: u8,
    release: u16,
}

#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
struct PLimitData {
    version: u16,
    length: u16,
    cpu_p_limit: u32,
    gpu_p_limit: u32,
    mem_p_limit: u32,
}

#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
struct KeyInfo {
    data_size: u32,
    /// fourCC（如 "flt " / "sp78" / "ui8 "），大端 u32
    data_type: u32,
    data_attributes: u8,
}

#[repr(C)]
#[derive(Debug, Default)]
struct KeyData {
    key: u32,
    vers: KeyDataVer,
    p_limit_data: PLimitData,
    key_info: KeyInfo,
    result: u8,
    status: u8,
    /// 9 = 取键信息，5 = 读值
    data8: u8,
    data32: u32,
    bytes: [u8; 32],
}

extern "C" {
    /// 注意：mach_task_self_ 是**全局变量**（mach_init.h: `#define mach_task_self()
    /// mach_task_self_`），按函数调用会 SIGBUS（首轮 spike 实测）
    static mach_task_self_: u32;
    fn IOServiceMatching(name: *const i8) -> *const c_void;
    fn IOServiceGetMatchingServices(
        main_port: u32,
        matching: *const c_void,
        existing: *mut u32,
    ) -> i32;
    fn IOIteratorNext(iterator: u32) -> u32;
    fn IORegistryEntryGetName(entry: u32, name: *mut i8) -> i32;
    fn IOObjectRelease(obj: u32) -> i32;
    fn IOServiceOpen(device: u32, task: u32, typ: u32, conn: *mut u32) -> i32;
    fn IOServiceClose(conn: u32) -> i32;
    fn IOConnectCallStructMethod(
        conn: u32,
        selector: u32,
        input: *const c_void,
        input_size: usize,
        output: *mut c_void,
        output_size: *mut usize,
    ) -> i32;
}

/// fourcc → u32（大端，与 SMC 键编码一致：'f'<<24 | 'l'<<16 | 't'<<8 | ' '）
const fn fourcc(s: &str) -> u32 {
    let bytes = s.as_bytes();
    let mut acc: u32 = 0;
    let mut i = 0;
    while i < bytes.len() {
        acc = (acc << 8) | bytes[i] as u32;
        i += 1;
    }
    acc
}

const TYPE_FLT: u32 = fourcc("flt ");
const TYPE_SP78: u32 = fourcc("sp78");

struct Smc {
    conn: u32,
    /// 键元信息缓存：size/type 不变，探测一次终身用
    key_info: HashMap<u32, KeyInfo>,
}

impl Smc {
    /// 打开 AppleSMC。先找 AppleSMCKeysEndpoint（Apple Silicon），回退 AppleSMC 本体
    fn open() -> Option<Self> {
        const NAME_LEN: usize = 128;
        let matching = unsafe { IOServiceMatching(c"AppleSMC".as_ptr()) };
        if matching.is_null() {
            return None;
        }
        let mut iter: u32 = 0;
        if unsafe { IOServiceGetMatchingServices(0, matching, &mut iter) } != 0 {
            return None;
        }

        let mut primary: Option<u32> = None;
        let mut fallback: Option<u32> = None;
        let mut entry = unsafe { IOIteratorNext(iter) };
        while entry != 0 {
            let mut buf = [0i8; NAME_LEN];
            let mut next = true;
            if unsafe { IORegistryEntryGetName(entry, buf.as_mut_ptr()) } == 0 {
                let bytes: Vec<u8> = buf
                    .iter()
                    .take_while(|&&c| c != 0)
                    .map(|&c| c as u8)
                    .collect();
                let name = String::from_utf8_lossy(&bytes);
                if name == "AppleSMCKeysEndpoint" {
                    // 选中句柄不在此处 release——IOServiceOpen 之后才释放
                    primary = Some(entry);
                    next = false;
                } else if name == "AppleSMC" && fallback.is_none() {
                    fallback = Some(entry);
                    next = false;
                }
            }
            if next {
                let _ = unsafe { IOObjectRelease(entry) };
            }
            entry = unsafe { IOIteratorNext(iter) };
        }

        let device = primary.or(fallback)?;
        let mut conn: u32 = 0;
        let task = unsafe { mach_task_self_ };
        let kr = unsafe { IOServiceOpen(device, task, 0, &mut conn) };
        let _ = unsafe { IOObjectRelease(device) };
        if kr != 0 || conn == 0 {
            return None;
        }
        Some(Smc {
            conn,
            key_info: HashMap::new(),
        })
    }

    fn call(&self, input: &KeyData) -> Option<KeyData> {
        let mut out = KeyData::default();
        let mut out_size = std::mem::size_of::<KeyData>();
        let kr = unsafe {
            IOConnectCallStructMethod(
                self.conn,
                2,
                input as *const _ as *const c_void,
                std::mem::size_of::<KeyData>(),
                &mut out as *mut _ as *mut c_void,
                &mut out_size,
            )
        };
        // kr != 0 = IOKit 层失败；result != 0 = SMC 层失败（含 132「键不存在」）
        if kr != 0 || out.result != 0 {
            return None;
        }
        Some(out)
    }

    fn key_info(&mut self, key: u32) -> Option<KeyInfo> {
        if let Some(info) = self.key_info.get(&key) {
            return Some(*info);
        }
        let input = KeyData {
            key,
            data8: 9,
            ..Default::default()
        };
        let out = self.call(&input)?;
        self.key_info.insert(key, out.key_info);
        Some(out.key_info)
    }

    /// 读一个键的原始字节 + 类型。键不存在 = None（正常路径，不是错误）。
    fn read(&mut self, key: &str) -> Option<([u8; 32], u32, usize)> {
        let key_id = fourcc(key);
        let info = self.key_info(key_id)?;
        let input = KeyData {
            key: key_id,
            data8: 5,
            key_info: info,
            ..Default::default()
        };
        let out = self.call(&input)?;
        Some((out.bytes, info.data_type, info.data_size as usize))
    }

    /// 键总数（#KEY）
    fn key_count(&mut self) -> Option<u32> {
        let (bytes, dtype, size) = self.read("#KEY")?;
        if dtype != fourcc("ui32") || size < 4 {
            return None;
        }
        Some(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    /// 按索引读键名（data8 = 8）
    fn key_by_index(&mut self, index: u32) -> Option<String> {
        let input = KeyData {
            data8: 8,
            data32: index,
            ..Default::default()
        };
        let out = self.call(&input)?;
        let key = out.key.to_be_bytes();
        Some(String::from_utf8_lossy(&key).to_string())
    }

    /// 枚举所有键名（一次性探测用，几百次 SMC 调用 ≈ 数十 ms）
    fn all_keys(&mut self) -> Vec<String> {
        let Some(count) = self.key_count() else {
            return Vec::new();
        };
        let mut keys = Vec::with_capacity(count as usize);
        for i in 0..count {
            if let Some(k) = self.key_by_index(i) {
                keys.push(k);
            }
        }
        keys
    }
}

impl Drop for Smc {
    fn drop(&mut self) {
        unsafe { IOServiceClose(self.conn) };
    }
}

/// 解码温度（℃）。类型不认识 = None。
fn decode_temp(bytes: &[u8; 32], dtype: u32, size: usize) -> Option<f64> {
    if dtype == TYPE_FLT && size >= 4 {
        let v = f32::from_le_bytes(bytes[0..4].try_into().ok()?) as f64;
        return Some(v);
    }
    if dtype == TYPE_SP78 && size >= 2 {
        let v = i16::from_be_bytes([bytes[0], bytes[1]]) as f64 / 256.0;
        return Some(v);
    }
    None
}

/// 读 CPU 温度：一次性枚举本机温度键（进程级缓存）后读中位数。
/// 键优先级：`Tp*`（P 核近端）> 全部 T 键 > 硬编码候选表（枚举失败的回退）。
/// 全部失败 → None → 前端整卡不渲染。
pub fn cpu_temp_median() -> Option<f64> {
    static PROBED: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    let probed = PROBED.get_or_init(cpu_temp_keys_probe);
    let keys: Vec<&str> = if probed.iter().any(|k| k.starts_with("Tp")) {
        probed
            .iter()
            .filter(|k| k.starts_with("Tp"))
            .map(|s| s.as_str())
            .collect()
    } else if !probed.is_empty() {
        probed.iter().map(|s| s.as_str()).collect()
    } else {
        TEMP_KEYS.to_vec()
    };
    if keys.is_empty() {
        return None;
    }
    cpu_temp_median_with_keys(&keys)
}

/// 用给定键集读 CPU 温度。键集可以是硬编码候选表（TEMP_KEYS），也可以是
/// 一次性枚举出来的本机 T 键（见 [`cpu_temp_keys_probe`]）。
pub fn cpu_temp_median_with_keys(keys: &[&str]) -> Option<f64> {
    let mut smc = Smc::open()?;
    let mut vals = Vec::new();
    for key in keys {
        if let Some((bytes, dtype, size)) = smc.read(key) {
            if let Some(t) = decode_temp(&bytes, dtype, size) {
                if (TEMP_MIN..=TEMP_MAX).contains(&t) {
                    vals.push(t);
                }
            }
        }
    }
    if vals.is_empty() {
        return None;
    }
    vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    Some(vals[vals.len() / 2])
}

/// 一次性枚举：把本机所有「温度类」T 键找出来（类型 flt / sp78、读数落在
/// 合理区间）。缓存给采样器终身使用——SMC 键集随系统运行不变。
/// 枚举失败返回空表（不可用）。
pub fn cpu_temp_keys_probe() -> Vec<String> {
    let mut smc = match Smc::open() {
        Some(s) => s,
        None => return Vec::new(),
    };
    let mut out = Vec::new();
    for key in smc.all_keys() {
        // 只看 T 开头的键（温度族），过滤掉枚举噪音
        if !key.starts_with('T') || key.len() != 4 {
            continue;
        }
        if let Some((bytes, dtype, size)) = smc.read(&key) {
            if let Some(t) = decode_temp(&bytes, dtype, size) {
                if (TEMP_MIN..=TEMP_MAX).contains(&t) {
                    out.push(key);
                }
            }
        }
    }
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::{cpu_temp_keys_probe, cpu_temp_median_with_keys};

    /// 真机 spike（验收 §7 阶段三）：枚举本机温度键并用它们读出中位数。
    /// 虚拟机 / 无 SMC 机器上允许 None（标注 #[ignore] 手动跑）。
    #[test]
    #[ignore]
    fn spike_reads_cpu_temp_on_real_hardware() {
        let keys = cpu_temp_keys_probe();
        eprintln!("probed temp keys = {keys:?}");
        assert!(
            !keys.is_empty(),
            "no temperature keys found on this machine"
        );
        let key_refs: Vec<&str> = keys.iter().map(|s| s.as_str()).collect();
        let t = cpu_temp_median_with_keys(&key_refs);
        eprintln!("cpu temp = {t:?}");
        assert!(t.is_some_and(|v| (20.0..110.0).contains(&v)));
    }
}
