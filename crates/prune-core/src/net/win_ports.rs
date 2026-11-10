//! Windows 端口采集：`GetExtendedTcpTable` / `GetExtendedUdpTable`（iphlpapi）。
//!
//! 与 macOS 的差别（docs/m7-port-plan.md §4.1）：
//! - **不需要提权**：Windows 的 owner-pid 表对所有进程可见，不存在 macOS 那种
//!   「非 root 看不到别人」的静默缺失，所以 `coverage.note` 在 Windows 上基本恒为 None；
//! - 端口字段是**网络字节序**（`dwLocalPort` 低 16 位），直接 `as u16` 会得到 47104 这类鬼数字；
//! - UDP 有独立表（`UDP_TABLE_OWNER_PID`），没有 LISTEN 概念。
//!
//! 结构体一律用 `windows-sys` 的官方定义，不手写布局——布局写错的后果是读出垃圾数值，
//! 而这种错误在 macOS 上永远测不出来（同 `win.rs` 的取舍）。

use std::net::{Ipv4Addr, Ipv6Addr};

use windows_sys::Win32::Foundation::{ERROR_INSUFFICIENT_BUFFER, NO_ERROR};
use windows_sys::Win32::NetworkManagement::IpHelper::{
    GetExtendedTcpTable, GetExtendedUdpTable, MIB_TCP6ROW_OWNER_PID, MIB_TCPROW_OWNER_PID,
    MIB_UDP6ROW_OWNER_PID, MIB_UDPROW_OWNER_PID, TCP_TABLE_OWNER_PID_ALL, UDP_TABLE_OWNER_PID,
};
use windows_sys::Win32::Networking::WinSock::{AF_INET, AF_INET6};

use super::ports::{KernelTcp, TcpScan};
use super::{BindScope, PortBinding, Proto};

/// `MIB_TCP_STATE_*` 里本模块关心的四个
const STATE_LISTEN: u32 = 2;
const STATE_ESTAB: u32 = 5;
const STATE_CLOSE_WAIT: u32 = 8;
const STATE_TIME_WAIT: u32 = 11;

/// 通用「两次调用」模式：先问大小（必得 INSUFFICIENT_BUFFER），再取数据。
unsafe fn query<F>(mut call: F) -> Result<Vec<u8>, String>
where
    F: FnMut(*mut core::ffi::c_void, *mut u32) -> u32,
{
    let mut size: u32 = 0;
    let rc = call(std::ptr::null_mut(), &mut size);
    if rc != ERROR_INSUFFICIENT_BUFFER && rc != NO_ERROR {
        return Err(format!("iphlpapi 查询大小失败：rc={rc}"));
    }
    // 空表（比如没有任何监听）会直接成功且 size=0
    if size == 0 {
        return Ok(Vec::new());
    }
    let mut buf = vec![0u8; size as usize];
    let rc = call(buf.as_mut_ptr() as *mut core::ffi::c_void, &mut size);
    if rc != NO_ERROR {
        return Err(format!("iphlpapi 读取表失败：rc={rc}"));
    }
    buf.truncate(size as usize);
    Ok(buf)
}

/// 读取行数组：表头是 u32 `dwNumEntries`，其后紧跟 `[Row; 1]`。
///
/// 偏移固定为 4——两种 Row 都是 4 字节对齐的 `repr(C)` 结构，所以表头后
/// 没有填充；用 `read_unaligned` 是为了不依赖 `Vec<u8>` 的对齐假设。
unsafe fn rows<T: Copy>(buf: &[u8]) -> Vec<T> {
    if buf.len() < 4 {
        return Vec::new();
    }
    let count = u32::from_ne_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
    let row_size = std::mem::size_of::<T>();
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let off = 4 + i * row_size;
        if off + row_size > buf.len() {
            break;
        }
        out.push(std::ptr::read_unaligned(buf.as_ptr().add(off) as *const T));
    }
    out
}

/// DWORD 里的端口是网络序，放在低 16 位。
fn port_of(dw: u32) -> u16 {
    u16::from_be(dw as u16)
}

fn binding(port: u16, addr: &str, v6: bool, pid: u32) -> PortBinding {
    PortBinding {
        port,
        proto: Proto::Tcp,
        scope: BindScope::from_addr(addr),
        addr: addr.to_string(),
        v6,
        pid: if pid == 0 { None } else { Some(pid as i32) },
        // 进程名由上层用 sysinfo 的进程表补齐：本模块不重复枚举进程，
        // 避免每次端口扫描都付一次 new_all 的代价。
        comm: String::new(),
        start_time: 0,
        project: None,
        project_label: None,
    }
}

pub fn scan_tcp() -> Result<TcpScan, String> {
    let mut scan = TcpScan::default();
    let mut conns: std::collections::HashMap<u16, u32> = std::collections::HashMap::new();

    // 按 pid 计数：Windows 的 owner-pid 表一行就是一个连接，直接累加（与 mac 分支等价）
    fn bump(scan: &mut TcpScan, pid: u32, state: u32) {
        if pid == 0 {
            return;
        }
        let s = scan.conns_by_pid.entry(pid as i32).or_default();
        if state == STATE_ESTAB {
            s.established += 1;
        } else if state == STATE_CLOSE_WAIT {
            s.close_wait += 1;
        }
    }

    unsafe {
        // IPv4
        let buf =
            query(|p, s| GetExtendedTcpTable(p, s, 0, AF_INET as u32, TCP_TABLE_OWNER_PID_ALL, 0))?;
        for row in rows::<MIB_TCPROW_OWNER_PID>(&buf) {
            let port = port_of(row.dwLocalPort);
            let addr = Ipv4Addr::from(row.dwLocalAddr.to_ne_bytes()).to_string();
            bump(&mut scan, row.dwOwningPid, row.dwState);
            match row.dwState {
                STATE_LISTEN => scan
                    .listeners
                    .push(binding(port, &addr, false, row.dwOwningPid)),
                STATE_ESTAB => *conns.entry(port).or_insert(0) += 1,
                _ => {}
            }
        }

        // IPv6（Node / 现代服务常默认 `::`）
        let buf6 = query(|p, s| {
            GetExtendedTcpTable(p, s, 0, AF_INET6 as u32, TCP_TABLE_OWNER_PID_ALL, 0)
        })?;
        for row in rows::<MIB_TCP6ROW_OWNER_PID>(&buf6) {
            let port = port_of(row.dwLocalPort);
            let addr = Ipv6Addr::from(row.ucLocalAddr).to_string();
            bump(&mut scan, row.dwOwningPid, row.dwState);
            match row.dwState {
                STATE_LISTEN => scan
                    .listeners
                    .push(binding(port, &addr, true, row.dwOwningPid)),
                STATE_ESTAB => {
                    // 同一服务端口的连接只按 v4/v6 各统计一次，端口相同即合并
                    *conns.entry(port).or_insert(0) += 1
                }
                _ => {}
            }
        }
    }

    let mut conns: Vec<(u16, u32)> = conns.into_iter().collect();
    conns.sort_unstable();
    scan.conns = conns;
    Ok(scan)
}

pub fn scan_udp() -> Result<Vec<PortBinding>, String> {
    let mut out = Vec::new();
    unsafe {
        let buf =
            query(|p, s| GetExtendedUdpTable(p, s, 0, AF_INET as u32, UDP_TABLE_OWNER_PID, 0))?;
        for row in rows::<MIB_UDPROW_OWNER_PID>(&buf) {
            let port = port_of(row.dwLocalPort);
            if port == 0 {
                continue;
            }
            let addr = Ipv4Addr::from(row.dwLocalAddr.to_ne_bytes()).to_string();
            out.push(binding(port, &addr, false, row.dwOwningPid));
        }

        let buf6 =
            query(|p, s| GetExtendedUdpTable(p, s, 0, AF_INET6 as u32, UDP_TABLE_OWNER_PID, 0))?;
        for row in rows::<MIB_UDP6ROW_OWNER_PID>(&buf6) {
            let port = port_of(row.dwLocalPort);
            if port == 0 {
                continue;
            }
            let addr = Ipv6Addr::from(row.ucLocalAddr).to_string();
            out.push(binding(port, &addr, true, row.dwOwningPid));
        }
    }
    Ok(out)
}

/// 内核口径：LISTEN 总数 + TIME_WAIT（含端口）。
///
/// Windows 上这个数与 `scan_tcp` 的监听项**应当相等**（表本身带 pid），
/// 所以 `coverage.note` 正常为 None；这里仍照接口返回，保持跨平台一致。
pub fn scan_kernel_tcp() -> Option<KernelTcp> {
    let mut out = KernelTcp::default();
    let mut tw: std::collections::BTreeSet<u16> = std::collections::BTreeSet::new();

    unsafe {
        let buf =
            query(|p, s| GetExtendedTcpTable(p, s, 0, AF_INET as u32, TCP_TABLE_OWNER_PID_ALL, 0))
                .ok()?;
        for row in rows::<MIB_TCPROW_OWNER_PID>(&buf) {
            match row.dwState {
                STATE_LISTEN => out.listen_total += 1,
                STATE_TIME_WAIT => {
                    out.time_wait += 1;
                    if tw.len() < 128 {
                        tw.insert(port_of(row.dwLocalPort));
                    }
                }
                _ => {}
            }
        }
        let buf6 =
            query(|p, s| GetExtendedTcpTable(p, s, 0, AF_INET6 as u32, TCP_TABLE_OWNER_PID_ALL, 0))
                .ok()?;
        for row in rows::<MIB_TCP6ROW_OWNER_PID>(&buf6) {
            match row.dwState {
                STATE_LISTEN => out.listen_total += 1,
                STATE_TIME_WAIT => {
                    out.time_wait += 1;
                    if tw.len() < 128 {
                        tw.insert(port_of(row.dwLocalPort));
                    }
                }
                _ => {}
            }
        }
    }

    out.time_wait_ports = tw.into_iter().collect();
    Some(out)
}
