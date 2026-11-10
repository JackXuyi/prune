//! 局域网设备（M18，docs/m18-lan-devices-spec.md）。
//!
//! 定位是「本机暴露面 × 同网段设备」，不是网络扫描器（边界见 plan §1）。
//!
//! 模块分层（spec §4/§5）：
//! - 纯函数：解析 / 展开 / 合并判定，全部可单测，fixture 用本机实测输出
//! - 平台口（C2 接入，`#[cfg]` 分叉）：`local_scope()` / `neighbors()` / `established_peers()`
//! - 服务发现（C4 接入）：mDNS（`mdns-sd`）+ SSDP（手写 M-SEARCH）
//!
//! 口径纪律（与 `mod.rs` 同级，「不撒谎」条款）：
//! 1. `mac = None` 是「邻居表里没有」，不是「没有 MAC」
//! 2. `vendor = None` 是「查不到」，不显示「未知厂商」
//! 3. 随机化 MAC（locally administered）不查 OUI——查出来也是错的
//! 4. 在线判定保留内核原始六态，不压成 bool
//!
//! C1 范围：数据结构 + 纯函数 + 单测。无 IO、无平台代码、无外部依赖。

use std::net::Ipv4Addr;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------- 枚举

/// 邻居状态。源：macOS arp 标记 / Windows `NL_NEIGHBOR_STATE`。
///
/// 保留原始六态而不是压成 bool——「内核视角」与「应用视角」不能合并（口径纪律 4）。
/// macOS 的 arp 表只有 `incomplete` / `permanent` 两个标记，其余一律归 [`NeighborState::Reachable`]，
/// **不要伪造 Stale/Delay/Probe**。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NeighborState {
    Reachable,
    Stale,
    Delay,
    Probe,
    Incomplete,
    Permanent,
    /// 平台没给状态（macOS 只有 incomplete / permanent 两标记，其余不归此——见上）。
    /// 当前双平台都不会产出 Unknown，保留给未来平台。
    Unknown,
}

/// 设备类型。由服务类型 + 厂商证据推断（[`kind_from_evidence`]），推断不出就是
/// [`DeviceKind::Unknown`]——**错判比不判更糟**，不做 TTL / 端口组合的 OS 指纹。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeviceKind {
    Router,
    Computer,
    Phone,
    Tablet,
    Tv,
    Speaker,
    Printer,
    Nas,
    Camera,
    Iot,
    Unknown,
}

// ---------------------------------------------------------------- 数据结构

/// 邻居表原始行（平台无关）。macOS 来自 `arp -an` 解析，Windows 来自 `GetIpNetTable2`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawNeighbor {
    pub ip: Ipv4Addr,
    /// `None` = ARP 未解析（incomplete）或该行无 MAC。不是「没有 MAC」（口径纪律 1）
    pub mac: Option<[u8; 6]>,
    pub iface: Option<String>,
    pub state: NeighborState,
}

/// 参与扫描的网段（有默认路由的那张网卡）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanScope {
    pub iface: String,
    pub addr: Ipv4Addr,
    pub prefix: u8,
}

/// 一台局域网设备。三层数据源（邻居表 / 探测 / 服务发现）在 [`merge`] 里合成。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LanDevice {
    pub ip: String,
    /// 大写冒号分隔 `"BC:24:11:4C:D8:39"`；`None` = 邻居表无此条
    pub mac: Option<String>,
    /// OUI 查表结果；`None` = 查不到（含随机化 MAC），**不猜**（口径纪律 2）
    pub vendor: Option<String>,
    /// mDNS / SSDP 得到的名字；`None` = 未拿到，不拼 `Device-192.168.0.44`
    pub hostname: Option<String>,
    pub kind: DeviceKind,
    pub state: NeighborState,
    /// 内核视角或 TCP 探测任一认定为活
    pub online: bool,
    /// 服务类型字符串（mDNS 原样 + SSDP ST 归一化为 `ssdp:<ST>`），去重升序
    pub services: Vec<String>,
    /// 该设备正在连的我方端口，升序
    pub inbound: Vec<u16>,
    pub is_gateway: bool,
    pub is_self: bool,
    /// 随机化 MAC（locally administered）→ UI 不展示厂商列，也不查 OUI
    pub randomized_mac: bool,
}

/// 一轮局域网扫描的完整产出。serde camelCase ↔ `src/types/lan.ts`。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LanSnapshot {
    pub at: i64,
    /// 参与扫描的网卡（有默认路由那张）；多网卡时只取一张，其余进 `notes`
    pub iface: Option<String>,
    /// `"192.168.0.0/23"`。本机实测是 /23（netmask 0xfffffe00），**不是 /24**
    pub cidr: Option<String>,
    /// 已探测地址数
    pub scanned: u32,
    /// 上限内的目标总数（cap 1024）。`scanned < total` 时 UI 必须显示 `N/M`，
    /// 不能静默截断——cap 截断会造成「全网就这些设备」的错觉
    pub total: u32,
    pub devices: Vec<LanDevice>,
    /// 本机对同网段可达的监听端口（复用 ports.rs 的 `BindScope::Any`，不重算）
    pub exposed_ports: Vec<u16>,
    /// 必须显示给用户的原因（授权被拒 / 防火墙疑似拦截 / 多网卡只取一张 / cap 截断）
    pub notes: Vec<String>,
    pub scan_ms: u64,
}

impl LanSnapshot {
    /// 无数据但必须可解释的空快照（授权被拒 / 无默认路由 / 扫描失败）。
    /// 对齐 [`crate::net::ports::PortSnapshot::unavailable`] 的既有写法。
    pub fn unavailable(note: &str) -> Self {
        Self {
            at: 0,
            iface: None,
            cidr: None,
            scanned: 0,
            total: 0,
            devices: Vec::new(),
            exposed_ports: Vec::new(),
            notes: vec![note.to_string()],
            scan_ms: 0,
        }
    }
}

// ---------------------------------------------------------------- 纯函数：解析

/// macOS `arp -an` 解析（`-n` 不带 DNS 反查，**不要用 `arp -a`**——慢且依赖网络）。
///
/// 实测样例（注意 MAC 可能是不补零的单十六进制位，`5` 是合法段）：
/// ```text
/// ? (192.168.0.44) at bc:24:11:4c:d8:39 on en0 ifscope [ethernet]
/// ? (192.168.0.116) at 8a:32:81:3a:5:5c on en0 ifscope [ethernet]
/// ? (192.168.0.50) at (incomplete) on en0 ifscope [ethernet]
/// ? (192.168.0.115) at 82:ac:9b:43:bd:28 on en0 ifscope permanent [ethernet]
/// ```
///
/// 解析不了的行跳过、IP 解析失败跳过——**不猜**，坏行不产生设备。
pub fn parse_arp_output(text: &str) -> Vec<RawNeighbor> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        let Some(lp) = line.find('(') else { continue };
        let Some(r_off) = line[lp..].find(')') else {
            continue;
        };
        let Ok(ip) = line[lp + 1..lp + r_off].trim().parse::<Ipv4Addr>() else {
            continue;
        };

        let after_ip = &line[lp + r_off + 1..];
        let Some(at_off) = after_ip.find("at ") else {
            continue;
        };
        let rest = &after_ip[at_off + 3..];
        let (mac_str, iface_part) = match rest.find(" on ") {
            Some(i) => (&rest[..i], &rest[i + 4..]),
            None => (rest, ""),
        };
        let mac_str = mac_str.trim();
        let iface = iface_part.split_whitespace().next().map(str::to_string);

        if mac_str == "(incomplete)" {
            out.push(RawNeighbor {
                ip,
                mac: None,
                iface,
                state: NeighborState::Incomplete,
            });
            continue;
        }

        let Some(bytes) = parse_mac_str(mac_str) else {
            continue;
        };
        let state = if iface_part.contains("permanent") {
            NeighborState::Permanent
        } else {
            NeighborState::Reachable
        };
        out.push(RawNeighbor {
            ip,
            mac: Some(bytes),
            iface,
            state,
        });
    }
    out
}

/// MAC 字符串 → 字节。接受不补零的单十六进制位（`8a:32:81:3a:5:5c` 是实测真实输出）。
fn parse_mac_str(s: &str) -> Option<[u8; 6]> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 6 {
        return None;
    }
    let mut bytes = [0u8; 6];
    for (i, p) in parts.iter().enumerate() {
        if p.is_empty() || p.len() > 2 {
            return None;
        }
        bytes[i] = u8::from_str_radix(p, 16).ok()?;
    }
    Some(bytes)
}

/// 十六进制 netmask（`"0xfffffe00"`）→ 前缀长度。
///
/// 只接受「全 1 后跟全 0」的连续掩码；`0xff00ff00` 这类洞洞返回 `None`——
/// CIDR 表达不了它，硬转前缀会得出错误网段（**不猜**）。
/// `0x00000000`（/0，无网段语义）同样 `None`。
pub fn prefix_from_netmask_hex(hex: &str) -> Option<u8> {
    let v = u32::from_str_radix(
        hex.trim().trim_start_matches("0x").trim_start_matches("0X"),
        16,
    )
    .ok()?;
    // 连续掩码 ⇔ 取反后 +1 是 2 的幂（或本来就是 0，即全 1 掩码 /32）
    // wrapping_add：0x00000000 的取反是 u32::MAX，普通加法会溢出（这正是要返回 None 的输入）
    let inv = !v;
    let contiguous = inv == 0 || (inv.wrapping_add(1)).is_power_of_two();
    if !contiguous {
        return None;
    }
    Some(v.count_ones() as u8)
}

/// macOS `ifconfig <iface>` 输出 → `(本机地址, 前缀长度)`。
///
/// 输入形如（含 tab 缩进）：
/// ```text
///     inet 192.168.0.115 netmask 0xfffffe00 broadcast 192.168.1.255
/// ```
/// `inet6` 行天然跳过（`strip_prefix("inet ")` 匹配不上 `inet6 `）。
pub fn parse_ifconfig_inet(text: &str) -> Option<(Ipv4Addr, u8)> {
    for line in text.lines() {
        let Some(rest) = line.trim_start().strip_prefix("inet ") else {
            continue;
        };
        let mut toks = rest.split_whitespace();
        let ip = toks.next()?.parse::<Ipv4Addr>().ok()?;
        let mut hex = None;
        while let Some(t) = toks.next() {
            if t == "netmask" {
                hex = toks.next();
                break;
            }
        }
        let prefix = prefix_from_netmask_hex(hex?)?;
        return Some((ip, prefix));
    }
    None
}

// ---------------------------------------------------------------- 纯函数：网段与探测

/// CIDR 展开可用主机地址（按前缀排除网络地址与广播地址），返回 `(地址列表, 目标总数)`。
///
/// - 超过 `cap` 时截断到前 `cap` 个，`total` 仍是完整数——调用方据此显示 `N/M`
/// - `/31` 按 RFC 3021 两端都可用；`/32` 只有该地址本身
/// - 内部按 `net & mask` 归一化，调用方传本机地址（而非网络地址）也能得到正确结果
pub fn expand_cidr(net: Ipv4Addr, prefix: u8, cap: usize) -> (Vec<Ipv4Addr>, u32) {
    if prefix > 32 || cap == 0 {
        return (Vec::new(), 0);
    }
    let mask: u32 = if prefix == 0 {
        0
    } else {
        u32::MAX << (32 - prefix as u32)
    };
    let network = u32::from(net) & mask;
    let broadcast = network | !mask;

    let total: u64 = match prefix {
        32 => 1,
        31 => 2,
        p => (1u64 << (32 - p as u32)) - 2,
    };

    let mut out = Vec::with_capacity(total.min(cap as u64) as usize);
    let mut cur = network;
    loop {
        let edge = cur == network || cur == broadcast;
        let take = match prefix {
            31 | 32 => true,
            _ => !edge,
        };
        if take {
            out.push(Ipv4Addr::from(cur));
            if out.len() >= cap {
                break;
            }
        }
        if prefix == 0 {
            if cur == u32::MAX {
                break;
            }
        } else if cur == broadcast {
            break;
        }
        cur = cur.wrapping_add(1);
    }
    (out, u32::try_from(total).unwrap_or(u32::MAX))
}

/// 该地址是否值得探测。排除保留段（0.x / 127 / 169.254 / 组播与广播 224–255）
/// 以及本网段的网络地址与广播地址——它们永远不是设备。
pub fn is_probe_candidate(ip: Ipv4Addr, net: Ipv4Addr, prefix: u8) -> bool {
    let o = ip.octets();
    if o[0] == 0 || o[0] == 127 || o[0] >= 224 || (o[0] == 169 && o[1] == 254) {
        return false;
    }
    if prefix > 32 {
        return true; // 非法前缀时只做保留段过滤，调用方（expand_cidr）已经拒绝过
    }
    let mask: u32 = if prefix == 0 {
        0
    } else {
        u32::MAX << (32 - prefix as u32)
    };
    let network = u32::from(net) & mask;
    let broadcast = network | !mask;
    let v = u32::from(ip);
    v != network && v != broadcast
}

// ---------------------------------------------------------------- 纯函数：MAC 与厂商

/// `[0x8a,0x32,0x81,0x3a,0x05,0x5c]` → `"8A:32:81:3A:05:5C"`（大写、补零）。
/// 按值取参（6 字节 Copy）：`neighbor.mac.map(format_mac)` 可直接用。
pub fn format_mac(bytes: [u8; 6]) -> String {
    let mut s = String::with_capacity(17);
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 {
            s.push(':');
        }
        s.push_str(&format!("{b:02X}"));
    }
    s
}

/// 第 1 字节 bit1 = locally administered（iPhone「私密 Wi-Fi 地址」/ Android 随机化）。
///
/// 本机实测 10 条邻居里 5 条是随机化——这是常态不是异常。随机 MAC 查 OUI 是错的，
/// 且同一设备跨网络换 MAC，**设备聚合键统一用 IP**（见 spec §9）。
pub fn is_randomized_mac(bytes: &[u8; 6]) -> bool {
    bytes[0] & 0x02 != 0
}

// ---------------------------------------------------------------- 纯函数：类型推断

/// 由证据推断设备类型。`services` 里的 SSDP 记录已归一化为 `ssdp:<ST>` 形式，
/// 所以 `ssdp:InternetGatewayDevice` / `ssdp:MediaRenderer` 直接 `contains` 命中。
///
/// 规则按 spec §8 优先级从上到下，命中即返回；全不命中 → [`DeviceKind::Unknown`]。
/// **刻意不做**：TTL 指纹、端口组合推 OS、厂商名推设备类型（Apple 既可能是手机也可能是 Mac）。
pub fn kind_from_evidence(
    services: &[String],
    hostname: Option<&str>,
    vendor: Option<&str>,
    is_gateway: bool,
) -> DeviceKind {
    let has = |needle: &str| services.iter().any(|s| s.contains(needle));

    if is_gateway || has("InternetGatewayDevice") {
        return DeviceKind::Router;
    }
    // 规则 2：AirPlay + 主机名含 appletv 才是电视；裸 AirPlay 多为音箱（规则 3）
    if has("_airplay") {
        let is_tv = hostname.is_some_and(|h| {
            let l = h.to_ascii_lowercase();
            l.contains("appletv") || l.contains("apple-tv")
        });
        return if is_tv {
            DeviceKind::Tv
        } else {
            DeviceKind::Speaker
        };
    }
    if has("_raop") {
        return DeviceKind::Speaker;
    }
    if has("_googlecast") || has("MediaRenderer") {
        return DeviceKind::Tv;
    }
    if has("_ipp") || has("_ipps") || has("_printer") || has("_pdl-datastream") || has("_scanner") {
        return DeviceKind::Printer;
    }
    if has("_smb") || has("_afpovertcp") {
        return DeviceKind::Nas;
    }
    if has("_hap") {
        return DeviceKind::Iot;
    }
    if has("_companion-link") {
        // 区分不了 iPhone / iPad，统一 Phone（UI 同一个图标）
        return DeviceKind::Phone;
    }
    if has("_workstation") || has("_ssh") {
        return DeviceKind::Computer;
    }
    if vendor.is_some_and(|v| {
        let l = v.to_ascii_lowercase();
        ["hikvision", "dahua", "axis", "reolink"]
            .iter()
            .any(|k| l.contains(k))
    }) {
        return DeviceKind::Camera;
    }
    DeviceKind::Unknown
}

// ---------------------------------------------------------------- 测试

#[cfg(test)]
mod tests {
    use super::*;

    /// 本机实测（2026-11-10，/23 网段）。注意 `8a:32:81:3a:5:5c` 的单十六进制位。
    const ARP_SAMPLE: &str = "\
? (192.168.0.44) at bc:24:11:4c:d8:39 on en0 ifscope [ethernet]
? (192.168.0.109) at f6:74:4d:e9:41:5e on en0 ifscope [ethernet]
? (192.168.0.110) at 52:cb:56:85:1c:ee on en0 ifscope [ethernet]
? (192.168.0.111) at 6e:a7:e6:68:64:bb on en0 ifscope [ethernet]
? (192.168.0.115) at 82:ac:9b:43:bd:28 on en0 ifscope permanent [ethernet]
? (192.168.0.116) at 8a:32:81:3a:5:5c on en0 ifscope [ethernet]
? (192.168.1.1) at e4:77:27:f0:12:56 on en0 ifscope [ethernet]
? (192.168.1.255) at ff:ff:ff:ff:ff:ff on en0 ifscope [ethernet]
? (224.0.0.251) at 1:0:5e:0:0:fb on en0 ifscope permanent [ethernet]
? (239.255.255.250) at 1:0:5e:7f:ff:fa on en0 ifscope permanent [ethernet]
";

    fn ip(s: &str) -> Ipv4Addr {
        s.parse().unwrap()
    }

    #[test]
    fn arp_parse_full() {
        let rows = parse_arp_output(ARP_SAMPLE);
        assert_eq!(rows.len(), 10);

        // 单十六进制位要补零：8a:32:81:3a:5:5c → 8A:32:81:3A:05:5C
        let r116 = rows.iter().find(|r| r.ip == ip("192.168.0.116")).unwrap();
        assert_eq!(
            r116.mac.map(format_mac).as_deref(),
            Some("8A:32:81:3A:05:5C")
        );
        assert_eq!(r116.state, NeighborState::Reachable);
        assert_eq!(r116.iface.as_deref(), Some("en0"));

        // permanent 标记落在 0.115（本机）与两条组播上
        let r115 = rows.iter().find(|r| r.ip == ip("192.168.0.115")).unwrap();
        assert_eq!(r115.state, NeighborState::Permanent);

        // 网关在跨 /24 边界的 192.168.1.1 —— /23 才扫得到它
        let gw = rows.iter().find(|r| r.ip == ip("192.168.1.1")).unwrap();
        assert_eq!(gw.mac.map(format_mac).as_deref(), Some("E4:77:27:F0:12:56"));
    }

    #[test]
    fn arp_parse_incomplete() {
        let rows = parse_arp_output("? (192.168.0.50) at (incomplete) on en0 ifscope [ethernet]\n");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].ip, ip("192.168.0.50"));
        assert_eq!(rows[0].mac, None);
        assert_eq!(rows[0].state, NeighborState::Incomplete);
    }

    #[test]
    fn arp_parse_permanent() {
        let rows = parse_arp_output(
            "? (224.0.0.251) at 1:0:5e:0:0:fb on en0 ifscope permanent [ethernet]\n",
        );
        assert_eq!(rows[0].state, NeighborState::Permanent);
        assert_eq!(
            rows[0].mac.map(format_mac).as_deref(),
            Some("01:00:5E:00:00:FB")
        );
    }

    #[test]
    fn arp_parse_garbage() {
        for bad in [
            "",
            "arp: bad option -- foo\nusage: arp [-an] ...\n",
            "no parens here\n",
        ] {
            assert!(parse_arp_output(bad).is_empty(), "bad input: {bad:?}");
        }
        // IP 段不是地址 → 整行跳过，不 panic、不产设备
        assert!(
            parse_arp_output("? (not-an-ip) at bc:24:11:4c:d8:39 on en0 ifscope [ethernet]\n")
                .is_empty()
        );
    }

    #[test]
    fn netmask_hex() {
        assert_eq!(prefix_from_netmask_hex("0xfffffe00"), Some(23));
        assert_eq!(prefix_from_netmask_hex("0xffffff00"), Some(24));
        assert_eq!(prefix_from_netmask_hex("0xffff0000"), Some(16));
        assert_eq!(prefix_from_netmask_hex("0xffffffff"), Some(32));
    }

    #[test]
    fn netmask_noncontiguous() {
        // 洞洞掩码：CIDR 表达不了，必须 None
        assert_eq!(prefix_from_netmask_hex("0xff00ff00"), None);
        // /0 没有网段语义
        assert_eq!(prefix_from_netmask_hex("0x00000000"), None);
        // 非十六进制
        assert_eq!(prefix_from_netmask_hex("banana"), None);
    }

    #[test]
    fn ifconfig_inet() {
        let out = "	en0: flags=8863<UP,BROADCAST,SMART,RUNNING,SIMPLEX,MULTICAST> mtu 1500\n\
                  	inet 192.168.0.115 netmask 0xfffffe00 broadcast 192.168.1.255\n\
                  	inet6 fe80::1234:5678:9abc:def0%en0 prefixlen 64 secured scopeid 0x5\n";
        let (addr, prefix) = parse_ifconfig_inet(out).unwrap();
        assert_eq!(addr, ip("192.168.0.115"));
        assert_eq!(prefix, 23);
    }

    #[test]
    fn ifconfig_inet_no_inet_row() {
        assert!(parse_ifconfig_inet("	en0: flags=8863 mtu 1500\n").is_none());
    }

    #[test]
    fn cidr_expand_23() {
        // 本机实测：netmask 0xfffffe00 = /23 = 510 个可用主机地址
        let (addrs, total) = expand_cidr(ip("192.168.0.0"), 23, 1024);
        assert_eq!(total, 510);
        assert_eq!(addrs.len(), 510);
        assert_eq!(addrs[0], ip("192.168.0.1"));
        assert_eq!(addrs[addrs.len() - 1], ip("192.168.1.254"));
        // 网络地址与广播地址不在列表里
        assert!(!addrs.contains(&ip("192.168.0.0")));
        assert!(!addrs.contains(&ip("192.168.1.255")));
    }

    #[test]
    fn cidr_expand_normalizes_host_addr() {
        // 传本机地址（而非网络地址）也归一化到同一网段——实测本机就是 192.168.0.115/23
        let (addrs, total) = expand_cidr(ip("192.168.0.115"), 23, 1024);
        assert_eq!(total, 510);
        assert_eq!(addrs.len(), 510);
        assert_eq!(addrs[0], ip("192.168.0.1"));
    }

    #[test]
    fn cidr_cap() {
        let (addrs, total) = expand_cidr(ip("10.0.0.0"), 16, 1024);
        assert_eq!(addrs.len(), 1024);
        assert_eq!(total, 65534); // total 不随 cap 缩水，UI 据此显示 N/M
        assert_eq!(addrs[0], ip("10.0.0.1"));
        // 第 1024 个主机 = 10.0.0.1 + 1023 = 10.0.4.0
        assert_eq!(addrs[1023], ip("10.0.4.0"));
    }

    #[test]
    fn cidr_edge_prefixes() {
        // /31：RFC 3021，两端都可用；/32：只有自己
        let (a31, t31) = expand_cidr(ip("192.168.0.4"), 31, 1024);
        assert_eq!((a31.len(), t31), (2, 2));
        assert_eq!(a31, vec![ip("192.168.0.4"), ip("192.168.0.5")]);
        let (a32, t32) = expand_cidr(ip("192.168.0.4"), 32, 1024);
        assert_eq!((a32.len(), t32), (1, 1));
        // cap=0 与非法前缀
        assert_eq!(expand_cidr(ip("10.0.0.0"), 24, 0), (Vec::new(), 0));
        assert_eq!(expand_cidr(ip("10.0.0.0"), 33, 100), (Vec::new(), 0));
    }

    #[test]
    fn probe_candidate() {
        let (net, prefix) = (ip("192.168.0.0"), 23);
        assert!(is_probe_candidate(ip("192.168.0.44"), net, prefix));
        assert!(is_probe_candidate(ip("192.168.1.1"), net, prefix)); // 跨 /24 边界的网关
                                                                     // 保留段
        assert!(!is_probe_candidate(ip("0.0.0.0"), net, prefix));
        assert!(!is_probe_candidate(ip("127.0.0.1"), net, prefix));
        assert!(!is_probe_candidate(ip("169.254.1.1"), net, prefix));
        assert!(!is_probe_candidate(ip("224.0.0.251"), net, prefix));
        assert!(!is_probe_candidate(ip("255.255.255.255"), net, prefix));
        // 本网段的网络地址与广播地址
        assert!(!is_probe_candidate(ip("192.168.0.0"), net, prefix));
        assert!(!is_probe_candidate(ip("192.168.1.255"), net, prefix));
    }

    #[test]
    fn mac_format_pad() {
        let bytes: [u8; 6] = [0x8a, 0x32, 0x81, 0x3a, 0x05, 0x5c];
        assert_eq!(format_mac(bytes), "8A:32:81:3A:05:5C");
        assert_eq!(
            format_mac([0xbc, 0x24, 0x11, 0x4c, 0xd8, 0x39]),
            "BC:24:11:4C:D8:39"
        );
    }

    #[test]
    fn mac_randomized() {
        // 本机实测：10 条里 5 条随机化
        assert!(is_randomized_mac(&[0xf6, 0x74, 0x4d, 0xe9, 0x41, 0x5e]));
        assert!(is_randomized_mac(&[0x52, 0xcb, 0x56, 0x85, 0x1c, 0xee]));
        assert!(is_randomized_mac(&[0x6e, 0xa7, 0xe6, 0x68, 0x64, 0xbb]));
        assert!(is_randomized_mac(&[0x8a, 0x32, 0x81, 0x3a, 0x05, 0x5c]));
        assert!(is_randomized_mac(&[0x82, 0xac, 0x9b, 0x43, 0xbd, 0x28]));
        // 全局 OUI（bit1 = 0）
        assert!(!is_randomized_mac(&[0xbc, 0x24, 0x11, 0x4c, 0xd8, 0x39]));
        assert!(!is_randomized_mac(&[0xe4, 0x77, 0x27, 0xf0, 0x12, 0x56]));
    }

    fn sv(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn kind_airplay_tv() {
        // 规则 2：AirPlay + 主机名含 appletv 才是电视
        let kind = kind_from_evidence(
            &sv(&["_airplay._tcp", "_companion-link._tcp"]),
            Some("Apple-TV-3.local"),
            None,
            false,
        );
        assert_eq!(kind, DeviceKind::Tv);
    }

    #[test]
    fn kind_airplay_speaker() {
        // 规则 3：裸 AirPlay（HomePod 不带 appletv 主机名）→ Speaker
        let kind = kind_from_evidence(
            &sv(&["_airplay._tcp", "_raop._tcp"]),
            Some("Living-Room.local"),
            None,
            false,
        );
        assert_eq!(kind, DeviceKind::Speaker);
    }

    #[test]
    fn kind_printer() {
        // 规则 5：_ipp / _printer / _scanner 一族
        let kind = kind_from_evidence(
            &sv(&[
                "_ipp._tcp",
                "_printer._tcp",
                "_scanner._tcp",
                "_pdl-datastream._tcp",
            ]),
            None,
            Some("Brother Industries"),
            false,
        );
        assert_eq!(kind, DeviceKind::Printer);
    }

    #[test]
    fn kind_router_wins_over_everything() {
        // 规则 1 优先级最高：网关哪怕带 _ipp（管理页有打印协议）也是 Router
        let kind = kind_from_evidence(&sv(&["_ipp._tcp"]), None, None, true);
        assert_eq!(kind, DeviceKind::Router);
        // SSDP ST 归一化形式同样命中
        let kind = kind_from_evidence(
            &sv(&["ssdp:urn:schemas-upnp-org:device:InternetGatewayDevice:1"]),
            None,
            None,
            false,
        );
        assert_eq!(kind, DeviceKind::Router);
    }

    #[test]
    fn kind_unknown() {
        // 什么都不命中 → Unknown（不是猜 Computer）
        assert_eq!(
            kind_from_evidence(&sv(&[]), None, None, false),
            DeviceKind::Unknown
        );
        // vendor Apple 不推设备类型（可能是 Mac 也可能是手机）
        assert_eq!(
            kind_from_evidence(&sv(&[]), None, Some("Apple, Inc."), false),
            DeviceKind::Unknown
        );
    }

    #[test]
    fn kind_camera_by_vendor() {
        assert_eq!(
            kind_from_evidence(&sv(&[]), None, Some("Hikvision Digital Technology"), false),
            DeviceKind::Camera
        );
    }

    #[test]
    fn snapshot_unavailable_shape() {
        let s = LanSnapshot::unavailable("本地网络授权被拒绝");
        assert!(s.devices.is_empty());
        assert!(s.iface.is_none() && s.cidr.is_none());
        assert_eq!(s.notes, vec!["本地网络授权被拒绝".to_string()]);
        // 序列化键名是 camelCase（↔ src/types/lan.ts）
        let v = serde_json::to_value(&s).unwrap();
        assert!(v.get("exposedPorts").is_some());
        assert!(v.get("exposed_ports").is_none());
    }
}
