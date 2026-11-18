//! 端口占用（M7）：把「谁占着 3000」变成一次可查找的索引。
//!
//! 定位见 docs/m7-port-plan.md §1.3：**端口不是第四个资源域，是进程的反查索引**。
//! 因此本模块只产出 `PortSnapshot`，不参与内存/存储口径，也不塞进 `MemSnapshot`
//! （两者采样频率不同，混在一个 payload 里必然出现时间戳错配）。
//!
//! 三条口径纪律（§3.2，都是「不撒谎」级别）：
//! 1. **「没查到」≠「端口空闲」**：非 root 的 lsof 对无权进程静默跳过，
//!    所以 `Coverage` 必须带上「内核说 N 个监听、我能归属 M 个」的差额。
//! 2. **TIME_WAIT 不是占用**：它没有持有者，不能渲染成「被占用」，更不能出现结束入口。
//! 3. **双栈不是两条记录**：同一 socket 在 v4/v6 各占一个 fd，必须按 `(port, proto)` 合并。
//!
//! 关注度分层（§3.3）是本模块的产品核心：实测本机 27 个监听端口里 19 个（70%）
//! 是 Electron / 微信 / Cursor 的内部 IPC 随机端口，全列出来等于把功能做废。

/// 基线漂移（F22，docs/f22-port-baseline-plan.md）：可信基线 + diff/merge 纯函数。
/// 只做查询态呈现（CLI / 行内徽标），不做灵动岛事件（路线图裁决红线）。
pub mod baseline;
/// 网络诊断（docs/net-diag-plan.md）：DNS / 网关 / 出口三层探测 + 分层判定
pub mod diag;
pub mod host;
/// M18 局域网设备（docs/m18-lan-devices-spec.md）：邻居表 / 网段展开 / 服务类型推断。
/// C1 只含纯函数；平台 IO 与服务发现在 C2/C4 接入
pub mod lan;
/// M19 设备时间线（docs/m19-lan-device-times-spec.md）：跨轮次身份 + 时间历史层。
pub mod lan_history;
/// M10：应用网速（nettop 累计字节差分）
pub mod netspeed;
pub mod ports;
/// 端口服务类型（docs/research/port-service-type-research.md）：
/// L0 静态推断（`service.rs`）+ L1 主动探针（`probe.rs`，用户显式触发）。
pub mod probe;
/// 暴露面可达实测（docs/f17-exposure-reach-plan.md，F17）：同机经网卡地址
/// 回连一次，四态分层。用户显式触发，无常驻轮询；只报观测事实不定因。
pub mod reach;
pub mod service;
/// 站点连通性 / 传输速率（docs/net-sites-plan.md，F15）：按站点测五阶段，
/// 与 `diag` 的「网络哪一层慢」互补，不共用判定表
pub mod sites;
/// M14：系统口径总网速 + 每日流量（网卡分类 / 本地日期 / 数据契约）
pub mod traffic;
/// 端口白话解释（docs/f16-port-why-plan.md，F16）：把快照证据翻译成
/// 一句人话 + 证据 + 指路。纯函数零 IO，CLI / MCP / 前端共用同一结论。
pub mod why;
/// Windows 走 iphlpapi 的 owner-pid 表，不依赖 lsof
#[cfg(windows)]
pub mod win_ports;

use serde::{Deserialize, Serialize};

/// 传输层。UDP 没有 LISTEN 状态，语义上只有「绑定」。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Proto {
    Tcp,
    Udp,
}

/// 绑定范围——这是「安全」语义的唯一来源，不是显示细节。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BindScope {
    /// 127.0.0.1 / ::1：只有本机可达
    Loopback,
    /// 绑定到某一具体网卡地址（192.168.x.x）
    Interface,
    /// * / 0.0.0.0 / [::]：同网段设备可达
    Any,
    /// 地址解析失败。UI 不标暴露面，也不猜。
    Unknown,
}

impl BindScope {
    /// 聚合多绑定时取**最宽**的面：只要有一个监听在 `*` 上，这个端口就对外可达。
    pub fn widest(self, other: Self) -> Self {
        fn rank(s: BindScope) -> u8 {
            match s {
                BindScope::Any => 3,
                BindScope::Interface => 2,
                BindScope::Loopback => 1,
                BindScope::Unknown => 0,
            }
        }
        if rank(other) > rank(self) {
            other
        } else {
            self
        }
    }

    /// 从 lsof / WinSock 的地址串判定绑定范围。
    pub fn from_addr(addr: &str) -> Self {
        match addr {
            "" | "*" | "0.0.0.0" | "::" | "[::]" | "::0" => BindScope::Any,
            "::1" | "[::1]" => BindScope::Loopback,
            a if a.starts_with("127.") => BindScope::Loopback,
            a if a.starts_with("169.254.") => BindScope::Interface,
            _ => BindScope::Interface,
        }
    }
}

/// 一条「进程 × 端口」绑定。一个端口可以有多个 binding（双栈 / SO_REUSEPORT）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortBinding {
    pub port: u16,
    pub proto: Proto,
    pub scope: BindScope,
    /// 原始绑定地址（`*` / `127.0.0.1` / `::1` / `192.168.0.115`）
    pub addr: String,
    /// 该 fd 是 IPv6 socket。双栈合并后靠它显示 `IPv4+IPv6`
    pub v6: bool,
    /// None = 内核持有（Windows 的 pid 0），不可结束
    pub pid: Option<i32>,
    /// 进程命令名（lsof 的 `c` 行直出，零额外成本）
    pub comm: String,
    /// 启动时间（防 pid 复用串台，规则同 merge_deep）
    #[serde(default)]
    pub start_time: u64,
    /// cwd 归属的项目目录（绝对路径）；None = 无项目线索（GUI 应用 / 系统进程）
    pub project: Option<String>,
    /// 项目展示名
    pub project_label: Option<String>,
}

/// 按 `(port, proto)` 聚合后的一行。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortEntry {
    pub port: u16,
    pub proto: Proto,
    pub listeners: Vec<PortBinding>,
    /// 该端口的 ESTABLISHED 连接数；None = 本轮未统计（UDP / 采集失败）
    pub conns: Option<u32>,
    /// 聚合后的暴露面（listeners 中最宽者）
    pub exposure: BindScope,
    /// 是否进「关注」层（§3.3）。false → 折进「其他 N 个」
    pub watched: bool,
    /// 内置解释（目前只有 AirPlay 5000/7000）。有就显示，没有就不占位。
    pub note: Option<String>,
    /// 服务类型（L0 静态推断）。None = 推断不出，**不是「不是服务」**。
    /// L1 探针（`probe.rs`）的确认结果不落这里——它是快照外的即时数据，
    /// 由前端按 `(pid, port, start_time)` 叠加显示。
    #[serde(default)]
    pub service: Option<service::ServiceKind>,
    /// 入站来源摘要（F21，docs/f21-inbound-source-plan.md）。None = 本端口
    /// 没有 ESTABLISHED 连接或本轮未统计；语义是「没有记录 = 0 条」。
    /// 只统计「本端是监听端口」的连接（连出去的本端是临时端口，天然过滤）。
    #[serde(default)]
    pub inbound: Option<PortPeers>,
    /// 基线漂移徽标（F22，docs/f22-port-baseline-plan.md）。None = 无漂移 /
    /// 基线还没建 / 非采样轮路径（CLI 的 `scan()` 不带基线，漂移由 `--drift` 单独算）。
    /// 判定在核心层 `baseline::diff` 算好随轮下发，前端不持基线。
    #[serde(default)]
    pub drift: Option<baseline::DriftKind>,
}

impl PortEntry {
    /// IPv4 与 IPv6 是否同时存在（双栈）。UI 据此标 `IPv4+IPv6`。
    pub fn dual_stack(&self) -> bool {
        self.listeners.iter().any(|b| b.v6) && self.listeners.iter().any(|b| !b.v6)
    }

    /// 去重后的持有进程数（SO_REUSEPORT 时会 >1）
    pub fn owner_count(&self) -> usize {
        let mut pids: Vec<i32> = self.listeners.iter().filter_map(|b| b.pid).collect();
        pids.sort_unstable();
        pids.dedup();
        pids.len()
    }

    /// 主展示进程：第一个有 pid 的 binding
    pub fn primary(&self) -> Option<&PortBinding> {
        self.listeners.iter().find(|b| b.pid.is_some())
    }
}

/// 单个进程的 TCP 连接计数（M8 应用维度）。
///
/// 只看两个状态，别的状态对「这个应用连了多少东西」没有解释力：
/// - `ESTABLISHED`：活跃连接，用户可感知的「在联着」
/// - `CLOSE_WAIT`：本端还没关，往往意味着对端已关而程序没释放 fd（泄漏信号，不是结论）
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnStats {
    pub established: u32,
    pub close_wait: u32,
}

impl ConnStats {
    pub fn is_empty(&self) -> bool {
        self.established == 0 && self.close_wait == 0
    }
}

// ───────────────────────── 入站来源（F21） ─────────────────────────

/// 连接对端的来源段分类（F21）。**只分类，不定因**——不做反解析、不发包，
/// 全部判据来自地址字面值（lsof 以 `-n` 数字输出，无 DNS 依赖）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PeerScope {
    /// 127.0.0.0/8、::1 —— 本机自己
    Loopback,
    /// RFC1918（10/8、172.16/12、192.168/16）+ 链路本地（169.254/16、fe80::/10）
    /// + IPv6 ULA（fc00::/7）—— 局域网 / 内网
    Lan,
    /// 其余可解析地址 —— 公网。对 any 暴露面这是正常态，不是攻击信号
    Public,
    /// 字面值解析失败（不应出现，出现就如实标，不塞进别的段谎报）
    Unknown,
}

impl PeerScope {
    /// 地址字面值 → 来源段。判据表集中在此处，测试对表。
    pub fn from_addr(addr: &str) -> Self {
        let a = addr.trim_start_matches('[').trim_end_matches(']');
        // IPv6 先判（含 ':' 的都是 v6 字面量）
        if a.contains(':') {
            let la = a.to_ascii_lowercase();
            if la == "::1" || la == "::" {
                return PeerScope::Loopback;
            }
            if la.starts_with("fe8")
                || la.starts_with("fe9")
                || la.starts_with("fea")
                || la.starts_with("feb")
            {
                return PeerScope::Lan; // 链路本地 fe80::/10
            }
            if la.starts_with("fc") || la.starts_with("fd") {
                return PeerScope::Lan; // ULA fc00::/7
            }
            return PeerScope::Public;
        }
        // IPv4
        let octets: Vec<u32> = a.split('.').filter_map(|s| s.parse().ok()).collect();
        if octets.len() != 4 {
            return PeerScope::Unknown;
        }
        match octets[0] {
            127 => PeerScope::Loopback,
            10 => PeerScope::Lan,
            192 if octets[1] == 168 => PeerScope::Lan,
            172 if (16..=31).contains(&octets[1]) => PeerScope::Lan,
            169 if octets[1] == 254 => PeerScope::Lan,
            _ => PeerScope::Public,
        }
    }

    pub fn label_cn(self) -> &'static str {
        match self {
            PeerScope::Loopback => "本机",
            PeerScope::Lan => "局域网",
            PeerScope::Public => "公网",
            PeerScope::Unknown => "未知",
        }
    }
}

/// 单条入站来源明细。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerEntry {
    /// 对端地址（数字字面量，无反解析）
    pub addr: String,
    pub scope: PeerScope,
}

/// 一个监听端口的入站来源摘要（F21）。
///
/// 计数是口径（压测场景一个端口几百连入），明细是给人看的——截断 32 条，
/// 余量由 `total - detail.len()` 表达，不谎称「全部」。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortPeers {
    pub loopback: u32,
    pub lan: u32,
    pub public: u32,
    pub unknown: u32,
    /// 对端明细（去重，≤32 条）。空 = 只有计数没有细节（不应出现）
    pub detail: Vec<PeerEntry>,
}

impl PortPeers {
    /// 对端总数（各段计数之和，含明细截断掉的部分）
    pub fn total(&self) -> u32 {
        self.loopback + self.lan + self.public + self.unknown
    }

    /// 明细之外的余量（`+N more` 用）
    pub fn more(&self) -> u32 {
        self.total().saturating_sub(self.detail.len() as u32)
    }

    /// 记录一条对端：计数 +1；明细未满 32 条且地址未出现过则追加
    pub fn record(&mut self, addr: String, scope: PeerScope) {
        match scope {
            PeerScope::Loopback => self.loopback += 1,
            PeerScope::Lan => self.lan += 1,
            PeerScope::Public => self.public += 1,
            PeerScope::Unknown => self.unknown += 1,
        }
        if self.detail.len() < 32 && self.detail.iter().all(|p| p.addr != addr) {
            self.detail.push(PeerEntry { addr, scope });
        }
    }
}

/// 可见性说明（§3.2.1）。这是「不撒谎」的载体——没有它就只能猜。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Coverage {
    /// 内核口径的 LISTEN 条数（netstat / GetTcpTable）。0 = 未采集到
    pub listen_total: usize,
    /// 其中能归属到进程的条数（lsof 拿到的）
    pub attributed: usize,
    /// 非空 = 覆盖面受限，UI 必须显示这句话
    pub note: Option<String>,
}

impl Coverage {
    /// 差额 = 内核有、但本进程看不到的监听数。
    pub fn invisible(&self) -> usize {
        self.listen_total.saturating_sub(self.attributed)
    }
}

/// 一次端口采样的完整产出。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortSnapshot {
    /// 按 port 升序
    pub entries: Vec<PortEntry>,
    pub coverage: Coverage,
    /// 内核 TIME_WAIT 条数；None = 未采集（平台不支持 / 命令缺失）
    pub time_wait: Option<u32>,
    /// TIME_WAIT 涉及的本地端口（去重，升序，上限 128）。用于解释「EADDRINUSE 但没进程」
    pub time_wait_ports: Vec<u16>,
    /// 每进程的连接计数（M8）：端口域与应用维度的共享底座。
    /// 只含「有 ESTABLISHED / CLOSE_WAIT」的进程——没有连接的进程不出现在这里，
    /// 语义是「没有记录 = 0 条」，而不是「不知道」。
    #[serde(default)]
    pub conns_by_pid: std::collections::BTreeMap<i32, ConnStats>,
    /// 项目 `prune.toml` 声明的关注端口并集（W2/D-09，旧快照缺省为空）。
    /// UI 用它显示「配置了关注但当前空闲」的端口——空闲是状态变化，不是没有
    #[serde(default)]
    pub configured_watch_ports: Vec<u16>,
    pub at: i64,
    pub scan_ms: u64,
}

impl PortSnapshot {
    /// 无数据但必须可解释的空快照（命令缺失 / 权限全无）。
    pub fn unavailable(note: &str, scan_ms: u64) -> Self {
        Self {
            entries: Vec::new(),
            coverage: Coverage {
                listen_total: 0,
                attributed: 0,
                note: Some(note.to_string()),
            },
            time_wait: None,
            time_wait_ports: Vec::new(),
            conns_by_pid: std::collections::BTreeMap::new(),
            configured_watch_ports: Vec::new(),
            at: 0,
            scan_ms,
        }
    }

    /// 一组 pid 的连接汇总（应用维度用：家族成员 pid 求和）。
    /// 每次都归零重算，避免把上一轮的数字累积进来。
    pub fn conns_of(&self, pids: &[i32]) -> ConnStats {
        let mut out = ConnStats::default();
        for pid in pids {
            if let Some(c) = self.conns_by_pid.get(pid) {
                out.established += c.established;
                out.close_wait += c.close_wait;
            }
        }
        out
    }

    pub fn watched_count(&self) -> usize {
        self.entries.iter().filter(|e| e.watched).count()
    }

    pub fn folded_count(&self) -> usize {
        self.entries.iter().filter(|e| !e.watched).count()
    }
}

// ---------------------------------------------------------------- 关注度规则

/// 开发者真正会问「谁占着这个端口」的端口清单。
///
/// 只放**有明确语义**的端口号；不放 `/etc/services` 全表——那会输出
/// 「http-alt」这类对开发者毫无用处的名字，还不如一个精确的表。
pub const WATCHED_PORTS: &[u16] = &[
    // 前端 / 静态服务
    3000, 3001, 3002, 3003, 4200, 4321, 5173, 5174, 5175, 6006, 7000, 7001, 5000, 5001, 8000, 8001,
    8002, 8080, 8081, 8082, 8888, 9000, 9090, 4173, // 数据库 / 中间件
    3306, 5432, 6379, 27017, 27018, 9200, 9300, 11211, 5672, 15672, 9092, 2181,
    // 代理 / 容器 / 调试
    1080, 1087, 2375, 2376, 50051, 9229, 9230,
];

/// macOS 上把 5000/7000 抢走的系统组件（AirPlay 接收器）。
/// 这是 Django / Flask 开发者最经典的「端口凭空被占」，必须给出可操作的出路。
const AIRPLAY_PORTS: &[u16] = &[5000, 7000];

/// 系统关键进程：它们的端口是事实，但不是「用户的开发服务」，默认折叠进「其他」。
///
/// 与 M5 §7 的禁杀名单同源（那边管「不许结束」，这边管「不占关注层」）。
const SYSTEM_COMMS: &[&str] = &[
    "ControlCenter",
    "rapportd",
    "sharingd",
    "mDNSResponder",
    "launchd",
    "configd",
    "netbiosd",
    "sshd",
    "cupsd",
    "rpcbind",
    "SystemUIServer",
    "WindowServer",
    "identityservicesd",
    "remoted",
    "AirPlayUIAgent",
    "svchost.exe",
    "System",
    "lsass.exe",
];

pub fn is_system_process(comm: &str) -> bool {
    SYSTEM_COMMS.iter().any(|s| s.eq_ignore_ascii_case(comm))
}

/// 是否进关注层（§3.3 规则表）：
/// 1. 特权端口（`< 1024`）——系统服务，冲突时最需要解释；
/// 2. 命中 `WATCHED_PORTS`；
/// 3. 绑定在全部网卡且不是系统进程——「暴露面」本身就是一个信号；
/// 4. 能归属到某个项目目录——dev server 的 cwd 就是项目根，这类端口就是用户的开发服务
///    （实测：1430 → prune、3118 → meetily/frontend；没有这条规则它们会被折叠掉，
///    而用户装 Prune 最想看的恰恰是这些）；
/// 5. 项目 `prune.toml` 的 `watch.ports`（W2/D-09）——用户写进配置的关注端口，
///    即使不命中任何内置规则也进关注层。
pub fn is_watched(port: u16, scope: BindScope, comms: &[&str], has_project: bool) -> bool {
    is_watched_with(port, scope, comms, has_project, &[])
}

/// 同上，`extra` 是项目配置声明的关注端口（W2/D-09）。
pub fn is_watched_with(
    port: u16,
    scope: BindScope,
    comms: &[&str],
    has_project: bool,
    extra: &[u16],
) -> bool {
    if port == 0 {
        return false;
    }
    if port < 1024 || WATCHED_PORTS.contains(&port) || extra.contains(&port) {
        return true;
    }
    if scope == BindScope::Any && !comms.iter().any(|c| is_system_process(c)) {
        return true;
    }
    // 平台无关兜底（服务类型调研 §2）：运行时解释器进程监听的 loopback 高位端口
    // 极可能是 dev server。Windows 无 cwd → has_project 恒空，没有这条
    // uvicorn :8765 这类非标端口服务会被错误折叠。多看一条无害：
    // 折叠规则的本意是压 Electron / 微信类 IPC 噪音，解释器进程不属于该类。
    if scope == BindScope::Loopback
        && port >= 1024
        && comms.iter().any(|c| service::is_runtime_comm(c))
    {
        return true;
    }
    has_project
}

/// AirPlay 端口的解释文案。命中才返回，避免每行都挂一句没用的话。
pub fn port_note(port: u16, comm: &str) -> Option<String> {
    if AIRPLAY_PORTS.contains(&port) && comm.eq_ignore_ascii_case("ControlCenter") {
        Some("AirPlay 接收器 · 系统设置 → 通用 → 隔空投送与接力 可关闭".into())
    } else {
        None
    }
}

// ---------------------------------------------------------------- 聚合

/// 把扁平的 binding 列表聚合成端口行：合并双栈、算连接数、打关注度标记。
///
/// `conns` 是 `(port, established 数)`，由调用方从全 TCP 轮传入（可选）。
pub fn group(bindings: Vec<PortBinding>, conns: &[(u16, u32)]) -> Vec<PortEntry> {
    group_with_watched(bindings, conns, &[])
}

/// 同上，`extra_watched` 是项目 `prune.toml` 声明的关注端口（W2/D-09）。
pub fn group_with_watched(
    bindings: Vec<PortBinding>,
    conns: &[(u16, u32)],
    extra_watched: &[u16],
) -> Vec<PortEntry> {
    use std::collections::BTreeMap;

    let mut map: BTreeMap<(u16, Proto), Vec<PortBinding>> = BTreeMap::new();
    for b in bindings {
        // 端口 0（`*:*`）是无绑定端口的通配 fd，不是服务，直接丢
        if b.port == 0 {
            continue;
        }
        map.entry((b.port, b.proto)).or_default().push(b);
    }

    let mut out: Vec<PortEntry> = Vec::with_capacity(map.len());
    for ((port, proto), mut listeners) in map {
        // 同一 pid 在双栈 / 多 fd 下会重复出现：按 (pid, addr) 去重后仍保留
        // v4 与 v6 两条（它们地址不同），但丢掉完全重复的记录。
        listeners.sort_by(|a, b| (a.pid, &a.addr, a.v6).cmp(&(b.pid, &b.addr, b.v6)));
        listeners.dedup_by(|a, b| a.pid == b.pid && a.addr == b.addr && a.v6 == b.v6);

        let exposure = listeners
            .iter()
            .fold(BindScope::Unknown, |acc, b| acc.widest(b.scope));
        let comms: Vec<&str> = listeners.iter().map(|b| b.comm.as_str()).collect();
        let has_project = listeners.iter().any(|b| b.project.is_some());
        let watched = is_watched_with(port, exposure, &comms, has_project, extra_watched);
        let note = listeners.iter().find_map(|b| port_note(port, &b.comm));

        out.push(PortEntry {
            port,
            proto,
            listeners,
            conns: conns
                .iter()
                .find(|(p, _)| *p == port)
                .map(|(_, n)| *n)
                .filter(|_| proto == Proto::Tcp),
            exposure,
            watched,
            note,
            // 推断需要 exe/cmd（group 只有 comm），由 scan_cached 按 ProcIdentity 回填
            service: None,
            // 入站来源由 scan() 按 tcp_scan.peers 回填（F21）
            inbound: None,
            // 基线漂移由采样轮按 baseline::diff 回填（F22）；CLI 路径保持 None
            drift: None,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bind(port: u16, addr: &str, pid: i32, comm: &str, v6: bool) -> PortBinding {
        PortBinding {
            port,
            proto: Proto::Tcp,
            scope: BindScope::from_addr(addr),
            addr: addr.to_string(),
            v6,
            pid: Some(pid),
            comm: comm.to_string(),
            start_time: 0,
            project: None,
            project_label: None,
        }
    }

    #[test]
    fn dual_stack_merges_into_one_entry() {
        // 实测形态：5000 在 v4 与 v6 各一条 fd，必须合并成一行
        let entries = group(
            vec![
                bind(5000, "127.0.0.1", 629, "ControlCenter", false),
                bind(5000, "::1", 629, "ControlCenter", true),
            ],
            &[],
        );
        assert_eq!(entries.len(), 1);
        assert!(entries[0].dual_stack());
        assert_eq!(entries[0].owner_count(), 1);
    }

    #[test]
    fn port_zero_wildcard_is_dropped() {
        // 实测 4 条 `*:*`（无绑定端口的 UDP 通配 fd）不属于任何服务
        let entries = group(vec![bind(0, "*", 1, "launchd", false)], &[]);
        assert!(entries.is_empty());
    }

    #[test]
    fn reuseport_keeps_both_owners() {
        let entries = group(
            vec![
                bind(8080, "*", 100, "node", false),
                bind(8080, "127.0.0.1", 200, "node", false),
            ],
            &[(8080, 7)],
        );
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].owner_count(), 2);
        assert_eq!(entries[0].conns, Some(7));
        // 有一条绑在 * 上 → 整个端口的暴露面是全开
        assert_eq!(entries[0].exposure, BindScope::Any);
    }

    /// W2/D-09：prune.toml 的 watch.ports 并入关注层——非标端口也要能被用户点名
    #[test]
    fn config_watch_ports_enter_watched_layer() {
        // 不带配置：7777 不在内置关注表里，「无项目 + 非 runtime 进程名」是纯反例
        assert!(!is_watched(7777, BindScope::Loopback, &["SomeApp"], false));
        assert!(is_watched_with(
            7777,
            BindScope::Loopback,
            &["SomeApp"],
            false,
            &[7777]
        ));
        // 0 号端口仍然无效：配置写错不能把通配 fd 变成服务
        assert!(!is_watched_with(0, BindScope::Any, &["node"], true, &[0]));
    }

    #[test]
    fn watched_rules() {
        // 常见开发端口
        assert!(is_watched(3000, BindScope::Loopback, &["node"], false));
        // 特权端口
        assert!(is_watched(22, BindScope::Any, &["sshd"], false));
        // 全部网卡的非系统进程：暴露面本身是信号
        assert!(is_watched(3118, BindScope::Any, &["node"], false));
        // 有项目归属：就是用户的 dev server（否则 1430 → prune 会被折叠）
        assert!(is_watched(1430, BindScope::Loopback, &["node"], true));
        // 随机高位内部 IPC、无项目、非全开：折叠
        assert!(!is_watched(
            50272,
            BindScope::Loopback,
            &["Cursor Helper"],
            false
        ));
        // 运行时解释器进程的 loopback 高位端口：进关注层（Windows 无 cwd 的兜底）
        assert!(is_watched(
            8765,
            BindScope::Loopback,
            &["python3.11"],
            false
        ));
        assert!(is_watched(8766, BindScope::Loopback, &["node.exe"], false));
        // 同端口但进程不是解释器：仍折叠
        assert!(!is_watched(
            8765,
            BindScope::Loopback,
            &["Cursor Helper"],
            false
        ));
        // 系统进程占着全部网卡：折叠（否则每个 Mac 都会看到 rapportd）
        assert!(!is_watched(58578, BindScope::Any, &["rapportd"], false));
        // 端口 0 永远不关注
        assert!(!is_watched(0, BindScope::Any, &["node"], true));
    }

    #[test]
    fn airplay_note_only_for_controlcenter() {
        assert!(port_note(5000, "ControlCenter").is_some());
        assert!(port_note(5000, "python3").is_none());
        assert!(port_note(3000, "ControlCenter").is_none());
    }

    #[test]
    fn scope_parsing() {
        assert_eq!(BindScope::from_addr("*"), BindScope::Any);
        assert_eq!(BindScope::from_addr("0.0.0.0"), BindScope::Any);
        assert_eq!(BindScope::from_addr("::"), BindScope::Any);
        assert_eq!(BindScope::from_addr("127.0.0.1"), BindScope::Loopback);
        assert_eq!(BindScope::from_addr("::1"), BindScope::Loopback);
        assert_eq!(BindScope::from_addr("192.168.0.115"), BindScope::Interface);
        // 取最宽
        assert_eq!(BindScope::Loopback.widest(BindScope::Any), BindScope::Any);
        assert_eq!(BindScope::Any.widest(BindScope::Loopback), BindScope::Any);
    }

    #[test]
    fn coverage_invisible_count() {
        let c = Coverage {
            listen_total: 30,
            attributed: 27,
            note: Some("3 个不可见".into()),
        };
        assert_eq!(c.invisible(), 3);
        // 反向（lsof 比 netstat 多）不能下溢成天文数字
        let c2 = Coverage {
            listen_total: 2,
            attributed: 5,
            note: None,
        };
        assert_eq!(c2.invisible(), 0);
    }

    #[test]
    fn peer_scope_from_addr_table() {
        // 本机
        assert_eq!(PeerScope::from_addr("127.0.0.1"), PeerScope::Loopback);
        assert_eq!(PeerScope::from_addr("::1"), PeerScope::Loopback);
        // 局域网 / 内网
        assert_eq!(PeerScope::from_addr("192.168.1.8"), PeerScope::Lan);
        assert_eq!(PeerScope::from_addr("10.0.0.2"), PeerScope::Lan);
        assert_eq!(PeerScope::from_addr("172.16.0.1"), PeerScope::Lan);
        assert_eq!(PeerScope::from_addr("169.254.7.7"), PeerScope::Lan);
        assert_eq!(PeerScope::from_addr("fe80::1"), PeerScope::Lan);
        assert_eq!(PeerScope::from_addr("fd00::5"), PeerScope::Lan);
        // 172 只在 16..=31 段是内网
        assert_eq!(PeerScope::from_addr("172.32.0.1"), PeerScope::Public);
        // 公网
        assert_eq!(PeerScope::from_addr("203.0.113.9"), PeerScope::Public);
        // 解析失败如实标 Unknown，不塞进别的段谎报
        assert_eq!(PeerScope::from_addr("not-an-ip"), PeerScope::Unknown);
    }

    #[test]
    fn port_peers_counts_caps_and_dedups_detail() {
        let mut pp = PortPeers::default();
        // 先记本机（明细未满，地址能进明细），再灌满局域网
        pp.record("127.0.0.1".into(), PeerScope::Loopback);
        for i in 0..40u8 {
            pp.record(format!("10.0.0.{i}"), PeerScope::Lan);
        }
        assert_eq!(pp.lan, 40);
        assert_eq!(pp.loopback, 1);
        // 明细封顶 32 条，余量可算（不谎称「全部」）
        assert_eq!(pp.detail.len(), 32);
        assert_eq!(pp.total(), 41);
        assert_eq!(pp.more(), 9);
        // 同地址重复：计数 +1，明细不重复
        pp.record("127.0.0.1".into(), PeerScope::Loopback);
        assert_eq!(pp.loopback, 2);
        assert_eq!(
            pp.detail.iter().filter(|p| p.addr == "127.0.0.1").count(),
            1
        );
    }
}
