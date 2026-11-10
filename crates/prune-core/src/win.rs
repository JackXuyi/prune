//! Windows 专属系统调用封装（仅 Windows 编译）。
//!
//! 为什么用 `windows-sys` 而不是手写 `extern "system"`：
//! `PROCESS_MEMORY_COUNTERS` 有 10 个字段，`MEMORYSTATUSEX` 有 7 个，
//! 手写 struct 一旦布局/对齐错位，读出来的是垃圾数值——而这种错误在 macOS 上
//! 永远测不出来（编译能过、类型也对）。用官方绑定换掉这份风险。
//!
//! 口径说明：这里只做「读」，不做任何「释放内存」动作。

use std::net::Ipv4Addr;

use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::NetworkManagement::IpHelper::{
    FreeMibTable, GetIpForwardTable2, MIB_IPFORWARD_TABLE2,
};
use windows_sys::Win32::Networking::WinSock::AF_INET;
use windows_sys::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

/// `GlobalMemoryStatusEx` 的结果（物理内存视角）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SysMemory {
    pub total: u64,
    /// `ullAvailPhys`：可用物理内存（已排除 standby/cached，可直接分配）
    pub available: u64,
    /// `dwMemoryLoad`：系统给出的物理内存占用百分比（0–100）
    pub load_pct: u32,
}

/// 读物理内存总量 / 可用量 / 占用率。
pub fn system_memory() -> Option<SysMemory> {
    // SAFETY: dwLength 必须由调用方填成结构体大小，否则 API 直接返回失败
    let mut st: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
    st.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
    let ok = unsafe { GlobalMemoryStatusEx(&mut st) };
    (ok != 0).then_some(SysMemory {
        total: st.ullTotalPhys,
        available: st.ullAvailPhys,
        load_pct: st.dwMemoryLoad,
    })
}

/// 进程私有提交字节（Private Bytes），对应任务管理器「提交大小」。
///
/// API 里的字段名是 `PagefileUsage`，语义就是 Private Bytes（提交内存）：
/// 含堆、栈、私有映射页。**不含已被换出到 pagefile 的部分**——Windows 没有
/// 等价的可读口径，因此 Windows 上 `swapped` 一律为 `None`，UI 隐藏换出读数。
pub fn private_bytes(pid: i32) -> Option<u64> {
    if pid <= 0 {
        return None;
    }
    // SAFETY: OpenProcess 失败返回空句柄；成功时下面一定 CloseHandle
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid as u32) };
    if handle.is_null() {
        return None;
    }

    let mut counters: PROCESS_MEMORY_COUNTERS = unsafe { std::mem::zeroed() };
    counters.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
    // SAFETY: 句柄有效、结构体在栈上且 cb 已填
    let ok = unsafe { K32GetProcessMemoryInfo(handle, &mut counters, counters.cb) };
    // SAFETY: 句柄由本函数独占持有，关闭一次
    unsafe { CloseHandle(handle) };

    (ok != 0).then_some(counters.PagefileUsage as u64)
}

/// IPv4 默认网关（网络诊断 §3.6 的 Windows 口）。
///
/// `GetIpForwardTable2` 遍历路由表取 `0.0.0.0/0`：
/// - 多条默认路由（多网卡）取 **Metric 最小**的那条——与系统实际出口一致；
/// - `NextHop` 为 0.0.0.0 的 on-link 路由没有网关，跳过；
/// - 返回 `None` = 没有可用默认网关（未联网 / 仅 IPv6）。
///
/// SAFETY 取舍：`NextHop` 是 `SOCKADDR_INET` union，官方绑定只有字段定义没有
/// 安全访问器。这里按标准 sockaddr_in ABI（family 2 B + port 2 B + addr 4 B）
/// 从 union 起始地址读第 4–7 字节拿 IPv4 八位组——比手解 union 字段路径
/// 更稳（绑定的字段名在 windows-sys 版本间变动过，ABI 不会）。
pub fn default_gateway() -> Option<String> {
    // SAFETY: GetIpForwardTable2 成功时 table 指向内核分配的表，
    // 用完必须 FreeMibTable（两条路径都会走到）。
    let mut table: *mut MIB_IPFORWARD_TABLE2 = std::ptr::null_mut();
    let rc = unsafe { GetIpForwardTable2(AF_INET, &mut table) };
    if rc != 0 || table.is_null() {
        return None;
    }

    let count = unsafe { (*table).NumEntries } as usize;
    let base = unsafe { (*table).Table.as_ptr() };
    let mut best: Option<(u32, Ipv4Addr)> = None;
    for i in 0..count {
        // SAFETY: i < NumEntries，行数组由内核按连续布局分配
        let row = unsafe { &*base.add(i) };
        if row.DestinationPrefix.PrefixLength != 0 {
            continue;
        }
        // SAFETY: si_family 是 union 的第一个字段（ADDRESS_FAMILY），读前 2 字节合法
        let family = unsafe { row.DestinationPrefix.Prefix.si_family };
        if family != AF_INET {
            continue;
        }
        // SAFETY: 按 sockaddr_in ABI 读 NextHop 起始的 16 字节（见函数注释）
        let hop_bytes: [u8; 16] =
            unsafe { std::ptr::read(std::ptr::addr_of!(row.NextHop) as *const [u8; 16]) };
        if hop_bytes[4..8] == [0, 0, 0, 0] {
            continue; // on-link 默认路由没有网关
        }
        let hop = Ipv4Addr::new(hop_bytes[4], hop_bytes[5], hop_bytes[6], hop_bytes[7]);
        let metric = row.Metric;
        if best.is_none_or(|(bm, _)| metric < bm) {
            best = Some((metric, hop));
        }
    }
    // SAFETY: 表指针有效且只释放一次
    unsafe { FreeMibTable(table.cast()) };
    best.map(|(_, ip)| ip.to_string())
}

// ---------------------------------------------------------------- M18 局域网设备
//
// docs/m18-lan-devices-spec.md §5.2 / §5.4 / §5.5。**只保证编译通过**，
// 功能正确性留给 W5 真机走查（spec §14 的 7 项走查清单，未验证前不得标记完成）。
//
// 常量口径已逐个核对 windows-sys 0.61.2 源码，不要凭记忆改：
// - GAA_FLAG_SKIP_ANYCAST = 2、GAA_FLAG_SKIP_DNS_SERVER = 8、
//   GAA_FLAG_INCLUDE_GATEWAYS = 128（不传它 FirstGatewayAddress 恒为 null）
// - 0.61.2 **没有导出** GAA_FLAG_SKIP_MULTICAST 和 IF_TYPE_* 常量，
//   IfType 只能用 IANA ifType MIB 的裸值（6=ethernet、71=ieee80211）
// - NL_NEIGHBOR_STATE：Unreachable=0 / Incomplete=1 / Probe=2 / Delay=3 /
//   Stale=4 / Reachable=5 / Permanent=6
// （Ipv4Addr / AF_INET / FreeMibTable 已在文件头部 use 过，不重复导入）

use windows_sys::Win32::NetworkManagement::IpHelper::{
    GetAdaptersAddresses, GetExtendedTcpTable, GetIpNetTable2, GAA_FLAG_INCLUDE_GATEWAYS,
    GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER, IP_ADAPTER_ADDRESSES_LH, MIB_IPNET_TABLE2,
    MIB_TCPROW_OWNER_PID, MIB_TCP_STATE_ESTAB, TCP_TABLE_OWNER_PID_CONNECTIONS,
};
use windows_sys::Win32::NetworkManagement::Ndis::IfOperStatusUp;
// AF_INET 已在文件头部 use 过，这里只补 NL_NEIGHBOR_STATE 六态常量
use windows_sys::Win32::Networking::WinSock::{
    NlnsDelay, NlnsPermanent, NlnsProbe, NlnsReachable, NlnsStale, NL_NEIGHBOR_STATE,
};

/// IANA ifType MIB 裸值（0.61.2 未导出常量）：以太网 / Wi-Fi
const IF_TYPE_ETHERNET: u32 = 6;
const IF_TYPE_IEEE80211: u32 = 71;

const ERROR_BUFFER_OVERFLOW: u32 = 111;
const ERROR_INSUFFICIENT_BUFFER: u32 = 122;

/// sockaddr → IPv4：按 sockaddr_in ABI 读 16 字节，IPv4 在 bytes[4..8]。
///
/// **不猜 union 字段名**（`SOCKADDR_INET` / `SOCKADDR_IN` 的字段在 windows-sys
/// 版本间变过；win.rs 既有纪律，同 `default_gateway` 的 NextHop 读法）。
/// 返回 None = 家族不是 AF_INET。
// SAFETY: `p` 必须指向至少 16 字节可读的合法 sockaddr
unsafe fn sockaddr_v4(p: *const core::ffi::c_void) -> Option<Ipv4Addr> {
    let raw: [u8; 16] = unsafe { std::ptr::read(p as *const [u8; 16]) };
    let family = u16::from_ne_bytes([raw[0], raw[1]]);
    (family == AF_INET as u16).then(|| Ipv4Addr::new(raw[4], raw[5], raw[6], raw[7]))
}

/// UTF-16 PWSTR → String（`FriendlyName` 是宽字符，不是 C 字符串）
// SAFETY: `p` 必须是系统填的合法 UTF-16 串（nul 结尾）
unsafe fn pwstr_to_string(p: *const u16) -> String {
    if p.is_null() {
        return String::new();
    }
    let mut len = 0usize;
    // SAFETY: 逐元素读到 nul 为止，系统串保证有 nul
    while unsafe { *p.add(len) } != 0 {
        len += 1;
    }
    let wide = unsafe { std::slice::from_raw_parts(p, len) };
    use std::os::windows::ffi::OsStringExt;
    std::ffi::OsString::from_wide(wide)
        .to_string_lossy()
        .into_owned()
}

/// 读 [`crate::net::lan::LanScope`]：有默认路由的物理网卡（spec §5.2）。
///
/// 四条筛选（全满足）：OperStatus=Up、IfType∈{6,71}、有非 169.254/16 的 IPv4
/// 单播地址、有默认网关——后两条天然排除 Hyper-V / WSL / Docker 的虚拟网卡
/// （它们没有网关）。多候选按 `Ipv4Metric` 取最小，其余候选由调用方在 notes 里说明。
pub fn lan_scope() -> Option<crate::net::lan::LanScope> {
    // retry 模式（MSDN 推荐）：先 16 KB，ERROR_BUFFER_OVERFLOW 时按返回 size 重试一次
    let flags = GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_DNS_SERVER | GAA_FLAG_INCLUDE_GATEWAYS;
    let mut size: u32 = 16 * 1024;
    let mut buf = vec![0u8; size as usize];
    // SAFETY: buf 按 size 分配；size 由系统更新
    let mut rc = unsafe {
        GetAdaptersAddresses(
            AF_INET as u32,
            flags,
            std::ptr::null(),
            buf.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH,
            &mut size,
        )
    };
    if rc == ERROR_BUFFER_OVERFLOW {
        let mut buf2 = vec![0u8; size as usize];
        // SAFETY: 同上，第二轮按系统要求的大小分配
        rc = unsafe {
            GetAdaptersAddresses(
                AF_INET as u32,
                flags,
                std::ptr::null(),
                buf2.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH,
                &mut size,
            )
        };
        buf = buf2;
    }
    if rc != 0 {
        return None;
    }

    let mut candidates: Vec<(u32, crate::net::lan::LanScope)> = Vec::new();
    // 链表头就是缓冲区起始处的第一个 adapter
    let mut p = buf.as_mut_ptr() as *const IP_ADAPTER_ADDRESSES_LH;
    while !p.is_null() {
        // SAFETY: p 是 GetAdaptersAddresses 返回链表的有效节点
        let ad = unsafe { &*p };
        p = ad.Next;
        if ad.OperStatus != IfOperStatusUp {
            continue;
        }
        if ad.IfType != IF_TYPE_ETHERNET && ad.IfType != IF_TYPE_IEEE80211 {
            continue;
        }
        if ad.FirstGatewayAddress.is_null() {
            continue;
        }
        // 遍历单播地址链表，取第一个非 link-local 的 IPv4 及其 prefix
        let mut ua = ad.FirstUnicastAddress;
        let mut found: Option<crate::net::lan::LanScope> = None;
        while !ua.is_null() {
            // SAFETY: ua 是单播地址链表的有效节点
            let u = unsafe { &*ua };
            ua = u.Next;
            let sa = u.Address.lpSockaddr;
            if sa.is_null() || u.Address.iSockaddrLength < 16 {
                continue;
            }
            let Some(ip) = (unsafe { sockaddr_v4(sa.cast()) }) else {
                continue;
            };
            if ip.is_link_local() || ip.is_loopback() {
                continue;
            }
            let iface = unsafe { pwstr_to_string(ad.FriendlyName) };
            found = Some(crate::net::lan::LanScope {
                iface,
                addr: ip,
                prefix: u.OnLinkPrefixLength,
            });
            break;
        }
        if let Some(scope) = found {
            candidates.push((ad.Ipv4Metric, scope));
        }
    }
    candidates.sort_by_key(|(m, _)| *m);
    candidates.into_iter().next().map(|(_, s)| s)
}

/// 读邻居表（spec §5.4）：`GetIpNetTable2` 结构化返回，**不解析 `arp -a`**
/// （中文 Windows 的输出是「接口: 192.168.0.115 --- 0x8」，文本解析必崩）。
///
/// 扫描后表里残留大量 `Incomplete` / `Unreachable` 条目（每个探测目标一条）——
/// 那是「扫描污染」不是设备，在源头过滤掉（spec §5.4 高风险点）。
pub fn arp_neighbors() -> Vec<crate::net::lan::RawNeighbor> {
    // SAFETY: 成功时 table 指向内核分配的表，两条路径都 FreeMibTable
    let mut table: *mut MIB_IPNET_TABLE2 = std::ptr::null_mut();
    let rc = unsafe { GetIpNetTable2(AF_INET, &mut table) };
    if rc != 0 || table.is_null() {
        return Vec::new();
    }

    let count = unsafe { (*table).NumEntries } as usize;
    let base = unsafe { (*table).Table.as_ptr() };
    let mut out = Vec::new();
    for i in 0..count {
        // SAFETY: i < NumEntries，行数组由内核按连续布局分配
        let row = unsafe { &*base.add(i) };
        // SAFETY: si_family 是 union 第一个字段，读前 2 字节合法（同 win.rs 范式）
        if unsafe { row.Address.si_family } != AF_INET {
            continue;
        }
        let Some(ip) = (unsafe { sockaddr_v4(std::ptr::addr_of!(row.Address).cast()) }) else {
            continue;
        };
        let state = map_state(row.State);
        if state == crate::net::lan::NeighborState::Incomplete {
            continue; // 扫描残留；有 MAC 的 Stale/Probe/Delay/Permanent 才是设备
        }
        let mac = (row.PhysicalAddressLength as usize >= 6).then(|| {
            [
                row.PhysicalAddress[0],
                row.PhysicalAddress[1],
                row.PhysicalAddress[2],
                row.PhysicalAddress[3],
                row.PhysicalAddress[4],
                row.PhysicalAddress[5],
            ]
        });
        out.push(crate::net::lan::RawNeighbor {
            ip,
            mac,
            iface: None,
            state,
        });
    }
    // SAFETY: 表指针有效且只释放一次
    unsafe { FreeMibTable(table.cast()) };
    out
}

/// `NL_NEIGHBOR_STATE` → [`crate::net::lan::NeighborState`]（不做六态以外的映射）
fn map_state(s: NL_NEIGHBOR_STATE) -> crate::net::lan::NeighborState {
    use crate::net::lan::NeighborState as N;
    match s {
        NlnsStale => N::Stale,
        NlnsDelay => N::Delay,
        NlnsProbe => N::Probe,
        NlnsReachable => N::Reachable,
        NlnsPermanent => N::Permanent,
        _ => N::Incomplete, // Unreachable(0) 与 Incomplete(1) 同归「未解析」
    }
}

/// ESTABLISHED 对端（spec §5.5）：`GetExtendedTcpTable(CONNECTIONS)`，
/// `dwLocalPort` 是 DWORD 里塞网络字节序的 16 位——忘了 `u16::from_be` 会把
/// 3000 读成 47115（W5 走查第 7 条的判据只认端口对得上）。
pub fn established_peers() -> Vec<(u16, Ipv4Addr)> {
    let mut size: u32 = 0;
    // SAFETY: 第一问大小（必得 INSUFFICIENT_BUFFER），与 win_ports::query 同模式
    let rc = unsafe {
        GetExtendedTcpTable(
            std::ptr::null_mut(),
            &mut size,
            0,
            AF_INET as u32,
            TCP_TABLE_OWNER_PID_CONNECTIONS,
            0,
        )
    };
    if rc != ERROR_INSUFFICIENT_BUFFER || size == 0 {
        return Vec::new();
    }
    let mut buf = vec![0u8; size as usize];
    // SAFETY: buf 按返回的 size 分配
    let rc = unsafe {
        GetExtendedTcpTable(
            buf.as_mut_ptr() as *mut core::ffi::c_void,
            &mut size,
            0,
            AF_INET as u32,
            TCP_TABLE_OWNER_PID_CONNECTIONS,
            0,
        )
    };
    if rc != 0 {
        return Vec::new();
    }
    buf.truncate(size as usize);

    // 表头 u32 NumEntries，其后紧跟 [MIB_TCPROW_OWNER_PID; 1]（4 字节对齐无填充，
    // read_unaligned 不依赖 Vec 的对齐假设——同 win_ports::rows 的口径）
    let n = if buf.len() >= 4 {
        u32::from_ne_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize
    } else {
        0
    };
    let stride = std::mem::size_of::<MIB_TCPROW_OWNER_PID>();
    let mut out = Vec::new();
    for i in 0..n {
        let off = 4 + i * stride;
        if off + stride > buf.len() {
            break;
        }
        // SAFETY: off..off+stride 在 buf 界内，read_unaligned 处理任意对齐
        let row: MIB_TCPROW_OWNER_PID =
            unsafe { std::ptr::read_unaligned(buf.as_ptr().add(off) as *const _) };
        // MIB_TCP_STATE 是 i32、dwState 是 u32（win_ports.rs 用自己的 u32 常量绕开，
        // 这里直接做一次有据可查的 as 转换：状态值全部非负）
        if row.dwState as i32 != MIB_TCP_STATE_ESTAB {
            continue;
        }
        let local_port = u16::from_be(row.dwLocalPort as u16);
        let remote = Ipv4Addr::from(row.dwRemoteAddr.to_ne_bytes());
        out.push((local_port, remote));
    }
    out
}
