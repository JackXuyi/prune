//! 系统级内存读数：`vm_stat` 页数换算、内存压力等级、物理内存总量。
//!
//! **页面大小必须从 `vm_stat` 头部 / `hw.pagesize` 读取**——本机是 16384 B，
//! 写死 4096 会让所有换算差 4 倍（docs/m2-memory-plan.md §2）。

use std::process::Command;

use super::{Pressure, PressureLevel};

/// `vm_stat` 解析结果（全部为原始页数/计数，换算由方法负责）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SystemMem {
    /// 页大小（字节），来自 vm_stat 头部
    pub page_size: u64,
    /// Pages free
    pub pages_free: u64,
    /// Pages inactive（可回收缓存，不算已用）
    pub pages_inactive: u64,
    /// Pages speculative（预取缓存，不算已用）
    pub pages_speculative: u64,
    /// 压缩内存占用的物理页（"Pages occupied by compressor"，旧系统为 "Pages stored in compressor"）
    pub pages_compressor: u64,
    pub swapins_total: u64,
    pub swapouts_total: u64,
    pub pageins_total: u64,
    pub pageouts_total: u64,
}

impl SystemMem {
    pub fn compressor_bytes(&self) -> u64 {
        self.pages_compressor.saturating_mul(self.page_size)
    }

    pub fn pages_free_bytes(&self) -> u64 {
        self.pages_free.saturating_mul(self.page_size)
    }

    /// 物理内存「已用」（M4/S1）：总量 − 空闲 − 非活跃缓存 − 预取缓存。
    ///
    /// 与活动监视器 Memory Used（App + Wired + Compressed）同类近似：
    /// inactive / speculative 属于随时可回收的缓存，不应计入「已用」。
    /// vm_stat 拿不到干净的分桶时（字段缺失）按 0 处理，结果只可能偏高，不会虚低。
    pub fn used_bytes(&self, physical_total: u64) -> u64 {
        let cache = self
            .pages_free_bytes()
            .saturating_add(self.pages_inactive.saturating_mul(self.page_size))
            .saturating_add(self.pages_speculative.saturating_mul(self.page_size));
        physical_total.saturating_sub(cache)
    }
}

/// 读取并解析一次 `vm_stat`
pub fn read() -> Result<SystemMem, String> {
    let out = Command::new("/usr/bin/vm_stat")
        .output()
        .map_err(|e| format!("执行 vm_stat 失败: {e}"))?;
    if !out.status.success() {
        return Err("vm_stat 返回非零退出码".to_string());
    }
    let text = String::from_utf8_lossy(&out.stdout);
    parse_vm_stat(&text)
}

/// 解析 `vm_stat` 输出（纯函数，便于单测）
pub fn parse_vm_stat(text: &str) -> Result<SystemMem, String> {
    let page_size = text
        .lines()
        .next()
        .and_then(parse_page_size)
        .ok_or_else(|| "vm_stat 头部缺少 page size".to_string())?;

    let mut map: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
    for line in text.lines().skip(1) {
        let Some((k, v)) = line.rsplit_once(':') else {
            continue;
        };
        let key = k.trim().trim_matches('"').trim().to_string();
        let val = v
            .trim()
            .trim_end_matches('.')
            .replace(',', "")
            .trim()
            .to_string();
        if let Ok(n) = val.parse::<u64>() {
            map.insert(key, n);
        }
    }

    // 新旧系统 key 不同：新系统两个都有，优先取「occupied」（真实占用物理页）
    let compressor = map
        .get("Pages occupied by compressor")
        .or_else(|| map.get("Pages stored in compressor"))
        .copied()
        .unwrap_or(0);

    Ok(SystemMem {
        page_size,
        pages_free: map.get("Pages free").copied().unwrap_or(0),
        pages_inactive: map.get("Pages inactive").copied().unwrap_or(0),
        pages_speculative: map.get("Pages speculative").copied().unwrap_or(0),
        pages_compressor: compressor,
        swapins_total: map.get("Swapins").copied().unwrap_or(0),
        swapouts_total: map.get("Swapouts").copied().unwrap_or(0),
        pageins_total: map.get("Pageins").copied().unwrap_or(0),
        pageouts_total: map.get("Pageouts").copied().unwrap_or(0),
    })
}

/// `Mach Virtual Memory Statistics: (page size of 16384 bytes)` → 16384
fn parse_page_size(line: &str) -> Option<u64> {
    let idx = line.find("page size of")? + "page size of".len();
    let digits: String = line[idx..]
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse().ok()
}

/// 读取内核内存压力等级（`kern.memorystatus_vm_pressure_level`）
pub fn read_pressure() -> Pressure {
    match sysctl("kern.memorystatus_vm_pressure_level")
        .and_then(|s| s.trim().parse::<i32>().ok())
    {
        Some(raw) => Pressure {
            level: map_pressure_level(raw),
            raw,
        },
        None => Pressure::unknown(),
    }
}

/// D-M3：macOS 常量 1=NORMAL / 2=WARNING / 4=CRITICAL。
/// 语义阈值仍需 T-210 与活动监视器校准，因此原始值一并透出。
pub fn map_pressure_level(raw: i32) -> PressureLevel {
    match raw {
        1 => PressureLevel::Normal,
        2 => PressureLevel::Warning,
        4 => PressureLevel::Critical,
        // 0 或其它未知值：不谎报警告
        _ => PressureLevel::Normal,
    }
}

/// 物理内存总量（hw.memsize，字节）
pub fn physical_total() -> u64 {
    sysctl("hw.memsize")
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(0)
}

/// `hw.pagesize`，用于交叉校验 vm_stat 头部（不一致时以 vm_stat 为准）
pub fn hardware_page_size() -> Option<u64> {
    sysctl("hw.pagesize").and_then(|s| s.trim().parse::<u64>().ok())
}

fn sysctl(name: &str) -> Option<String> {
    let out = Command::new("/usr/sbin/sysctl").arg("-n").arg(name).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE_16K: &str = r#"Mach Virtual Memory Statistics: (page size of 16384 bytes)
Pages free:                                3789.
Pages active:                            211348.
Pages inactive:                          187659.
Pages speculative:                         1210.
Pages throttled:                              0.
Pages wired down:                        234746.
"Translation faults":                3845710283.
File-backed pages:                       107237.
Anonymous pages:                         292980.
Pages stored in compressor:             1917419.
Pages occupied by compressor:            401443.
Pageins:                               78456721.
Pageouts:                               3365569.
Swapins:                               43542528.
Swapouts:                              50939260.
"#;

    #[test]
    fn parses_page_size_and_counts() {
        let m = parse_vm_stat(FIXTURE_16K).unwrap();
        assert_eq!(m.page_size, 16384);
        assert_eq!(m.pages_free, 3789);
        assert_eq!(m.pages_inactive, 187659);
        assert_eq!(m.pages_speculative, 1210);
        // 两个 compressor key 并存时取 occupied
        assert_eq!(m.pages_compressor, 401443);
        assert_eq!(m.compressor_bytes(), 401_443 * 16_384);
        assert_eq!(m.pages_free_bytes(), 3789 * 16_384);
        assert_eq!(m.swapins_total, 43_542_528);
        assert_eq!(m.swapouts_total, 50_939_260);
    }

    #[test]
    fn used_bytes_excludes_reclaimable_cache() {
        let m = parse_vm_stat(FIXTURE_16K).unwrap();
        // fixture 来自 16 GB 机器：total − free − inactive − speculative
        let total: u64 = 16 * 1024 * 1024 * 1024;
        let used = m.used_bytes(total);
        let cache = (3789 + 187659 + 1210) as u64 * 16_384;
        assert_eq!(used, total - cache);
        assert!(used > total * 3 / 4, "正常负载下已用应占大头");
    }

    #[test]
    fn uses_pagesize_from_header_not_hardcoded_4096() {
        let fixture = FIXTURE_16K.replace("16384", "4096");
        let m = parse_vm_stat(&fixture).unwrap();
        assert_eq!(m.page_size, 4096);
        assert_eq!(m.compressor_bytes(), 401_443 * 4096);
    }

    #[test]
    fn falls_back_to_stored_in_compressor_on_old_macos() {
        let fixture = FIXTURE_16K.replace("Pages occupied by compressor:            401443.\n", "");
        let m = parse_vm_stat(&fixture).unwrap();
        assert_eq!(m.pages_compressor, 1_917_419);
    }

    #[test]
    fn missing_header_page_size_is_error() {
        assert!(parse_vm_stat("Pages free: 1.").is_err());
    }

    #[test]
    fn pressure_levels_map_per_macos_constants() {
        assert_eq!(map_pressure_level(1), PressureLevel::Normal);
        assert_eq!(map_pressure_level(2), PressureLevel::Warning);
        assert_eq!(map_pressure_level(4), PressureLevel::Critical);
        assert_eq!(map_pressure_level(0), PressureLevel::Normal);
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn reads_live_values() {
        let m = read().expect("本机 vm_stat 应可读");
        assert!(m.page_size >= 4096);
        assert!(physical_total() > 0);
        assert!(hardware_page_size().is_some());
    }
}
