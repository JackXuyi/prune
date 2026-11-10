# M18 局域网设备（D0–D4）

> 双平台：macOS 13+ / Windows 10 1809+。定位是「本机暴露面 × 同网段设备」，**不是网络扫描器**。
>
> 实现细节（结构体、函数签名、Windows API 调用、测试矩阵、提交顺序）见
> [m18-lan-devices-spec.md](./m18-lan-devices-spec.md)。

## 1. 结论与定位边界

技术上无阻塞点：邻居表、存活探测、服务发现三层数据源在两个平台上都**不需要提权**，
Windows 侧甚至在结构化程度上优于 macOS（`GetIpNetTable2` 直接给 MAC + 邻居状态，
不用解析命令输出）。

但必须先钉死边界，否则这个功能会从「资源守门员」滑向「Nmap 套壳」：

| ✅ 做 | ❌ 不做 |
|---|---|
| 列出同网段设备：IP / MAC / 厂商 / 类型 / 在线状态 | 端口爆破、弱口令探测、漏洞扫描 |
| 告诉用户「我的 3000 端口对这 N 台设备开放」 | 跨设备 / 路由器的流量归因（需 SNMP，README 已列不做） |
| 告诉用户「这台设备正在连我的哪些端口」 | 后台常驻扫描（只做手动一轮 + 30 s 服务监听） |
| 服务类型推断（AirPlay / 打印机 / NAS / 智能音箱） | 设备指纹识别到具体型号（不猜） |

**为什么值得做**：`ports.rs` 的 `BindScope::Any` 已经在说「同网段可达」，但用户不知道
同网段有谁。补上设备列表后，「对外暴露」从抽象属性变成可核对的事实——这是安全价值，
不是「看看家里有几台手机」的新鲜感。

## 2. 三层数据源

| 层 | 拿到什么 | macOS | Windows |
|---|---|---|---|
| L1 邻居表 | IP + MAC + 邻居状态 | `arp -an` 解析（实测本机 10 条） | `GetIpNetTable2`（结构化，含 `NL_NEIGHBOR_STATE`） |
| L2 服务发现 | 设备**类型** = 人话 | `mdns-sd`（mDNS）+ SSDP | 同左 + Windows 独有坑见 §3 |
| L3 存活扫描 | 全段在线设备（含从未通信过的） | TCP connect 并发 | TCP connect 并发 |

L1 单独用不够：ARP 表只有**最近通信过**的条目，冷启动时列表稀疏。L3 补全段，
且扫描行为本身会填充 ARP 缓存（见 §4 的 MAC 获取），两件事天然互补。

## 3. 平台差异（沿 `windows-support-plan.md` §7 范式）

| 项 | macOS | Windows |
|---|---|---|
| 邻居表数据源 | `arp -an`，纯文本解析（`parse_arp_output()` 纯函数，用实测输出做 fixture） | `GetIpNetTable2(AF_INET)`，`MIB_IPNET_ROW2` 直读 `PhysicalAddress` + `PhysicalAddressLength` |
| locale 风险 | 中英输出格式一致，风险低 | **必须用 API**：`arp -a` 在中文 Windows 输出「接口:」，解析必崩（与 `win.rs` 「用官方绑定不解析命令」同一理由） |
| 邻居状态 | `arp` 只有 `incomplete` / `permanent` 两个标记 | `NL_NEIGHBOR_STATE`：`Reachable` / `Stale` / `Delay` / `Probe` / `Incomplete` / `Permanent` |
| 网段 | `ifconfig <iface>` 解析 `netmask 0xfffffe00`（实测本机 = **/23**，不是 /24） | `GetAdaptersAddresses` 的 `OnLinkPrefixLength` |
| 服务发现 | 系统 `dns-sd` 可用亦可不用；`mdns-sd` 走 5353 多播，零权限 | **无系统 `dns-sd`**；`mdns-sd` 可用，但入站 UDP 受 Defender 防火墙影响（见下） |
| 存活探测 | TCP connect（标准库 `connect_timeout`） | 同左，**完全同构** |
| 权限 | **macOS 15+ 首次访问本地网络弹系统授权**；`Info.plist` 需 `NSLocalNetworkUsageDescription` + `NSBonjourServices` | 零提权、无系统授权；但监听 5353 首次可能触发防火墙入站提示 |
| 分发要求 | 直分发（GitHub release，非 MAS 沙箱）→ **不需要** `com.apple.developer.networking.multicast` | 无对应物 |

### 3.1 为什么不用 ICMP

- `windows-sys 0.61.2` **没有 `IcmpCreateFile` / `IcmpSendEcho`**（已核对源码，IpHelper 模块内不存在），
  要用就得手写 `extern "system"` + 自建 `ICMP_ECHO_REPLY` 结构——正是 `win.rs` 注释里
  明确要避免的那类「布局写错读出垃圾值」的风险。
- macOS 上 raw ICMP 需要 root（违反「永不提权」）；非特权 ICMP datagram socket 行为跨版本不稳。

→ **两端统一 TCP connect 探测**。代价：完全静默的主机（防火墙 DROP）会漏，靠 L1 + L2 补。
收益：一套代码、零 unsafe、零权限、零外部命令。

### 3.2 Windows 防火墙（首要风险）

Defender 默认阻止**入站** UDP。mDNS 靠收 5353 的响应包，可能被静默丢弃且**不报错**——
用户会看到「设备很少」而不是「被拦了」。处理方式：

1. SSDP（`M-SEARCH` → `239.255.255.250:1900`）作为 Windows 首选：响应是单播回包，
   状态化防火墙放行出站关联，通常不弹窗。
2. mDNS 结果为空时，`notes` 明确写「mDNS 无响应，可能被防火墙拦截」，**不假装局域网没有 Apple 设备**。
3. 不主动请求用户开放防火墙规则（写系统配置属破坏性动作）。

## 4. MAC 获取：不提权的曲线方案

主动 ARP 扫描要 raw socket / BPF → 需要 root，与 README 第 5 条「永不提权」直接冲突。
但 TCP connect 扫描时**内核会为目标地址自动填 ARP 缓存**（探测失败的条目状态为
`incomplete`）。所以：

```
probe_sweep()  →  逐地址 TCP connect（并发 32，超时 300 ms）
              →  扫完读一次邻居表（macOS arp / Windows GetIpNetTable2）
              →  MAC 有了，全程无提权
```

厂商识别用 OUI 前 3 字节查表：内置精简 CSV（只收常见厂商约 2k 行，不引在线查询、
不引第三方 crate）。查不到就 `vendor = null`，**不猜**。

## 5. 数据结构

`crates/prune-core/src/net/lan.rs`，serde camelCase ↔ `src/types/lan.ts`：

```rust
pub struct LanSnapshot {
    pub at: i64,
    /// 参与扫描的网卡（有默认路由那张）；多网卡时只取一张，其余进 notes
    pub iface: Option<String>,
    /// "192.168.0.0/23"
    pub cidr: Option<String>,
    /// 已探测 / 上限内的总数（cap 1024）；二者不等时 UI 必须显示「N/M」
    pub scanned: u32,
    pub total: u32,
    pub devices: Vec<LanDevice>,
    /// 本机对同网段可达的监听端口（复用 ports.rs 的 BindScope::Any，不重算）
    pub exposed_ports: Vec<u16>,
    pub notes: Vec<String>,
}

pub struct LanDevice {
    pub ip: String,
    pub mac: Option<String>,          // 大写冒号分隔；None = 邻居表无记录
    pub vendor: Option<String>,       // OUI 查表；None = 不猜
    pub hostname: Option<String>,     // mDNS/SSDP 得到；None = 不猜
    pub kind: DeviceKind,             // 由服务类型推断，推断不出就是 Unknown
    pub state: NeighborState,
    pub online: bool,                 // state 可达 或 TCP 探测成功
    pub services: Vec<String>,        // ["_airplay._tcp", "ipp"]
    /// 它正在连的我方端口（来自 lsof / GetExtendedTcpTable 的 ESTABLISHED 对端）
    pub inbound: Vec<u16>,
    pub is_gateway: bool,
    pub is_self: bool,
}
```

口径纪律（同 `mod.rs` 的「不撒谎」级别）：

1. **`mac = null` ≠ 「没有 MAC」**，是「邻居表里没有这条」。UI 显示 `—`。
2. **`vendor = null` 不显示「未知厂商」**，显示 `—`；「未知」会被误读成「查过但没查到」。
3. **`scanned < total` 必须显示**，不能静默截断（企业 /16 尤其）。
4. **在线判定不合并三个信号**：`state` 是内核视角，TCP 探测是应用视角，两者分开存。

## 6. 设备类型推断（不猜的边界）

| 证据 | 判定 |
|---|---|
| `_airplay._tcp` / `_raop._tcp` | Tv / Speaker（有 `_airplay` 且 `deviceid` 含 AppleTV → Tv） |
| `_ipp._tcp` / `_printer._tcp` / `_pdl-datastream` | Printer |
| `_smb._tcp` + `_afpovertcp._tcp` | Nas |
| `_googlecast._tcp` / `ssdp: MediaRenderer` | Tv |
| `ssdp: InternetGatewayDevice` 或 `is_gateway` | Router |
| `_companion-link._tcp`（无 AirPlay） | Phone / Tablet（区分不了，就报 Computer 侧不猜） |
| 只有 MAC、厂商是 Apple 且无服务 | Unknown + vendor=Apple |

**刻意不做**：靠 TTL / 端口开放组合做 OS 指纹。准确率不够，错判比不判更糟。

## 7. 拆解

### D0 邻居表（双平台，半天）
- `parse_arp_output(&str) -> Vec<RawNeighbor>` 纯函数 + 单测（fixture = 本机实测 10 行）
- `#[cfg(windows)] GetIpNetTable2` 封装进 `win.rs`（不新建 crate），返回统一 `RawNeighbor`
- `NeighborState` 双平台映射：`Reachable`/`Stale`/`Incomplete`/`Permanent` ↔ `arp` 两标记
- **验收**：macOS 上单测绿；`arp -an` 与函数输出逐条对齐

### D1 网段与探测（1 天）
- `local_cidr()`：macOS `ifconfig` 解析 / Windows `GetAdaptersAddresses`
- **虚拟网卡排除**：只认「有默认路由 + 网段含本机地址」那张；Hyper-V / WSL / Docker 的
  `172.17.x`、VPN 的虚拟口全部排除，避免用户看到一堆假设备（同 `traffic.rs` 只算物理网卡的原则）
- `probe_sweep(cidr, cap=1024)`：`connect_timeout` 300 ms，并发 32，扫完回读邻居表
- **验收**：本机 /23 = 510 地址，实测耗时 < 15 s；企业 /16 场景命中 cap 且 UI 明示

### D2 服务发现（1 天）
- 引 `mdns-sd`（纯 Rust，双平台）；SSDP 用标准库 `UdpSocket` 手写 `M-SEARCH`（约 60 行，不值得引 crate）
- 监听 30 s 后出快照，按 IP 合并进 L1 结果
- **验收**：macOS 实测能发现 `_airplay` / `_ipp` / `_scanner` / `_companion-link`（已在本机验证 4 s 内出现）

### D3 暴露面联动（半天）
- `exposed_ports` 直接取 `ports.rs` 已有快照里 `exposure == Any` 的端口，不重算
- `inbound` 从 ESTABLISHED 连接的对端 IP 反查（macOS `lsof` / Windows `GetExtendedTcpTable` 已有）
- **验收**：开一个 `0.0.0.0:3000` 的 dev server，从手机访问 → 对应设备卡上出现 `3000`

### D4 壳层与打包（1 天）
| 位置 | 改动 |
|---|---|
| `src-tauri/src/lan.rs` | `lan_snapshot()` / `lan_scan()` command，`spawn_blocking`；扫描进度用事件推（参照 `net_diag.rs`） |
| `src-tauri/src/lib.rs` | `generate_handler!` 注册两条命令 |
| `src-tauri/Info.plist` | 新建（当前无此文件）：`NSLocalNetworkUsageDescription` + `NSBonjourServices`（`_services._dns-sd._udp`、`_airplay._tcp`、`_ipp._tcp` 等实际浏览的类型） |
| `src-tauri/tauri.conf.json` | 确认 `src-tauri/Info.plist` 被合并进 bundle（Tauri 2 默认行为，需实测验证） |
| `src/panels/` + `components/LanBoard.tsx` + `stores/lanStore.ts` | 设备卡片：类型图标 / IP / MAC / 厂商 / 在线 / 服务标签 / 「连了我哪些端口」 |
| `crates/prune-cli` | `prune lan [--json]` |
| `crates/prune-cli/src/mcp.rs` | `lan_devices`（只读） |

**触发方式**：进 tab 不扫描，用户点「扫描局域网」才发包；mDNS 监听随 tab 可见性起停
（同「应用网速」只在可见时采样）。这是 Prune 第一次引入系统权限弹窗，不能让它在启动时蹦出来。

## 8. 验收清单（含真机走查）

macOS 侧可在本机完成；**Windows 侧必须在真机走查**（`cargo check` 只证明能编译）。

| # | 项 | macOS | Windows |
|---|---|---|---|
| 1 | 首次扫描触发本地网络授权弹窗，拒绝后 notes 给出去哪开 | 待测 | 不适用 |
| 2 | 邻居表设备数与 `arp -an` / `arp -a` 一致 | 本机 10 条 | 待测 |
| 3 | /23 网段完整扫描（510 地址） | 待测 | 待测 |
| 4 | 网关单独标记 Router | 待测 | 待测 |
| 5 | mDNS 发现 AirPlay / 打印机 | 已验证可发现 | 待测（防火风险） |
| 6 | 手机连本机 3000 后设备卡出现 inbound | 待测 | 待测 |
| 7 | Hyper-V / WSL / VPN 虚拟网段不出现在列表 | 不适用 | **重点** |
| 8 | 中文 Windows 下 MAC / 厂商不乱码（验证不解析命令输出） | 不适用 | **重点** |
| 9 | 断网 / 无默认路由时返回空快照 + note，不 panic | 待测 | 待测 |

## 9. 风险

- **macOS 授权弹窗是首次出现的权限裂缝**：README 卖点之一是「永不提权」。这里要区分
  「提权（root）」与「隐私授权（本地网络）」，README 措辞需同步改，否则自相矛盾。
- **Windows 防火墙导致 mDNS 静默为空**：只能靠 notes 说明，无法自动修复（不写系统配置）。
- **企业网 /16 扫不完**：cap 1024 是产品取舍，UI 必须明示「已扫 N/M」，不能给人「全网就这些」的错觉。
- **OUI 厂商表会过时**：内置快照，README 注明版本；查不到就 `null`，绝不猜厂商。
