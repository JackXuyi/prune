# M18 局域网设备 技术规格（spec）

> 配套文档：[m18-lan-devices-plan.md](./m18-lan-devices-plan.md)（定位、边界、平台差异总表）。
> 本文只写**怎么实现**：结构体、函数签名、平台调用、算法、测试、提交顺序。
> 平台：macOS 13+ / Windows 10 1809+。macOS 侧可在本机验证，Windows 侧标注为「待真机走查」的项全部未经实测。

---

## 1. 架构决策

### 决策 1：放弃 ICMP，统一 TCP connect 探测

| 方案 | 否决理由 |
|---|---|
| `IcmpSendEcho`（Windows） | `windows-sys 0.61.2` **不含该函数**（已核对 `IpHelper/mod.rs`，只有 `GetIpNetTable2` / `GetAdaptersAddresses` / `GetIpForwardTable2`）。要 ICMP 必须手写 `extern "system"` + 自建 `ICMP_ECHO_REPLY` 布局——正是 `win.rs` 头注释明确要规避的「布局写错读出垃圾值、且在 macOS 上永远测不出来」 |
| raw ICMP（macOS） | 需要 root，违反「永不提权」 |
| 非特权 ICMP datagram socket（macOS） | 跨版本行为不稳，且 Windows 无对应物 → 双平台代码分叉 |
| **TCP `connect_timeout`** | ✅ 标准库、零 unsafe、零权限、两端完全同构 |

关键认知：**探测的目的不是「TCP 通」，是把 ARP 缓存催熟**。判定在线的真正依据是邻居表状态（见 §6）。

### 决策 2：挂在「端口」tab 内做分段，不新增一级导航

`MainWindow.tsx` 已把 8 tab 收敛为 6（`stats / resources / memory / disk / net / ports`），并绑定了 `⌘1–⌘6`。新增第 7 个 tab 会破坏这套键位与「UX v3 方案 A」的收敛结论。

而「谁能连我的端口」和「同网段有谁」是同一个域：都回答可达性/暴露面。所以：

```
ports tab
├── 分段一：端口        （现状 PortBoard）
└── 分段二：局域网      （新增 LanBoard）
```

好处：可共享同一次端口快照，`exposed_ports` 不需要重算。

### 决策 3：`inbound` 独立采集，不动 `ports.rs`

现有 `ConnStats` 只有 `established` / `close_wait` 两个计数，**没有对端 IP**。要补有三条路：

| 方案 | 取舍 |
|---|---|
| 改 `ports.rs` 的 lsof 解析，加对端字段 | ❌ 动了 M7/M8 已验收的解析路径与 `ConnStats` 契约，回归面大 |
| **`lan.rs` 独立跑一次连接表查询** | ✅ 隔离；多一次 ~50 ms 采集，只取 `(local_port, remote_ip)` |
| 解析 `netstat -an` | ❌ 文本解析 + Windows locale 风险 |

选独立采集。**不叫「重算」**——这是新增维度（对端身份），不是重复已有口径。

---

## 2. 文件清单

| 文件 | 动作 | 内容 |
|---|---|---|
| `crates/prune-core/src/net/lan.rs` | 新增 ~700 行 | 数据结构、纯函数、编排、服务发现、OUI |
| `crates/prune-core/src/net/mod.rs` | 改 1 行 | `pub mod lan;` |
| `crates/prune-core/src/win.rs` | 追加 ~140 行 | `lan_scope()` / `arp_neighbors()` / `established_peers()` |
| `crates/prune-core/src/net/oui.csv` | 新增 ~600 行 | OUI 精简表，`include_str!` |
| `crates/prune-core/Cargo.toml` | 追加 1 依赖 | `mdns-sd`（双平台） |
| `src-tauri/src/lan.rs` | 新增 ~120 行 | 2 条 command + 事件推进度 |
| `src-tauri/src/lib.rs` | 改 2 行 | 注册 command + state |
| `src-tauri/Info.plist` | 新建 | 本地网络授权描述（见 §13） |
| `src/types/lan.ts` | 新增 | 与 Rust 一侧 serde 结构一一对应 |
| `src/stores/lanStore.ts` | 新增 | Zustand：snapshot / scanning / error |
| `src/components/LanBoard.tsx` | 新增 | 设备卡片列表 |
| `src/windows/MainWindow.tsx` | 改 ~15 行 | ports tab 内加分段 |
| `crates/prune-cli/src/main.rs` | 改 ~40 行 | `Lan { json, scan }` 子命令 |
| `crates/prune-cli/src/mcp.rs` | 改 ~30 行 | `lan_devices` 工具 |

---

## 3. 数据结构

`crates/prune-core/src/net/lan.rs`。全部 `serde(rename_all = "camelCase")`，与 `src/types/lan.ts` 一一对应。

```rust
/// 邻居状态。源：macOS arp 标记 / Windows NL_NEIGHBOR_STATE。
/// 保留原始六态而不是压成 bool —— 「内核视角」与「应用视角」不能合并（口径纪律 4）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NeighborState {
    Reachable,
    Stale,
    Delay,
    Probe,
    Incomplete,
    Permanent,
    /// 平台没给状态（macOS 只有 incomplete / permanent 两标记，其余归此）
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeviceKind {
    Router, Computer, Phone, Tablet, Tv, Speaker, Printer, Nas, Camera, Iot, Unknown,
}

/// 邻居表原始行（平台无关）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawNeighbor {
    pub ip: Ipv4Addr,
    /// None = ARP 未解析（incomplete）或该行无 MAC
    pub mac: Option<[u8; 6]>,
    pub iface: Option<String>,
    pub state: NeighborState,
}

/// 参与扫描的网段（有默认路由那张网卡）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanScope {
    pub iface: String,
    pub addr: Ipv4Addr,
    pub prefix: u8,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LanDevice {
    pub ip: String,
    /// 大写冒号分隔 "BC:24:11:4C:D8:39"；None = 邻居表无此条
    pub mac: Option<String>,
    /// OUI 查表结果；None = 查不到（含随机化 MAC），**不猜**
    pub vendor: Option<String>,
    /// mDNS / SSDP 得到的名字；None = 未拿到，不拼 "Device-192.168.0.44"
    pub hostname: Option<String>,
    pub kind: DeviceKind,
    pub state: NeighborState,
    /// 内核视角或 TCP 探测任一认定为活
    pub online: bool,
    /// ["_airplay._tcp", "ipp"]，去重升序
    pub services: Vec<String>,
    /// 该设备正在连的我方端口，升序
    pub inbound: Vec<u16>,
    pub is_gateway: bool,
    pub is_self: bool,
    /// 随机化 MAC（locally administered）→ UI 不展示厂商列
    pub randomized_mac: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LanSnapshot {
    pub at: i64,
    pub iface: Option<String>,
    /// "192.168.0.0/23"
    pub cidr: Option<String>,
    /// 已探测地址数
    pub scanned: u32,
    /// 上限内的目标总数（cap 1024）。scanned < total 时 UI 必须显示 "N/M"
    pub total: u32,
    pub devices: Vec<LanDevice>,
    /// 本机对同网段可达的监听端口（来自 PortSnapshot 的 BindScope::Any）
    pub exposed_ports: Vec<u16>,
    /// 必须显示给用户的原因（授权被拒 / 防火墙疑似拦截 / 多网卡只取一张 / cap 截断）
    pub notes: Vec<String>,
    pub scan_ms: u64,
}
```

`LanSnapshot::unavailable(note: &str) -> Self`：无数据但可解释的空快照（对齐 `PortSnapshot::unavailable` 的既有写法）。

---

## 4. 纯函数规格

每个都要求单测，测试数据用本机实测输出（见 §14）。

```rust
/// macOS `arp -an` 解析。
/// 实测样例（注意 MAC 可能是不补零的单十六进制位）：
///   ? (192.168.0.44) at bc:24:11:4c:d8:39 on en0 ifscope [ethernet]
///   ? (192.168.0.116) at 8a:32:81:3a:5:5c on en0 ifscope [ethernet]
///   ? (192.168.0.50) at (incomplete) on en0 ifscope [ethernet]
///   ? (224.0.0.251) at 1:0:5e:0:0:fb on en0 ifscope permanent [ethernet]
pub fn parse_arp_output(text: &str) -> Vec<RawNeighbor>;

/// "0xfffffe00" → 23。非法/非连续掩码 → None（不猜）
pub fn prefix_from_netmask_hex(hex: &str) -> Option<u8>;

/// macOS `ifconfig <iface>` 里 "inet 192.168.0.115 netmask 0xfffffe00 broadcast ..."
/// → (192.168.0.115, 23)
pub fn parse_ifconfig_inet(text: &str) -> Option<(Ipv4Addr, u8)>;

/// CIDR 展开可用主机地址（排除网络地址与广播地址）。
/// 返回 (地址列表, 上限内的目标总数)。超过 cap 时截断并只返回前 cap 个。
pub fn expand_cidr(net: Ipv4Addr, prefix: u8, cap: usize) -> (Vec<Ipv4Addr>, u32);

/// 该地址是否值得探测（排除 0.x / 组播 224-239 / 广播 / 保留段）
pub fn is_probe_candidate(ip: Ipv4Addr, net: Ipv4Addr, prefix: u8) -> bool;

/// [0xbc,0x24,0x11,...] → "BC:24:11:4C:D8:39"
pub fn format_mac(bytes: &[u8; 6]) -> String;

/// 第 1 字节 bit1 = locally administered（iPhone 私密 Wi-Fi / Android 随机化）
pub fn is_randomized_mac(bytes: &[u8; 6]) -> bool;

/// OUI 前 3 字节查厂商。二分查 oui.csv。未命中 → None
pub fn lookup_vendor(bytes: &[u8; 6]) -> Option<&'static str>;

/// 服务类型 + 主机名 + 厂商 + 是否网关 → 设备类型。规则见 §8，命中不了就是 Unknown。
/// hostname 参数是 spec §8 规则 2（AirPlay + appletv → Tv）的必要输入，
/// 比 §4 初版签名多一个参数（C1 实现时修正）。
/// services 里的 SSDP 记录已归一化为 `ssdp:<ST>`，规则 1/4 直接 contains 命中。
pub fn kind_from_evidence(
    services: &[String],
    hostname: Option<&str>,
    vendor: Option<&str>,
    is_gateway: bool,
) -> DeviceKind;

/// 三层结果合并成设备列表（按 ip 升序）。纯函数，输入都是上面几个函数的产物。
/// C2 实现时修正两处：`iface` 参数去掉（过滤由 scope 完成，iface 在 merge 内无用）；
/// 新增 `scope`（过滤「非本次网段的地址」必须有网段信息，原签名做不到）。
pub fn merge(
    neighbors: &[RawNeighbor],
    mdns: &[MdnsRecord],
    ssdp: &[SsdpRecord],
    peers: &[(u16, Ipv4Addr)],   // (local_port, remote_ip)
    gateway: Option<Ipv4Addr>,
    me: Option<Ipv4Addr>,
    scope: Option<(Ipv4Addr, u8)>,
) -> Vec<LanDevice>;
```

`merge` 的判定规则（写成表，实现时逐条对应）：

| 条件 | online | kind |
|---|---|---|
| `is_self` | true | —（本机不展示，只用于 `exposed_ports` 的说明） |
| `state == Incomplete` 且无服务 | **false** | —（ARP 无响应 = 网段里没这台；不进列表） |
| `state in (Reachable, Stale, Delay, Probe, Permanent)` | true | 由证据推断 |
| 无邻居条目但 mDNS/SSDP 有响应 | true | 由服务推断（组播响应本身就是存活证据） |
| 无邻居条目、无服务，仅 TCP 探测成功 | true | Unknown |

**必须过滤掉**：组播（`224.0.0.0/4`）、广播、`0.0.0.0`、本机地址、非本次 scope 网段的地址。

---

## 5. 平台分叉（三个口）

对齐 `diag.rs` 的做法：**平台差异只暴露少数几个函数，其余全部平台无关**。

```rust
// macOS: ifconfig 解析；Windows: GetAdaptersAddresses
pub fn local_scope() -> Option<LanScope>;

// macOS: arp -an 子进程；Windows: GetIpNetTable2
pub fn neighbors() -> Vec<RawNeighbor>;

// macOS: lsof -nP -iTCP -sTCP:ESTABLISHED；Windows: GetExtendedTcpTable(CONNECTIONS)
pub fn established_peers() -> Vec<(u16, Ipv4Addr)>;
```

### 5.1 `local_scope()` — macOS

```rust
// route -n get default → interface（复用 diag::default_gateway().iface）
// ifconfig <iface>   → parse_ifconfig_inet()
```

实测本机：`inet 192.168.0.115 netmask 0xfffffe00` → **/23**，510 个主机地址。
**禁止硬编码 /24**：写死 254 会在这台机器上直接漏掉一半网段。

### 5.2 `local_scope()` — Windows（待真机走查）

`GetAdaptersAddresses`（在 `Win32_NetworkManagement_IpHelper`，feature 已开，无需改 Cargo）：

```rust
// ⚠ 常量口径已核对 windows-sys 0.61.2 源码（含两条「不存在」），不要凭记忆写：
// - GAA_FLAG_SKIP_ANYCAST = 2
// - GAA_FLAG_SKIP_DNS_SERVER = 8
// - GAA_FLAG_INCLUDE_GATEWAYS = 128   ← 不传这个，FirstGatewayAddress 恒为 null
// - 不存在 GAA_FLAG_SKIP_MULTICAST（0.61.2 未导出）
let flags = GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_DNS_SERVER | GAA_FLAG_INCLUDE_GATEWAYS;
// retry 模式：先 16 KB buffer，ERROR_BUFFER_OVERFLOW 时按返回的 size 重试一次
```

筛选条件（四条全满足）：

1. `OperStatus == IfOperStatusUp`（`Ndis::IfOperStatusUp = 1`）
2. `IfType` ∈ `6`（ethernet）或 `71`（ieee80211）—— 裸值，**windows-sys 0.61.2 未导出 `IF_TYPE_*` 常量**；
   值来自 IANA ifType MIB，写注释说明来源。第 1、2 条天然排除 loopback(24) / tunnel(131) / PPP(23)
3. `FirstUnicastAddress` 里有非 `169.254/16` 的 IPv4，取该条目的 `OnLinkPrefixLength: u8` 作 prefix
4. `FirstGatewayAddress != null`（前提：flags 带了 `GAA_FLAG_INCLUDE_GATEWAYS`）

多候选时直接用 adapter 自带的 **`Ipv4Metric: u32` 取最小**——不必再查 `GetIpForwardTable2`。
其余候选写进 `notes`：「检测到 N 张可用网卡，本轮只扫 `<iface>`」。`iface` 取 `FriendlyName`（`PWSTR`，需 `to_string_lossy`）。

这一步同时解决 **Hyper-V / WSL / Docker 的 `172.17.x` 假设备**问题：它们的 `FirstGatewayAddress` 为 null，或 IfType 不是以太网/Wi-Fi，会被四条条件直接筛掉。

`MIB_IPNET_TABLE2` 的形状是 `{ NumEntries: u32, Table: [MIB_IPNET_ROW2; 1] }`——变长数组，
按 `NumEntries` 走指针偏移读，不要按 `Table` 的声明长度 1 去遍历。

### 5.3 `neighbors()` — macOS

子进程 `arp -an`，`parse_arp_output()` 解析。**不用 `arp -a`**（带 DNS 反查，慢且依赖网络）。

状态映射：

| arp 输出 | NeighborState | online |
|---|---|---|
| `at (incomplete)` | `Incomplete` | false |
| `... permanent [ethernet]` | `Permanent` | true（过滤后仅剩网关/静态项） |
| 有 MAC、无 permanent 标记 | `Reachable` | true |

macOS 的 arp 表不区分 Reachable/Stale，一律给 `Reachable`；**不要伪造六态**，`Unknown` 只用于平台没给的情形。

### 5.4 `neighbors()` — Windows（待真机走查）

`GetIpNetTable2(AF_INET, &mut table)` → `MIB_IPNET_TABLE2`。注意三点：

1. `MIB_IPNET_ROW2.Address` 是 `SOCKADDR_INET` union。按 `win.rs::default_gateway()` 已有的 ABI 读法：读起始 16 字节，IPv4 在 `bytes[4..8]`。**不要**去猜 union 的字段名（`windows-sys` 版本间变过）。
2. `PhysicalAddressLength == 0` → `mac = None`，配合 `State` 判定。
3. `State` 是 `NL_NEIGHBOR_STATE`：`Unreachable`(0) / `Incomplete`(1) / `Probe`(2) / `Delay`(3) / `Stale`(4) / `Reachable`(5) / `Permanent`(6)。
4. **必须 `FreeMibTable(table.cast())`**，两条返回路径都要走（`win.rs` 的既有 SAFETY 注释范式）。

高风险点：**扫描结束后邻居表里会残留大量 `Incomplete` 条目**（每个目标都发过 ARP）。这些必须过滤掉，否则用户看到满屏 IP 却不是设备。这是「扫描污染邻居表」的副作用，不是设备。

### 5.5 `established_peers()` — 双平台

| 平台 | 命令/API | 提取 |
|---|---|---|
| macOS | `lsof -nP -iTCP -sTCP:ESTABLISHED -F n` | `n<local>:<port>-><remote>:<port>` 拆出 `(local_port, remote_ip)` |
| Windows | `GetExtendedTcpTable(TCP_TABLE_OWNER_PID_CONNECTIONS)` | `dwState == MIB_TCP_STATE_ESTAB` 的行，`dwLocalPort` / `dwRemoteAddr` |

两个平台的端口字段都是网络字节序，**记得 `u16::from_be`**（Windows 的 `dwLocalPort` 是 DWORD 里塞了 BE 的 16 位，低位有效）。

只保留 `remote_ip` 落在本次 scope 网段内的记录 —— 公网连接不属于「局域网设备」。

---

## 6. 探测算法

```
1. local_scope()            → (addr, prefix)
2. expand_cidr(cap=1024)    → (targets, total)
3. probe_sweep(targets; 并发 32, 超时 200 ms)
4. neighbors()              → 回读邻居表（此时 ARP 缓存已被步骤 3 填满）
5. mdns_discover(30 s) + ssdp_discover(3 s)   ← 与步骤 3/4 可并行
6. established_peers()
7. merge(...)               → LanSnapshot
```

**为什么超时只要 200 ms**：TCP 握手成功与否不影响判定——步骤 3 的真实作用是在内核里为每个目标触发一次 ARP 解析。局域网 ARP 往返 < 5 ms，200 ms 是三个数量级余量。判定依据是步骤 4 的邻居状态。

并发取 32 而非 256：macOS/Windows 对同一网卡的 ARP 请求都有队列限流，一次性打 1024 个会触发限流反而更慢。32 × 200 ms → 1024 地址约 **6.4 s** 上限，实测应更快（大量目标收到 RST 立即返回）。

`expand_cidr` 的 cap：

| prefix | 主机数 | 处理 |
|---|---|---|
| /24 | 254 | 全扫 |
| **/23（本机实测）** | **510** | 全扫 |
| /22 | 1022 | 全扫 |
| /21 | 2046 | 截断到 1024，`notes` 写「网段过大，本轮只扫前 1024 个地址（共 2046）」 |
| /16 | 65534 | 同上（不能静默截断） |

---

## 7. 服务发现

### 7.1 mDNS（双平台）

依赖 `mdns-sd`（纯 Rust，走 5353 多播，无需 root）。首次接入时以 crates.io 最新稳定版为准，编译通过后再固定版本号——**不要在不验证的情况下写死版本**。

监听的服务类型（浏览器实例一轮，30 s 后收）：

| 服务类型 | 用途 |
|---|---|
| `_services._dns-sd._udp.local` | 枚举本网所有服务类型（实测 4 s 内出结果） |
| `_airplay._tcp` / `_raop._tcp` | Apple TV / HomePod / 音箱 |
| `_companion-link._tcp` | iPhone / iPad / Mac |
| `_ipp._tcp` / `_ipps._tcp` / `_printer._tcp` / `_pdl-datastream._tcp` / `_scanner._tcp` | 打印机 / 扫描仪 |
| `_smb._tcp` / `_afpovertcp._tcp` | NAS |
| `_googlecast._tcp` | Chromecast / 电视 |
| `_hap._tcp` | HomeKit 设备 |
| `_workstation._tcp` / `_ssh._tcp` | 电脑 / 服务器 |

每轮只保留 `Addr::V4` 的记录；同一 IP 多条记录合并进 `services`（去重升序）。

### 7.2 SSDP（双平台，Windows 上优先）

用标准库 `UdpSocket` 手写，不引 crate（约 60 行）：

```http
M-SEARCH * HTTP/1.1\r\n
HOST: 239.255.255.250:1900\r\n
MAN: "ssdp:discover"\r\n
MX: 2\r\n
ST: ssdp:all\r\n
\r\n
```

发到 `239.255.255.250:1900`，收 3 s（`set_read_timeout`）。解析响应头里的 `ST` / `USN` / `SERVER`，**不请求 `LOCATION` 指向的 XML**：

> 不对局域网设备发起 HTTP 请求。对方的 Web 服务日志会记录这次访问，那已经从「看谁在网里」越界成「主动探测别人」。这一条写进代码注释，避免以后有人「顺手补全」friendlyName。

**ST 归一化**：SSDP 的 `ST` 转成 `ssdp:<ST>` 字符串并入 `services`（如 `ssdp:MediaRenderer`、
`ssdp:urn:schemas-upnp-org:device:InternetGatewayDevice:1`），与 mDNS 记录同一数组，
`kind_from_evidence` 直接 `contains` 命中（C1 已实现并锁定该约定）。

Windows 上 SSDP 比 mDNS 可靠：`M-SEARCH` 是出站发起、响应单播回来，状态化防火墙放行关联；mDNS 要监听 5353 入站，可能被静默 DROP。

### 7.3 mDNS 为空的处理

Windows 上 mDNS 结果为空是**高频且无声**的（防火墙拦截不报错）。规则：

- mDNS 空 + SSDP 也空 → `notes.push("mDNS / SSDP 均无响应，可能被防火墙拦截；这不代表局域网内没有设备")`
- 只有 mDNS 空 → 不提示（SSDP 有结果就够了），但 `services` 会明显偏少
- **绝不做**：把「没发现服务」渲染成「没有设备」——邻居表仍然列出了设备

---

## 8. 设备类型推断

`kind_from_evidence()` 按**优先级从上到下**，命中即返回：

| 序 | 证据 | kind |
|---|---|---|
| 1 | `is_gateway` 或 SSDP `ST` 含 `InternetGatewayDevice` | `Router` |
| 2 | `_airplay._tcp` + hostname 含 `apple-tv` / `appletv` | `Tv` |
| 3 | `_airplay._tcp` 或 `_raop._tcp` | `Speaker` |
| 4 | `_googlecast._tcp` 或 SSDP `MediaRenderer` | `Tv` |
| 5 | `_ipp` / `_ipps` / `_printer` / `_pdl-datastream` / `_scanner` | `Printer` |
| 6 | `_smb._tcp` / `_afpovertcp._tcp` | `Nas` |
| 7 | `_hap._tcp` | `Iot` |
| 8 | `_companion-link._tcp` | `Phone`（区分不了 iPhone/iPad，统一按 Phone；UI 用同一个图标） |
| 9 | `_workstation._tcp` / `_ssh._tcp` | `Computer` |
| 10 | vendor 含 `Hikvision` / `Dahua` / `Axis` / `Reolink` | `Camera` |
| 11 | 以上都不命中 | `Unknown` |

**刻意不做**：TTL 指纹、端口开放组合推 OS、厂商名推设备类型（`Apple` 既可能是手机也可能是 Mac）。错判比不判更糟——`Unknown` 的图标比错误图标诚实。

---

## 9. OUI 与随机化 MAC

`oui.csv` 格式（无表头，`include_str!` 编进二进制）：

```
BC2411,Wistron Neweb Corporation
E47727,Hon Hai Precision Ind. Co.,Ltd.
```

约 600 行、~18 KB，只收常见消费电子 / 网络设备厂商。读取用 `binary_search_by`（文件按 OUI 升序预排），命中失败返回 `None`。

### 随机化 MAC 是常态，不是异常

本机实测 10 条邻居里有 **5 条** 是 locally administered（第 1 字节 bit1 = 1）：

```
f6:74:4d / 52:cb:56 / 6e:a7:e6 / 8a:32:81 / 82:ac:9b   ← 全部随机化
bc:24:11 / e4:77:27                                    ← 全局 OUI
```

原因：iPhone「私密 Wi-Fi 地址」、Android 随机化、部分 Windows 也默认开启。

因此：

1. `is_randomized_mac()` 为真 → `vendor = None`、`randomized_mac = true`，**不查 OUI**（查出来也是错的）。
2. UI 对这类设备不显示厂商列（不是显示「未知厂商」）。
3. **设备不能靠 MAC 做聚合键**：同一台 iPhone 换网络就换 MAC。聚合键统一用 `ip`，跨轮次的「同一设备」不做承诺（本轮快照语义）。
4. `notes` 里可选提示一句：「N 台设备使用随机化 MAC，无法识别厂商」——但不作为错误。

---

## 10. IPC 契约

`src/types/lan.ts`（与 §3 的 serde 结构严格对应，改一侧必须同步另一侧）：

```ts
/** 与 prune-core::net::lan 一一对应（camelCase）。改这里必须同步 Rust 侧。 */
export type NeighborState =
  | "reachable" | "stale" | "delay" | "probe" | "incomplete" | "permanent" | "unknown";

export type DeviceKind =
  | "router" | "computer" | "phone" | "tablet" | "tv"
  | "speaker" | "printer" | "nas" | "camera" | "iot" | "unknown";

export interface LanDevice {
  ip: string;
  mac: string | null;
  vendor: string | null;
  hostname: string | null;
  kind: DeviceKind;
  state: NeighborState;
  online: boolean;
  services: string[];
  inbound: number[];
  isGateway: boolean;
  isSelf: boolean;
  randomizedMac: boolean;
}

export interface LanSnapshot {
  at: number;
  iface: string | null;
  cidr: string | null;
  scanned: number;
  total: number;
  devices: LanDevice[];
  exposedPorts: number[];
  notes: string[];
  scanMs: number;
}
```

Tauri command：

```rust
#[tauri::command] pub async fn lan_snapshot() -> Result<LanSnapshot, String>;   // 读缓存，不扫
#[tauri::command] pub async fn lan_scan(app: AppHandle, state: State<'_, Arc<LanState>>)
    -> Result<LanSnapshot, String>;                                            // 全量一轮
```

- `lan_scan` 用 `tauri::async_runtime::spawn_blocking` 包住（扫描 ≤ 7 s，不能堵 async 运行时，对齐 `net_diag_run`）。
- 进度事件：`lan://progress`，payload `{ phase: "neighbors" | "probe" | "discover" | "merge", done: number, total: number }`。前端只需在 `probe` 阶段显示进度条，其余阶段显示文案。
- 并发保护：`LanState` 持 `AtomicBool scanning`，已在扫时直接返回上一次快照 + `notes.push("已有一轮扫描在进行")`，不叠加任务。
- 不需要新增 capability：`capabilities/default.json` 只声明 `core:default`，自定义 command 不受其限制（与 `net_diag_run` 现状一致）。

---

## 11. UI 规格

位置：`ports` tab 内，`Tabs` 组件做两分段（`端口` / `局域网`）。

**触发**：进入分段不扫描。空态显示一句说明 + 一个「扫描局域网」按钮。这是 Prune 第一次引入系统权限弹窗，**不能让它在 App 启动时蹦出来**。

三种状态：

| 状态 | 展示 |
|---|---|
| 未扫描 | 说明文案 + 按钮（「只探测本网段设备，不访问设备上的任何服务」——把边界写在按钮下面，不是藏在设置里） |
| 扫描中 | 进度条 `已探测 N/M` + 阶段文案（正在读取邻居表 / 正在探测 / 正在发现服务） |
| 有结果 | 设备列表 + 顶部摘要条 |

摘要条（一行，小字）：

```
192.168.0.0/23 · 6 台在线 · 我对这些设备开放 3 个端口（3000 / 3118 / 8080）
```

设备卡（列表行，非大卡）：

```
[图标] 192.168.0.44        Apple TV           在线 2 min
       BC:24:11:4C:D8:39 · Wistron Neweb      连了我 3000
```

字段取舍：

- `mac = null` → 显示 `—`，不显示「未知 MAC」
- `randomizedMac = true` → MAC 后加 `· 随机` 小标，厂商列整体省略
- `vendor = null` 且非随机 → 显示 `—`，不显示「未知厂商」
- `state = stale` → 状态显示「最近在线」而非「在线」
- `inbound` 非空 → 右侧高亮 `连了我 3000`
- `notes` 非空 → 列表上方一条浅色提示条，逐条列出（不折叠——这些是「读数可信度」的一部分）

**不排序控件**：按 `isGateway desc, online desc, ip asc` 固定排序。M10 的六字段排序是为应用卡设计的，设备列表只有 6–20 行，加排序是过度设计。

---

## 12. CLI 与 MCP

### CLI

```
prune lan [--json] [--scan]
```

- 不带 `--scan`：只读邻居表（< 200 ms），不探测、不发包 —— **默认行为必须是无副作用**，和 `reclaim` 默认 dry-run 同一原则
- `--scan`：完整一轮（含探测 + 服务发现，≤ 40 s）
- `--json`：`LanSnapshot` 序列化

`Command` enum 追加：

```rust
Lan { json: bool, scan: bool },
```

### MCP

工具名 `lan_devices`，只读：

```json
{
  "name": "lan_devices",
  "description": "局域网设备与暴露面（只读）。返回同网段设备列表 + 本机对它们开放的端口。注意：会向局域网发送 TCP SYN（--scan 语义），mDNS 监听 30s。设备厂商无法识别时 vendor 为 null，不代表没有厂商。",
  "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
}
```

MCP 侧**固定读缓存**，不触发扫描：Agent 高频调用不能每次都发 1024 个 SYN，且 MCP server 无 UI 承接权限弹窗。无缓存时返回 `{ "status": "unavailable", "note": "尚未在 GUI 中扫描过" }` —— 对齐 `mem_leaks` 读 GUI 序列库的既有做法。

---

## 13. 打包改动

`src-tauri/Info.plist`（当前不存在，新建）：

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>NSLocalNetworkUsageDescription</key>
  <string>Prune 需要访问本地网络来列出同一网段内的设备，并显示本机对这些设备开放的端口。只探测存活状态，不访问设备上的任何服务。</string>
  <key>NSBonjourServices</key>
  <array>
    <string>_services._dns-sd._udp</string>
    <string>_airplay._tcp</string>
    <string>_raop._tcp</string>
    <string>_companion-link._tcp</string>
    <string>_ipp._tcp</string>
    <string>_ipps._tcp</string>
    <string>_printer._tcp</string>
    <string>_pdl-datastream._tcp</string>
    <string>_scanner._tcp</string>
    <string>_smb._tcp</string>
    <string>_afpovertcp._tcp</string>
    <string>_googlecast._tcp</string>
    <string>_hap._tcp</string>
    <string>_workstation._tcp</string>
    <string>_ssh._tcp</string>
  </array>
</dict>
</plist>
```

**验证项（不可跳过）**：`pnpm tauri build` 后 `plutil -p "src-tauri/target/release/bundle/macos/Prune.app/Contents/Info.plist" | grep -A3 Bonjour` 确认键真的进了最终 bundle。Tauri 2 会合并 `src-tauri/Info.plist`，但这条路径**必须在第一次打包时实测确认**——合并不生效的话，`dns-sd` 会静默返回空，且没有任何报错，是最难查的一类问题。

Windows 侧无对应 plist；`bundle.targets` 已含 `nsis`/`msi`，不需要改。

---

## 14. 测试矩阵

### 单测（纯函数，`cargo test -p prune-core`）

| 测试 | fixture | 断言 |
|---|---|---|
| `arp_parse_full` | 本机实测 10 行原文 | 10 条；`192.168.0.116` 的 MAC 解析为 `8A:32:81:3A:05:5C`（**单十六进制位要补零**） |
| `arp_parse_incomplete` | `? (192.168.0.50) at (incomplete) on en0` | `mac = None`、`state = Incomplete` |
| `arp_parse_permanent` | `224.0.0.251 ... permanent` | `state = Permanent` |
| `arp_parse_garbage` | `arp: bad option` / 空串 | 返回空 Vec，不 panic |
| `netmask_hex` | `0xfffffe00` / `0xffffff00` / `0xffff0000` | 23 / 24 / 16 |
| `netmask_noncontiguous` | `0xff00ff00` | `None` |
| `ifconfig_inet` | `inet 192.168.0.115 netmask 0xfffffe00 broadcast 192.168.1.255` | `(192.168.0.115, 23)` |
| `cidr_expand_23` | `192.168.0.0/23` | 510 个地址，首 `192.168.0.1`，末 `192.168.1.254` |
| `cidr_cap` | `/16`, cap 1024 | 返回 1024 个，`total = 65534` |
| `probe_candidate` | 组播 / 广播 / 0.0.0.0 | 全部 false |
| `mac_format_pad` | `[0x8a,0x32,0x81,0x3a,0x05,0x5c]` | `"8A:32:81:3A:05:5C"` |
| `mac_randomized` | `f6:74:4d` / `bc:24:11` | true / false |
| `kind_airplay_tv` | `["_airplay._tcp"]` + hostname `appletv` | `Tv` |
| `kind_printer` | `["_ipp._tcp"]` | `Printer` |
| `kind_unknown` | `[]` | `Unknown`（不是猜 Computer） |
| `merge_filters_incomplete` | 1 条 Incomplete + 1 条 Reachable | 只出 1 台设备 |
| `merge_inbound` | peers 含 `(3000, 192.168.0.44)` | 对应设备 `inbound = [3000]` |
| `vendor_binary_search` | 表内第一条 / 表内最后一条 / 不存在 | Some / Some / None |

### 集成（手工，macOS 本机）

1. `prune lan` 不扫：设备数应等于 `arp -an` 过滤后的条目数
2. `prune lan --scan`：510 地址，耗时 < 15 s，设备数 ≥ 步骤 1
3. 打开一个 `0.0.0.0:3000` 服务，手机访问后 `prune lan` 能看到该手机 `inbound = [3000]`
4. 网关出现在列表首位且 `kind = router`
5. `prune lan --json | jq '.devices[].vendor'` 中随机 MAC 设备为 `null`（不是 "Unknown"）

### Windows 真机走查（与 W4 一批，未验证前不得标记完成）

| # | 项 | 判据 |
|---|---|---|
| 1 | 首次 `lan_scan` 是否触发防火墙提示 | 记录实际弹窗文案 |
| 2 | `GetIpNetTable2` 的 State 取值分布 | 打印各 state 计数，确认 Incomplete 被过滤 |
| 3 | Hyper-V / WSL / Docker 网段 | `172.17.x` / `172.x` 不出现在设备列表 |
| 4 | 中文 Windows 输出 | MAC / 厂商无乱码（验证点：根本没解析命令输出） |
| 5 | mDNS 是否被拦 | 对照手机上的「发现 AirPlay」类 App 结果 |
| 6 | 多网卡（Wi-Fi + 有线） | 只扫有默认路由那张，`notes` 说明 |
| 7 | `established_peers` 端口字节序 | 本机开 3000，手机访问，`inbound` 必须显示 `3000` |

第 7 条最容易出错：`dwLocalPort` 是 DWORD 里塞网络字节序的 16 位，忘了 `u16::from_be` 会把 `3000`（`0x0BB8`）读成 `47115`（`0xB80B`）。判据只认「端口号能对上 3000」，不认任何「看起来合理」的数。

---

## 15. 实施顺序（6 个可提交单元）

每个单元结束都要 `cargo test -p prune-core` + `cargo check --workspace` 绿，再提交。

| # | 内容 | 交付判据 |
|---|---|---|
| C1 | `lan.rs` 数据结构 + 全部纯函数 + 单测 | `cargo test` 覆盖 §14 前 15 行；无 IO 代码 |
| C2 | macOS `local_scope` + `neighbors` + `established_peers` | `prune lan --json` 能出设备列表（不扫） |
| C3 | `probe_sweep` + `expand_cidr` 接入 + `merge` 在线判定 | `prune lan --scan` 出全段结果 |
| C4 | `mdns-sd` + SSDP + 类型推断 + `oui.csv` | 设备卡带类型与厂商 |
| C5 | Tauri IPC + `lanStore` + `LanBoard` + ports tab 分段 | GUI 可扫、可看 |
| C6 | `Info.plist` + MCP 工具 + Windows 三个口（`win.rs`） | 打包后 plist 校验通过；`cargo check --target x86_64-pc-windows-msvc` 过 |

C6 的 Windows 部分**只保证编译**，功能正确性留给 W5 真机走查。这句要写进提交信息，避免后来人误以为已验证。

---

## 16. 验收命令

```bash
# 1. 纯函数与合并逻辑
cargo test -p prune-core lan::

# 2. 全工作区编译（含 Windows 交叉检查基线）
cargo check --workspace
MINGW_BIN=... ./scripts/check-windows.sh   # 沿用 docs/windows-support-plan.md §3 的现有脚本

# 3. CLI 冒烟（不扫）
cargo run -p prune-cli -- lan --json | jq '{iface, cidr, count: (.devices|length)}'

# 4. 全量扫描
cargo run -p prune-cli -- lan --scan --json | jq '.devices[] | {ip, mac, vendor, kind, online}'

# 5. 打包后 plist 校验（唯一能证明授权文案真的进包的方式）
pnpm tauri build
plutil -p "src-tauri/target/release/bundle/macos/Prune.app/Contents/Info.plist" \
  | grep -A17 NSBonjourServices
```

---

## 17. 风险与回滚

| 风险 | 影响 | 处置 |
|---|---|---|
| macOS 授权被拒 | 邻居表仍可读（`arp -an` 不受限），mDNS 与探测受限 | `notes` 写明去「系统设置 > 隐私与安全性 > 本地网络」开启；列表照常显示邻居表内容，不显示空态 |
| Windows 防火墙拦 mDNS | 服务类型缺失，设备全是 `Unknown` | SSDP 兜底 + `notes` 明示；不自动改防火墙规则 |
| 扫描污染邻居表 | Windows 上残留大量 `Incomplete` | §5.4 强制过滤；单测 `merge_filters_incomplete` 锁死 |
| cap 截断造成「只有这些设备」的错觉 | 误判 | `scanned < total` 时 UI 必须显示 `N/M`，`notes` 写清总数 |
| OUI 表过时 | 厂商缺失 | `vendor = null` 兜底，README 注明快照日期，不显示「未知」 |
| 新权限弹窗冲击「零权限」叙事 | 产品承诺自相矛盾 | README 安全护栏第 5 条改为「永不请求 root；本地网络授权是唯一一次隐私授权，且只在用户主动点击时触发」（随 C5 一起做） |
| **隐私声明「无网络请求」直接失效** | 法律级文案失真 | `prune-feature-spec-v2.md:275` 现写「零埋点、零遥测、**无网络请求**（除更新检查）」，该行标注为「营销资产」。M18 会主动发 510 个 TCP SYN + mDNS/SSDP 组播，必须改为「除更新检查、用户主动触发的局域网扫描外无网络请求」，并在同表补一句「扫描目标仅限本机所在网段，不向公网发包」。**这一条不能留到发版前补**——它与 `project-review-2026-11-10.md` 已记录的 P1（M17 网络诊断已使该声明失真）是同一条，M18 会让偏差扩大 |

**回滚**：功能完全隔离在 `lan.rs` + `LanBoard.tsx` + 一个 tab 分段内。删掉这两处、去掉 `mdns-sd` 依赖与 `Info.plist` 即回到 M17 状态，不触碰任何已验证模块（`ports.rs` / `diag.rs` / `win_ports.rs` 均零改动）。
