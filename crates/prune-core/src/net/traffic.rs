//! 网络流量口径（M14）：网卡分类、本地日期、每日流量与总网速数据契约。
//!
//! 为什么单独成模块：这些都是**纯数据 + 纯函数**，采样器（`src-tauri`）只负责
//! 把字节喂进来。CLI 之后要复用同一套口径（`prune net today` 之类），
//! 口径一旦在 GUI 里长歪，第二个消费方必然跟着歪。
//!
//! 三条「不撒谎」的规则直接写进类型：
//! 1. [`IfaceKind`] 决定「算不算主数字」——开 VPN 时 `utun3` 与 `en0` 统计的是
//!    同一批字节（一份明文、一份密文），相加等于双计（方案 §3.2）。
//! 2. [`NetTotalSample::today`] 是 `Option`：`None` = 「没落库 / 库打不开」，
//!    **不是**「今天用了 0」。`DayTraffic { 0, 0 }` 才是「今天没流量」（§3.5）。
//! 3. [`NetTotalStatus`] 三态：`warmup` 不给 0，`unavailable` 不给编造数字（§3.3）。

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------- 网卡分类

/// 网卡类型。只有 [`IfaceKind::Physical`] 计入主数字。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IfaceKind {
    /// 真实物理网卡（en0 / Ethernet / WLAN…）。未知名称默认落这里：宁可多算真实物理口
    Physical,
    /// VPN / 隧道（utun / tun / tap / wg…）——与物理口重复计同一批字节
    Tunnel,
    /// 桥接 / 虚拟机 / 隔空投送（bridge / awdl / vmenet / vEthernet…）
    Virtual,
    /// 回环。既不计入，也不展示
    Loopback,
}

impl IfaceKind {
    /// 是否计入主读数与今日流量
    pub fn counted(self) -> bool {
        matches!(self, IfaceKind::Physical)
    }

    /// 是否在「网卡」列表里展示（回环完全不显示，避免 `lo0` 常驻一行 0）
    pub fn visible(self) -> bool {
        !matches!(self, IfaceKind::Loopback)
    }
}

/// 隧道类前缀（小写匹配）
const TUNNEL_PREFIXES: &[&str] = &["utun", "tun", "tap", "wg", "ipsec", "ppp"];
/// 虚拟类前缀（小写匹配）。`ap*` 是 macOS 的个人热点/隔空播放，不是物理口
const VIRTUAL_PREFIXES: &[&str] = &[
    "bridge",
    "awdl",
    "llw",
    "ap",
    "anpi",
    "vmenet",
    "veth",
    "docker",
    "vethernet",
    "vmnet",
];

/// 按名称判定网卡类型（纯函数）。
///
/// 判定只依赖名称前缀，因为 `sysinfo` 不暴露「这张卡是不是物理的」。
/// 已知局限：无法从名称/MAC 判定「哪块是主网卡」，所以 UI 只按速率排序，
/// 不做「主网卡」结论。未知名称一律当物理口——少算真实流量比多算更糟。
pub fn classify_iface(name: &str) -> IfaceKind {
    let n = name.trim().to_ascii_lowercase();
    if n.is_empty() {
        return IfaceKind::Physical;
    }
    // 回环：macOS/Linux 的 lo0 / lo，Windows 的 "Loopback Pseudo-Interface 1"
    if n == "lo" || n == "lo0" || n.starts_with("loopback") {
        return IfaceKind::Loopback;
    }
    if TUNNEL_PREFIXES.iter().any(|p| n.starts_with(p)) {
        return IfaceKind::Tunnel;
    }
    if VIRTUAL_PREFIXES.iter().any(|p| n.starts_with(p)) {
        return IfaceKind::Virtual;
    }
    IfaceKind::Physical
}

// ---------------------------------------------------------------- 每日流量

/// 一天的网络流量（本地日期归集，仅物理口，仅 Prune 运行期间）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DayTraffic {
    /// 本地日期 `YYYY-MM-DD`（字典序即时间序，SQL 里能直接比较）
    pub day: String,
    pub down_bytes: u64,
    pub up_bytes: u64,
}

impl DayTraffic {
    pub fn zero(day: impl Into<String>) -> Self {
        Self {
            day: day.into(),
            down_bytes: 0,
            up_bytes: 0,
        }
    }

    /// 累加一批增量
    pub fn add(&mut self, down: u64, up: u64) {
        self.down_bytes = self.down_bytes.saturating_add(down);
        self.up_bytes = self.up_bytes.saturating_add(up);
    }

    pub fn total_bytes(&self) -> u64 {
        self.down_bytes.saturating_add(self.up_bytes)
    }
}

// ---------------------------------------------------------------- 采样契约

/// 采样状态。三者必须能被 UI 严格区分（方案 §3.3）：
/// - `Ok`：有基线且读到接口，本拍为 0 时显示 `0 B/s`
/// - `Warmup`：首拍 / 刚从不可用恢复，还没有基线 → 「采样中…」，**不显示 0**
/// - `Unavailable`：连续读不到 → 「不可读：{note}」，整页不出现编造的 0
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NetTotalStatus {
    Ok,
    Warmup,
    Unavailable,
}

/// 单张网卡的读数。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IfaceRate {
    pub name: String,
    pub kind: IfaceKind,
    pub down_bps: f64,
    pub up_bps: f64,
    /// 本次会话（自 Prune 启动）该网卡的正增量累计
    pub session_down_bytes: u64,
    pub session_up_bytes: u64,
}

impl IfaceRate {
    /// 排序与占比条用的合计速率
    pub fn total_bps(&self) -> f64 {
        self.down_bps + self.up_bps
    }
}

/// 走势里的一点（1 s 一点，900 点环形缓冲 ≈ 15 min）
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetTotalPoint {
    pub at: i64,
    pub down_bps: f64,
    pub up_bps: f64,
}

/// 一次系统级网络采样的完整产出。serde camelCase ↔ `src/types/net.ts`。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetTotalSample {
    pub at: i64,
    pub status: NetTotalStatus,
    /// 非空 = 必须显示给用户的原因（不可读 / 今日流量未落库）
    pub note: Option<String>,
    /// 物理口合计
    pub down_bps: f64,
    pub up_bps: f64,
    /// 上一拍的真实间隔（不是标称间隔）
    pub interval_ms: u64,
    /// 物理口在前、虚拟口在后，各自按速率降序
    pub ifaces: Vec<IfaceRate>,
    pub session_down_bytes: u64,
    pub session_up_bytes: u64,
    /// 峰值固定为「本次会话最大值」，与走势窗口解耦
    pub peak_down_bps: f64,
    pub peak_up_bps: f64,
    pub peak_at: i64,
    /// 今日（Prune 运行期间累计）。None = 未落库 / 库不可用 —— **不是 0**
    pub today: Option<DayTraffic>,
    /// 上次成功落库时刻（0 = 还没落过）
    pub today_persisted_at: i64,
}

// ---------------------------------------------------------------- 本地日期

/// Unix 秒 → 本地日期 `YYYY-MM-DD`。
///
/// 实现委托 [`crate::localtime::day_string`]（那里是唯一的平台 FFI 落点，
/// 与 swap 的「今日新增」共用同一份口径），本模块不再单独引 `libc` /
/// `windows-sys`，也**不引 chrono**。
///
/// 时区是**进程启动时的本地时区**；跨时区旅行可能把一天切成两段，属于
/// 已知限制（方案 §3.5），不做补偿。
pub fn local_day(ts: i64) -> String {
    crate::localtime::day_string(ts)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_day_format(s: &str) -> bool {
        let parts: Vec<&str> = s.split('-').collect();
        if parts.len() != 3 || parts[0].len() != 4 || parts[1].len() != 2 || parts[2].len() != 2 {
            return false;
        }
        if parts.iter().any(|p| !p.chars().all(|c| c.is_ascii_digit())) {
            return false;
        }
        let month: u32 = parts[1].parse().unwrap_or(0);
        let day: u32 = parts[2].parse().unwrap_or(0);
        (1..=12).contains(&month) && (1..=31).contains(&day)
    }

    /// 只断言**格式**，不硬编码时区期望：CI 跑在不同时区上，写死 `2026-11-05`
    /// 会让测试在 UTC-8 的机器上红一次（这类测试没人愿意修第二次）
    #[test]
    fn local_day_has_expected_format() {
        let now = 1_762_343_000; // 2025-11-05 前后
        let d = local_day(now);
        assert!(is_day_format(&d), "格式不对：{d}");
        assert_eq!(local_day(0), local_day(0), "同一秒必须稳定");
        assert!(is_day_format(&local_day(0)));
    }

    /// 一天的跨度必须换一天（地球上没有 ±24h 的时区，这条与时区无关）
    #[test]
    fn local_day_advances_by_one_day() {
        let now = 1_762_343_000;
        assert_ne!(local_day(now), local_day(now + 86_400));
        assert_ne!(local_day(now), local_day(now - 86_400));
    }

    #[test]
    fn classify_macos_names() {
        assert_eq!(classify_iface("en0"), IfaceKind::Physical);
        assert_eq!(classify_iface("en5"), IfaceKind::Physical);
        assert_eq!(classify_iface("lo0"), IfaceKind::Loopback);
        assert_eq!(classify_iface("utun3"), IfaceKind::Tunnel);
        assert_eq!(classify_iface("awdl0"), IfaceKind::Virtual);
        // 个人热点：不是物理口，不能算进主数字
        assert_eq!(classify_iface("ap1"), IfaceKind::Virtual);
        assert_eq!(classify_iface("bridge0"), IfaceKind::Virtual);
        assert_eq!(classify_iface("anpi0"), IfaceKind::Virtual);
        assert_eq!(classify_iface("llw0"), IfaceKind::Virtual);
    }

    #[test]
    fn classify_windows_and_linux_names() {
        assert_eq!(classify_iface("Ethernet"), IfaceKind::Physical);
        assert_eq!(classify_iface("WLAN"), IfaceKind::Physical);
        assert_eq!(classify_iface("vEthernet (WSL)"), IfaceKind::Virtual);
        assert_eq!(
            classify_iface("Loopback Pseudo-Interface 1"),
            IfaceKind::Loopback
        );
        assert_eq!(classify_iface("lo"), IfaceKind::Loopback);
        assert_eq!(classify_iface("docker0"), IfaceKind::Virtual);
        assert_eq!(classify_iface("wg0"), IfaceKind::Tunnel);
        // 未知名称宁可多算（少算真实流量比多算更糟）
        assert_eq!(classify_iface("以太网 2"), IfaceKind::Physical);
        assert_eq!(classify_iface(""), IfaceKind::Physical);
    }

    #[test]
    fn only_physical_counts_and_loopback_is_hidden() {
        assert!(IfaceKind::Physical.counted());
        assert!(!IfaceKind::Tunnel.counted());
        assert!(!IfaceKind::Virtual.counted());
        assert!(!IfaceKind::Loopback.counted());

        assert!(IfaceKind::Physical.visible());
        assert!(
            IfaceKind::Tunnel.visible(),
            "虚拟口要能在折叠区看到，不能丢信息"
        );
        assert!(!IfaceKind::Loopback.visible());
    }

    #[test]
    fn day_traffic_accumulates_without_overflow() {
        let mut d = DayTraffic::zero("2026-11-05");
        d.add(100, 10);
        d.add(1, 2);
        assert_eq!((d.down_bytes, d.up_bytes), (101, 12));
        assert_eq!(d.total_bytes(), 113);

        let mut big = DayTraffic::zero("2026-11-05");
        big.add(u64::MAX, 0);
        big.add(1, 0);
        assert_eq!(big.down_bytes, u64::MAX, "脏数据不能 panic（saturating）");
    }

    #[test]
    fn iface_rate_total_is_sum() {
        let r = IfaceRate {
            name: "en0".into(),
            kind: IfaceKind::Physical,
            down_bps: 1024.0,
            up_bps: 24.0,
            session_down_bytes: 0,
            session_up_bytes: 0,
        };
        assert_eq!(r.total_bps(), 1048.0);
    }
}
