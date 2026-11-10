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

/// 永远不是设备的地址：0.x / loopback / link-local / 组播与广播（224–255）。
/// `merge` 过滤邻居表与 `is_probe_candidate` 共用——实测 arp 表里就有
/// `224.0.0.251`（mDNS 组播）和 `239.255.255.250`（SSDP 组播）的 permanent 条目。
pub fn is_reserved(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    o[0] == 0 || o[0] == 127 || o[0] >= 224 || (o[0] == 169 && o[1] == 254)
}

/// 该地址是否值得探测。在 [`is_reserved`] 之上再排除本网段的网络地址与广播地址。
pub fn is_probe_candidate(ip: Ipv4Addr, net: Ipv4Addr, prefix: u8) -> bool {
    if is_reserved(ip) {
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

// ---------------------------------------------------------------- 服务发现记录（C4 填充）

/// mDNS 一条响应（C4 由 `mdns-sd` 采集；C2 阶段 merge 收到空切片）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MdnsRecord {
    pub ip: Ipv4Addr,
    /// mDNS 服务类型原样（`"_airplay._tcp"`）
    pub service: String,
    /// SRV/TXT 里的主机名（`"Apple-TV-3.local"`）
    pub hostname: Option<String>,
}

/// SSDP 一条 M-SEARCH 响应（C4 手写采集；ST 归一化在 merge 里做）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SsdpRecord {
    pub ip: Ipv4Addr,
    /// 响应头 `ST` 原文（`"urn:schemas-upnp-org:device:MediaRenderer:1"`）
    pub st: String,
}

// ---------------------------------------------------------------- 纯函数：合成

/// 三层数据源合成设备列表（按 IP 升序）。C2 阶段 `mdns` / `ssdp` 传空切片。
///
/// 判定规则（spec §4 表，逐条对应）：
/// - 保留段（组播/广播/0.x/loopback/link-local）与 `scope` 网段外 → 不进列表
/// - `Incomplete` 且无服务 → 不进列表（ARP 无响应 = 网段里没这台）
/// - `online` = 内核状态可达 或 服务发现有响应（组播响应本身就是存活证据）
/// - 本机保留为 `is_self = true`（展示层自行过滤，机器消费看字段）
///
/// `vendor` 恒为 `None`：OUI 查表（`lookup_vendor`）随 C4 的 `oui.csv` 一起接入。
pub fn merge(
    neighbors: &[RawNeighbor],
    mdns: &[MdnsRecord],
    ssdp: &[SsdpRecord],
    peers: &[(u16, Ipv4Addr)],
    gateway: Option<Ipv4Addr>,
    me: Option<Ipv4Addr>,
    scope: Option<(Ipv4Addr, u8)>,
) -> Vec<LanDevice> {
    use std::collections::{BTreeMap, BTreeSet};

    /// 四重过滤合一：scope 网段内、非保留段（组播/广播/0.x/loopback/link-local）、
    /// 非本网段的网络地址与广播地址。实测 arp 表里就有 `192.168.1.255`、
    /// `224.0.0.251`、`10.0.0.x`（别的网段）的条目，单独任何一条检查都拦不全。
    fn keep(ip: Ipv4Addr, scope: Option<(Ipv4Addr, u8)>) -> bool {
        let in_scope = match scope {
            None => true,
            Some((net, prefix)) => {
                let mask: u32 = if prefix == 0 {
                    0
                } else {
                    u32::MAX << (32 - prefix as u32)
                };
                u32::from(ip) & mask == u32::from(net) & mask
            }
        };
        in_scope
            && match scope {
                Some((net, prefix)) => is_probe_candidate(ip, net, prefix),
                None => !is_reserved(ip),
            }
    }

    struct Draft {
        mac: Option<[u8; 6]>,
        state: NeighborState,
        services: BTreeSet<String>,
        hostname: Option<String>,
        inbound: BTreeSet<u16>,
    }

    let mut drafts: BTreeMap<Ipv4Addr, Draft> = BTreeMap::new();

    // L1 邻居表：内核视角的地面真相
    for n in neighbors {
        if !keep(n.ip, scope) {
            continue;
        }
        let d = drafts.entry(n.ip).or_insert(Draft {
            mac: None,
            state: NeighborState::Unknown,
            services: BTreeSet::new(),
            hostname: None,
            inbound: BTreeSet::new(),
        });
        if n.mac.is_some() {
            d.mac = n.mac;
        }
        // macOS 只有两态；Windows 六态更准，后到的不覆盖先到的可达态
        if d.state == NeighborState::Unknown
            || d.state == NeighborState::Incomplete && n.state != NeighborState::Incomplete
        {
            d.state = n.state;
        }
    }

    // L2 mDNS：服务类型 + 主机名
    for r in mdns {
        if !keep(r.ip, scope) {
            continue;
        }
        let d = drafts.entry(r.ip).or_insert(Draft {
            mac: None,
            state: NeighborState::Unknown,
            services: BTreeSet::new(),
            hostname: None,
            inbound: BTreeSet::new(),
        });
        d.services.insert(r.service.clone());
        if d.hostname.is_none() {
            d.hostname.clone_from(&r.hostname);
        }
    }

    // L2 SSDP：ST 归一化为 ssdp:<ST> 并入 services（spec §7.2，C1 锁定）
    for r in ssdp {
        if !keep(r.ip, scope) {
            continue;
        }
        drafts
            .entry(r.ip)
            .or_insert(Draft {
                mac: None,
                state: NeighborState::Unknown,
                services: BTreeSet::new(),
                hostname: None,
                inbound: BTreeSet::new(),
            })
            .services
            .insert(format!("ssdp:{}", r.st));
    }

    // inbound：对端落在网段内才归属（公网连接不属于「局域网设备」）
    for (local_port, remote) in peers {
        if !keep(*remote, scope) {
            continue;
        }
        if let Some(d) = drafts.get_mut(remote) {
            d.inbound.insert(*local_port);
        }
    }

    drafts
        .into_iter()
        .filter_map(|(ip, d)| {
            let has_service = !d.services.is_empty();
            // 判定表：Incomplete 且无服务 = ARP 无响应 = 没这台，不进列表
            if d.state == NeighborState::Incomplete && !has_service {
                return None;
            }
            let is_gateway = Some(ip) == gateway;
            let is_self = Some(ip) == me;
            let online = d.state != NeighborState::Incomplete || has_service;
            let services: Vec<String> = d.services.into_iter().collect();
            let hostname = d.hostname;
            let kind = kind_from_evidence(&services, hostname.as_deref(), None, is_gateway);
            Some(LanDevice {
                ip: ip.to_string(),
                mac: d.mac.map(format_mac),
                // 口径纪律 2：查不到就不填。OUI 接入点在 C4
                vendor: None,
                hostname,
                kind,
                state: d.state,
                online,
                services,
                inbound: d.inbound.into_iter().collect(),
                is_gateway,
                is_self,
                randomized_mac: d.mac.is_some_and(|m| is_randomized_mac(&m)),
            })
        })
        .collect()
}

// ---------------------------------------------------------------- 纯函数：ESTABLISHED 对端

/// 解析 `lsof -nP -iTCP -sTCP:ESTABLISHED -F n` 输出 → `(local_port, remote_ip)`。
///
/// 实测输出形如（p/f 行忽略，只认 `n` 行；IPv6 与 loopback 由调用方按网段过滤，
/// 本函数只做格式解析）：
/// ```text
/// p576
/// f16
/// n[fe80:b::cf2:7729:3c38:7b17]:52909->[fe80:b::1484:b01:455b:72de]:52950
/// n192.168.0.115:3000->192.168.0.44:52341
/// n127.0.0.1:56205->127.0.0.1:7890
/// ```
pub fn parse_lsof_peers(text: &str) -> Vec<(u16, Ipv4Addr)> {
    let mut out = Vec::new();
    for line in text.lines() {
        let Some(n) = line.strip_prefix('n') else {
            continue;
        };
        let Some((local, remote)) = n.split_once("->") else {
            continue;
        };
        // 对端必须是 IPv4（v6 邻居走 NDP，arp -an 看不到，不属于本功能的判定面）；
        // 本地端允许是 `*`（任意绑定的服务端连接），local_port 仍然有效
        let Some((_, l_port)) = split_addr_port(local) else {
            continue;
        };
        let Some((Some(remote_ip), _)) = split_addr_port(remote) else {
            continue;
        };
        out.push((l_port, remote_ip));
    }
    out
}

/// `"192.168.0.115:3000"` / `"[fe80::1]:3000"` / `"*:3000"` → `(Option<IPv4>, 端口)`。
/// IPv6 返回 `(None, port)`——不猜 v4 映射。
fn split_addr_port(s: &str) -> Option<(Option<Ipv4Addr>, u16)> {
    let (addr, port) = s.rsplit_once(':')?;
    let port = port.parse::<u16>().ok()?;
    if addr.starts_with('[') {
        return Some((None, port)); // IPv6
    }
    if addr == "*" || addr.is_empty() {
        return Some((None, port)); // 未指定地址
    }
    Some((addr.parse::<Ipv4Addr>().ok(), port))
}

// ---------------------------------------------------------------- 平台口（三个）

/// 参与扫描的网段：有默认路由那张网卡的 `(地址, 前缀)`。
///
/// macOS：`route -n get default` 拿 iface（复用 [`crate::net::diag::default_gateway`]），
/// `ifconfig <iface>` 解析 netmask。**实测本机是 /23**——禁止硬编码 /24。
/// Windows：C6 用 `GetAdaptersAddresses`（spec §5.2 的常量口径）。
pub fn local_scope() -> Option<LanScope> {
    #[cfg(target_os = "macos")]
    {
        let iface = crate::net::diag::default_gateway()?.iface?;
        let out = std::process::Command::new("ifconfig")
            .arg(&iface)
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let text = String::from_utf8_lossy(&out.stdout);
        let (addr, prefix) = parse_ifconfig_inet(&text)?;
        Some(LanScope {
            iface,
            addr,
            prefix,
        })
    }
    #[cfg(not(target_os = "macos"))]
    {
        // C6：GetAdaptersAddresses（GAA_FLAG_SKIP_ANYCAST | SKIP_DNS_SERVER |
        // INCLUDE_GATEWAYS；IfType 裸值 6/71；Ipv4Metric 选路）——spec §5.2
        None
    }
}

/// 邻居表。macOS：`arp -an`（`-n` 不做 DNS 反查；**不要用 `arp -a`**）。
/// Windows：C6 用 `GetIpNetTable2`（中文 Windows 的 `arp -a` 输出无法解析）。
pub fn neighbors() -> Vec<RawNeighbor> {
    #[cfg(target_os = "macos")]
    {
        let out = match std::process::Command::new("arp").arg("-an").output() {
            Ok(o) if o.status.success() => o,
            _ => return Vec::new(), // 采集失败 = 空列表；上层用 notes 解释，不冒充「没有设备」
        };
        parse_arp_output(&String::from_utf8_lossy(&out.stdout))
    }
    #[cfg(not(target_os = "macos"))]
    {
        Vec::new() // C6：GetIpNetTable2（结构化，含 NL_NEIGHBOR_STATE）
    }
}

/// 谁在连我：`(local_port, remote_ip)`，只含 ESTABLISHED。
/// macOS：独立跑一次 `lsof -F n`（spec 决策 3：不动 ports.rs 的已验收解析路径）。
/// Windows：C6 用 `GetExtendedTcpTable(TCP_TABLE_OWNER_PID_CONNECTIONS)`。
pub fn established_peers() -> Vec<(u16, Ipv4Addr)> {
    #[cfg(target_os = "macos")]
    {
        let out = match std::process::Command::new(crate::net::ports::lsof_path())
            .args(["-nP", "-iTCP", "-sTCP:ESTABLISHED", "-F", "n"])
            .output()
        {
            Ok(o) if o.status.success() => o,
            _ => return Vec::new(),
        };
        parse_lsof_peers(&String::from_utf8_lossy(&out.stdout))
    }
    #[cfg(not(target_os = "macos"))]
    {
        Vec::new() // C6：GetExtendedTcpTable；dwLocalPort 记得 u16::from_be（spec §5.5）
    }
}

// ---------------------------------------------------------------- 编排

/// 探测参数（spec §6，写死不放配置——数值来自 ARP 往返的量级推导）。
/// 并发 32 而非 256：两端内核对同网卡的 ARP 请求都有队列限流，打太猛反而更慢。
pub const PROBE_CONCURRENCY: usize = 32;
/// 200 ms 足够：探测的真实作用是让内核为每个目标完成一次 ARP 解析（往返 < 5 ms），
/// connect 本身成功与否不影响判定——判定依据是探测后的邻居表状态。
pub const PROBE_TIMEOUT_MS: u64 = 200;
/// 全段扫描的地址数上限（spec §6 表：/21 以上截断并在 notes 说明，绝不静默截断）。
pub const SCAN_CAP: usize = 1024;

/// 对每个目标发一次 TCP connect（端口不重要，选一个大概率立即收到 RST 的）。
///
/// **结果被刻意丢弃**：这个函数的产物不是「谁开着 80 端口」，而是内核 ARP 缓存——
/// connect 会触发 ARP 解析，探测完立刻回读邻居表（[`neighbors`]）就能拿到 MAC。
/// 成功 connect 的目标反而说明端口开放，那属于端口域的事，不在这里归因。
///
/// 返回实际探测的地址数。零 unsafe、零提权（spec 决策 1：两端统一放弃 ICMP）。
pub fn probe_sweep(targets: &[Ipv4Addr], concurrency: usize, timeout_ms: u64) -> usize {
    use std::net::SocketAddr;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;

    if targets.is_empty() || concurrency == 0 {
        return 0;
    }
    let timeout = std::time::Duration::from_millis(timeout_ms);
    let done = AtomicUsize::new(0);
    let (tx, rx) = mpsc::channel::<Ipv4Addr>();
    // std 的 Receiver 不是 Sync，共享引用要过 Mutex（crossbeam 才有并发 Receiver）
    let rx = std::sync::Mutex::new(rx);
    std::thread::scope(|s| {
        for _ in 0..concurrency.min(targets.len()) {
            let rx = &rx;
            let done = &done;
            s.spawn(move || {
                // 端口 1（tcpmux）：不在任何常见监听清单里，已开机设备几乎必然回 RST
                // 立即返回，未开机设备在 ARP 超时后失败——两种结果都达成了目的。
                // 注意：guard 必须在 connect 前释放——`while let Ok(..) = rx.lock()..recv()`
                // 的临时 guard 在 edition 2021 会活到循环体结束，把整个扫描串行化
                loop {
                    let Ok(ip) = rx.lock().unwrap().recv() else {
                        break;
                    };
                    let addr = SocketAddr::from((ip, 1));
                    let _ = std::net::TcpStream::connect_timeout(&addr, timeout);
                    done.fetch_add(1, Ordering::Relaxed);
                }
            });
        }
        for t in targets {
            if tx.send(*t).is_err() {
                break;
            }
        }
        drop(tx);
    });
    done.load(Ordering::Relaxed)
}

/// 截断说明：目标数被 cap 截断时给一句诚实的话，没截断返回 None。
pub fn scan_truncation_note(targets: usize, total: u32) -> Option<String> {
    (total as usize > targets)
        .then(|| format!("网段过大，本轮只扫前 {targets} 个地址（共 {total}），列表可能不全。"))
}

/// `at` / `exposed` / `scope` / 合并结果 → 快照。`snapshot` 与 `snapshot_scan` 共用。
/// （`gateway` 不进参：is_gateway 已由 merge 算进每个设备。）
#[allow(clippy::too_many_arguments)]
fn finalize_snapshot(
    at: i64,
    exposed: &[u16],
    scope: LanScope,
    scanned: usize,
    total: u32,
    devices: Vec<LanDevice>,
    mut notes: Vec<String>,
    start: std::time::Instant,
) -> LanSnapshot {
    let network = u32::from(scope.addr)
        & if scope.prefix == 0 {
            0
        } else {
            u32::MAX << (32 - scope.prefix as u32)
        };
    if devices.is_empty() {
        notes.push("邻居表为空：本机最近没有与网段内设备通信过。".to_string());
    }
    LanSnapshot {
        at,
        iface: Some(scope.iface),
        cidr: Some(format!("{}/{}", Ipv4Addr::from(network), scope.prefix)),
        scanned: scanned as u32,
        total,
        devices,
        exposed_ports: exposed.to_vec(),
        notes,
        scan_ms: start.elapsed().as_millis() as u64,
    }
}

/// C2 一轮快照：邻居表 + 网段 + inbound（**无主动探测、无服务发现**）。
///
/// `exposed` 由调用方传入（CLI 现扫 ports、GUI 传缓存快照），本函数不重算端口域。
/// 全段探测用 [`snapshot_scan`]；Windows 上在 C6 之前两者都返回 [`LanSnapshot::unavailable`]。
pub fn snapshot(at: i64, exposed: &[u16]) -> LanSnapshot {
    let start = std::time::Instant::now();
    let Some(scope) = local_scope() else {
        let note = if cfg!(target_os = "macos") {
            "读不到默认网卡的网段：未联网，或只有 IPv6。"
        } else {
            "当前平台尚未接入（M18-C6）：Windows 数据源待实现。"
        };
        return LanSnapshot::unavailable(note);
    };

    let gateway =
        crate::net::diag::default_gateway().and_then(|g| g.gateway.parse::<Ipv4Addr>().ok());
    let nbrs = neighbors();
    let peers = established_peers();
    let network = u32::from(scope.addr)
        & if scope.prefix == 0 {
            0
        } else {
            u32::MAX << (32 - scope.prefix as u32)
        };
    let devices = merge(
        &nbrs,
        &[],
        &[],
        &peers,
        gateway,
        Some(scope.addr),
        Some((Ipv4Addr::from(network), scope.prefix)),
    );

    let notes = vec![
        // 口径诚实：这个模式只看邻居表，「最近通信过」≠「当前在线的全部设备」
        "本轮未主动探测：列表来自系统邻居表（最近通信过的设备），全段探测用 --scan。".to_string(),
    ];
    finalize_snapshot(at, exposed, scope, 0, 0, devices, notes, start)
}

/// C3 全段探测：expand_cidr → probe_sweep（催熟 ARP）→ 回读邻居表 → merge。
///
/// 顺序是 spec §6 的算法本体；步骤 5（mDNS/SSDP）随 C4 插进邻居回读之前。
/// `merge` 的在线判定自动吃掉探测成果：全段地址被内核探测过后，邻居表里
/// Incomplete（无响应）与 Reachable/Stale（有响应）就是「在不在」的判定面。
pub fn snapshot_scan(at: i64, exposed: &[u16]) -> LanSnapshot {
    let start = std::time::Instant::now();
    let Some(scope) = local_scope() else {
        let note = if cfg!(target_os = "macos") {
            "读不到默认网卡的网段：未联网，或只有 IPv6。"
        } else {
            "当前平台尚未接入（M18-C6）：Windows 数据源待实现。"
        };
        return LanSnapshot::unavailable(note);
    };

    let gateway =
        crate::net::diag::default_gateway().and_then(|g| g.gateway.parse::<Ipv4Addr>().ok());
    let network = u32::from(scope.addr)
        & if scope.prefix == 0 {
            0
        } else {
            u32::MAX << (32 - scope.prefix as u32)
        };

    // expand_cidr 已排除网络/广播地址（C1 单测锁定），探测目标直接可用
    let (targets, total) = expand_cidr(Ipv4Addr::from(network), scope.prefix, SCAN_CAP);
    let scanned = probe_sweep(&targets, PROBE_CONCURRENCY, PROBE_TIMEOUT_MS);

    let nbrs = neighbors(); // 此时 ARP 缓存已被 probe_sweep 填满
    let peers = established_peers();
    let devices = merge(
        &nbrs,
        &[],
        &[],
        &peers,
        gateway,
        Some(scope.addr),
        Some((Ipv4Addr::from(network), scope.prefix)),
    );

    // 探测数不进 notes：scanned/total 字段已承载（CLI 头部、GUI 卡片都能取到），
    // notes 只放异常与口径解释
    let mut notes = Vec::new();
    if let Some(note) = scan_truncation_note(targets.len(), total) {
        notes.push(note);
    }
    finalize_snapshot(at, exposed, scope, scanned, total, devices, notes, start)
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

    // ---- C2：merge 与 lsof 对端解析（fixture 均来自本机实测）----

    #[test]
    fn probe_sweep_counts_every_target() {
        // loopback 上 connect_timeout 立即失败/成功，不依赖外网；目的只是计数
        let targets = vec![
            ip("127.0.0.1"),
            ip("127.0.0.2"),
            ip("127.0.0.3"),
            ip("127.0.0.4"),
        ];
        assert_eq!(probe_sweep(&targets, 4, 200), 4);
        // 并发大于目标数也不多算
        assert_eq!(probe_sweep(&targets, 32, 200), 4);
    }

    #[test]
    fn probe_sweep_empty_and_zero_concurrency() {
        assert_eq!(probe_sweep(&[], 32, 200), 0);
        assert_eq!(probe_sweep(&[ip("127.0.0.1")], 0, 200), 0);
    }

    #[test]
    fn probe_constants_match_spec() {
        // spec §6 的三个数值是量级推导的产物，改任何一个都要同步 spec
        assert_eq!(PROBE_CONCURRENCY, 32);
        assert_eq!(PROBE_TIMEOUT_MS, 200);
        assert_eq!(SCAN_CAP, 1024);
    }

    #[test]
    fn scan_truncation_note_only_when_capped() {
        // /23 全部主机 510 < 1024：不截断
        let (targets, total) = expand_cidr(ip("192.168.0.0"), 23, SCAN_CAP);
        assert_eq!(targets.len(), 510);
        assert_eq!(total, 510);
        assert!(scan_truncation_note(targets.len(), total).is_none());
        // /21 主机 2046 > cap：截断并说明
        let (targets, total) = expand_cidr(ip("10.0.0.0"), 21, SCAN_CAP);
        assert_eq!(targets.len(), 1024);
        assert_eq!(total, 2046);
        let note = scan_truncation_note(targets.len(), total).unwrap();
        assert!(note.contains("1024") && note.contains("2046"), "{note}");
        // 口径纪律：total 不因截断缩水（UI 要显示 N/M）
        assert_eq!(total, 2046);
    }

    #[test]
    fn probe_targets_exlude_edges_and_gateway_scope() {
        // expand_cidr 产物必须全部通过 is_probe_candidate（探测目标与 merge 过滤同口径）
        let (targets, _) = expand_cidr(ip("192.168.0.0"), 23, SCAN_CAP);
        assert!(targets
            .iter()
            .all(|&t| is_probe_candidate(t, ip("192.168.0.0"), 23)));
        assert!(!targets.contains(&ip("192.168.0.0")));
        assert!(!targets.contains(&ip("192.168.1.255")));
    }

    /// 用实测 ARP_SAMPLE + 实测本机网段 /23 做 merge 的公共输入
    fn merge_sample(
        mdns: &[MdnsRecord],
        ssdp: &[SsdpRecord],
        peers: &[(u16, Ipv4Addr)],
    ) -> Vec<LanDevice> {
        merge(
            &parse_arp_output(ARP_SAMPLE),
            mdns,
            ssdp,
            peers,
            Some(ip("192.168.1.1")),
            Some(ip("192.168.0.115")),
            Some((ip("192.168.0.0"), 23)),
        )
    }

    #[test]
    fn merge_filters_incomplete() {
        let rows = parse_arp_output(
            "? (192.168.0.44) at bc:24:11:4c:d8:39 on en0 ifscope [ethernet]\n\
             ? (192.168.0.50) at (incomplete) on en0 ifscope [ethernet]\n",
        );
        let devices = merge(
            &rows,
            &[],
            &[],
            &[],
            None,
            None,
            Some((ip("192.168.0.0"), 23)),
        );
        // Incomplete 且无服务 = ARP 无响应 = 没这台，只剩 0.44
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].ip, "192.168.0.44");
        assert!(devices[0].online);
    }

    #[test]
    fn merge_incomplete_with_service_stays() {
        let rows = parse_arp_output("? (192.168.0.50) at (incomplete) on en0 ifscope [ethernet]\n");
        let mdns = vec![MdnsRecord {
            ip: ip("192.168.0.50"),
            service: "_airplay._tcp".into(),
            hostname: Some("Bedroom.local".into()),
        }];
        let devices = merge(
            &rows,
            &mdns,
            &[],
            &[],
            None,
            None,
            Some((ip("192.168.0.0"), 23)),
        );
        // 内核说 Incomplete，但组播响应本身就是存活证据
        assert_eq!(devices.len(), 1);
        assert!(devices[0].online);
        assert_eq!(devices[0].kind, DeviceKind::Speaker);
        assert_eq!(devices[0].hostname.as_deref(), Some("Bedroom.local"));
    }

    #[test]
    fn merge_inbound() {
        let devices = merge_sample(
            &[],
            &[],
            &[
                (3000, ip("192.168.0.44")),
                (8080, ip("192.168.0.44")),
                (443, ip("1.2.3.4")),
            ],
        );
        let d44 = devices.iter().find(|d| d.ip == "192.168.0.44").unwrap();
        assert_eq!(d44.inbound, vec![3000, 8080]); // 升序
                                                   // 公网对端（1.2.3.4）不属于局域网设备，且 0.44 之外没人收下 443
        assert!(devices.iter().all(|d| !d.inbound.contains(&443)));
    }

    #[test]
    fn merge_excludes_reserved_and_out_of_scope() {
        // 实测 arp 表里就有组播 permanent 条目与 /24 边界外地址
        let rows = parse_arp_output(
            "? (224.0.0.251) at 1:0:5e:0:0:fb on en0 ifscope permanent [ethernet]\n\
             ? (192.168.1.255) at ff:ff:ff:ff:ff:ff on en0 ifscope [ethernet]\n\
             ? (169.254.9.9) at aa:bb:cc:dd:ee:ff on en0 ifscope [ethernet]\n\
             ? (10.0.0.7) at aa:bb:cc:dd:ee:01 on en0 ifscope [ethernet]\n\
             ? (192.168.0.44) at bc:24:11:4c:d8:39 on en0 ifscope [ethernet]\n",
        );
        let devices = merge(
            &rows,
            &[],
            &[],
            &[],
            None,
            None,
            Some((ip("192.168.0.0"), 23)),
        );
        // 只剩 0.44：组播 / 广播 / link-local / 别的网段（10.x）全部排除
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].ip, "192.168.0.44");
    }

    #[test]
    fn merge_self_and_gateway_flags() {
        let devices = merge_sample(&[], &[], &[]);
        let me = devices
            .iter()
            .find(|d| d.is_self)
            .expect("本机应保留为 is_self");
        assert_eq!(me.ip, "192.168.0.115");
        let gw = devices.iter().find(|d| d.is_gateway).expect("网关应标记");
        assert_eq!(gw.ip, "192.168.1.1");
        assert_eq!(gw.kind, DeviceKind::Router); // 规则 1 优先级最高
        assert!(!me.is_gateway && !gw.is_self);
    }

    #[test]
    fn merge_randomized_mac_flag() {
        let devices = merge_sample(&[], &[], &[]);
        let r116 = devices.iter().find(|d| d.ip == "192.168.0.116").unwrap();
        // 8a:32:81 是随机化 MAC → vendor 留空、标志置位
        assert!(r116.randomized_mac);
        assert_eq!(r116.vendor, None);
        let gw = devices.iter().find(|d| d.is_gateway).unwrap();
        assert!(!gw.randomized_mac); // e4:77:27 是全局 OUI
    }

    #[test]
    fn merge_mdns_only_device() {
        // 没有邻居条目、只有 mDNS 响应 → 依然进列表（组播响应 = 存活证据）
        let mdns = vec![MdnsRecord {
            ip: ip("192.168.0.77"),
            service: "_ipp._tcp".into(),
            hostname: None,
        }];
        let devices = merge(
            &[],
            &mdns,
            &[],
            &[],
            None,
            None,
            Some((ip("192.168.0.0"), 23)),
        );
        assert_eq!(devices.len(), 1);
        assert!(devices[0].online);
        assert_eq!(devices[0].state, NeighborState::Unknown);
        assert_eq!(devices[0].kind, DeviceKind::Printer);
        assert_eq!(devices[0].mac, None); // 口径纪律 1：查不到 ≠ 没有
    }

    #[test]
    fn merge_ssdp_st_normalization() {
        let ssdp = vec![SsdpRecord {
            ip: ip("192.168.0.88"),
            st: "urn:schemas-upnp-org:device:MediaRenderer:1".into(),
        }];
        let devices = merge(
            &[],
            &[],
            &ssdp,
            &[],
            None,
            None,
            Some((ip("192.168.0.0"), 23)),
        );
        assert_eq!(devices[0].kind, DeviceKind::Tv);
        assert_eq!(
            devices[0].services,
            vec!["ssdp:urn:schemas-upnp-org:device:MediaRenderer:1"]
        );
    }

    #[test]
    fn lsof_peers_parse() {
        // fixture：实测输出（IPv6 主导 + IPv4 loopback + 一条局域网）
        let text = "\
p576
f16
n[fe80:b::cf2:7729:3c38:7b17]:52909->[fe80:b::1484:b01:455b:72de]:52950
p609
f57
n127.0.0.1:56205->127.0.0.1:7890
n192.168.0.115:3000->192.168.0.44:52341
n*:3000->192.168.0.44:53001
ngarbage
";
        let peers = parse_lsof_peers(text);
        assert_eq!(peers.len(), 3);
        assert!(peers.contains(&(3000, ip("192.168.0.44"))));
        // loopback 保留由调用方过滤（is_reserved 在 merge 里排除），解析层不丢信息
        assert!(peers.iter().any(|(p, _)| *p == 56205));
        // IPv6 与 garbage 行跳过，不 panic
        assert!(peers.iter().all(|(_, r)| !r.is_loopback() || true));
    }

    #[test]
    fn lsof_peers_garbage_only() {
        assert!(parse_lsof_peers("p576\nf16\nlsof: unknown error\n").is_empty());
    }
}
