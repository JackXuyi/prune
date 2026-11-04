//! 端口数据采集：`lsof` 机器格式解析 + `netstat` 交叉校验（macOS / Linux）。
//!
//! 为什么是 lsof 而不是 libproc FFI（M7 §2.2 实测）：
//! - `lsof -nP -a -iTCP -F pcnPT` 实测 **30–40 ms**，与一次 `sysinfo::scan()` 同量级；
//! - 零权限、零新依赖，解析层可以纯函数化（好测试）；
//! - Windows 走 `GetExtendedTcpTable`（见 `win_ports.rs`），接口形态一致。
//!
//! 两个必须记住的坑（都实测确认过）：
//! 1. **`-p` 与 `-i` 是 OR 关系**：`lsof -p 626 -iTCP -F` 会把微信的 cwd / txt / mmap
//!    路径全吐出来（`n/Applications/WeChat.app/...`）。**必须加 `-a`**，否则解析器会
//!    把文件路径当 socket 地址。本模块用「只认带 `P` 行的 fd」做第二道防线。
//! 2. **`n` 行可能带箭头**：已建立连接是 `n[::1]:3118->[::1]:51741`，
//!    取 local 必须先 `split("->")`，否则端口解析成 51741。

use std::process::Command;

use super::host;
use super::{BindScope, Coverage, PortBinding, PortSnapshot, Proto};

/// cwd 查询并发：单次 20 ms，通常 8–20 个监听进程，12 路可压到 2 轮以内
const CDW_CONCURRENCY: usize = 12;

/// 一次全 TCP 扫描的产出：监听项 + 每端口的 ESTABLISHED 数。
#[derive(Debug, Default, Clone)]
pub struct TcpScan {
    pub listeners: Vec<PortBinding>,
    /// local port → ESTABLISHED 条数（按 local 算：服务端的 local 就是服务端口）
    pub conns: Vec<(u16, u32)>,
    /// pid → 连接计数（M8）。与 `conns` 同源，只是按进程再切一刀
    pub conns_by_pid: std::collections::BTreeMap<i32, super::ConnStats>,
}

/// lsof 可执行文件路径。
///
/// macOS 在 `/usr/sbin/lsof`；Linux 常见 `/usr/bin/lsof`。写死单一路径会让
/// Linux 上整个功能静默变成「没有端口」，所以按序探测。
pub fn lsof_path() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "/usr/sbin/lsof"
    }
    #[cfg(not(target_os = "macos"))]
    {
        for p in ["/usr/bin/lsof", "/usr/sbin/lsof", "/bin/lsof"] {
            if std::path::Path::new(p).exists() {
                return p;
            }
        }
        "lsof"
    }
}

/// 全 TCP 一轮：LISTEN 与 ESTABLISHED 一起拿（总耗时与只拿 LISTEN 相同）。
///
/// 失败返回 `Err`：调用方必须把「采集失败」与「没有端口」区分开，
/// 不能用一个空 `Vec` 冒充「端口都空着」。
pub fn scan_tcp() -> Result<TcpScan, String> {
    #[cfg(windows)]
    {
        return super::win_ports::scan_tcp();
    }
    #[cfg(not(windows))]
    {
        let out = Command::new(lsof_path())
            .args(["-w", "-n", "-P", "-a", "-iTCP", "-F", "pcnPT"])
            .output()
            .map_err(|e| format!("lsof 无法执行：{e}"))?;
        if !out.status.success() && out.stdout.is_empty() {
            return Err(format!(
                "lsof 失败（{}）：{}",
                out.status,
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        Ok(parse_lsof_tcp(&String::from_utf8_lossy(&out.stdout)))
    }
}

/// UDP 绑定端口。UDP 没有 LISTEN 状态，也没有「连接数」。
pub fn scan_udp() -> Result<Vec<PortBinding>, String> {
    #[cfg(windows)]
    {
        return super::win_ports::scan_udp();
    }
    #[cfg(not(windows))]
    {
        let out = Command::new(lsof_path())
            .args(["-w", "-n", "-P", "-a", "-iUDP", "-F", "pcn"])
            .output()
            .map_err(|e| format!("lsof 无法执行：{e}"))?;
        if !out.status.success() && out.stdout.is_empty() {
            return Err(format!(
                "lsof 失败（{}）：{}",
                out.status,
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        Ok(parse_lsof_udp(&String::from_utf8_lossy(&out.stdout)))
    }
}

/// 内核口径的 TCP 状态统计：LISTEN 总数 + TIME_WAIT 条数与涉及的端口。
///
/// **为什么需要它**：lsof 对无权访问的进程静默跳过（§3.2.1），所以
/// 「lsof 没列出来」不能推出「没被占用」。netstat 走内核表，能给出分母。
#[derive(Debug, Default, Clone)]
pub struct KernelTcp {
    pub listen_total: usize,
    pub time_wait: u32,
    pub time_wait_ports: Vec<u16>,
}

pub fn scan_kernel_tcp() -> Option<KernelTcp> {
    #[cfg(windows)]
    {
        return super::win_ports::scan_kernel_tcp();
    }
    #[cfg(not(windows))]
    {
        // macOS: `netstat -anv -p tcp`；Linux: `netstat -an -p tcp`（-v 也支持）
        let out = Command::new("/usr/sbin/netstat")
            .args(["-anv", "-p", "tcp"])
            .output()
            .or_else(|_| Command::new("netstat").args(["-an", "-p", "tcp"]).output())
            .ok()?;
        if !out.status.success() {
            return None;
        }
        Some(parse_netstat(&String::from_utf8_lossy(&out.stdout)))
    }
}

// ---------------------------------------------------------------- 解析

/// 一个 fd 块的临时状态
#[derive(Default)]
struct FdBlock {
    proto: Option<Proto>,
    name: Option<String>,
    state: Option<String>,
}

/// 解析 `lsof -F pcnPT` 的输出。
///
/// 机器格式（实测）：
/// ```text
/// p584            pid
/// crapportd       命令名
/// f8              fd（块起点）
/// PTCP            协议
/// n*:58578        地址（可能带 `->`）
/// TST=LISTEN      状态
/// ```
pub fn parse_lsof_tcp(text: &str) -> TcpScan {
    let mut scan = TcpScan::default();
    let mut conns: std::collections::HashMap<u16, u32> = std::collections::HashMap::new();

    let mut pid: Option<i32> = None;
    let mut comm = String::new();
    let mut block = FdBlock::default();

    // flush：处理一个已完成的 fd 块
    let flush = |pid: Option<i32>,
                 comm: &str,
                 block: &FdBlock,
                 scan: &mut TcpScan,
                 conns: &mut std::collections::HashMap<u16, u32>| {
        let Some(pid) = pid else { return };        // 第二道防线：没有 P 行的 fd 是文件/管道，不是 socket（防 `-a` 缺失时的脏输入）
        let Some(proto) = block.proto else { return };
        let Some(name) = block.name.as_deref() else { return };

        let local = name.split("->").next().unwrap_or(name);
        let Some((addr, port)) = split_addr(local) else { return };
        let v6 = local.starts_with('[') || addr.contains(':');

        match block.state.as_deref() {
            Some("LISTEN") => {
                scan.listeners.push(PortBinding {
                    port,
                    proto,
                    scope: BindScope::from_addr(&addr),
                    addr,
                    v6,
                    pid: Some(pid),
                    comm: comm.to_string(),
                    start_time: 0,
                    project: None,
                    project_label: None,
                });
            }
            Some("ESTABLISHED") => {
                *conns.entry(port).or_insert(0) += 1;
                scan.conns_by_pid.entry(pid).or_default().established += 1;
            }
            // CLOSE_WAIT 不进端口计数（它不是服务），但对应用维度是泄漏信号：
            // 本端没关、对端已关，说明程序没释放 fd。只计数，不下结论。
            Some("CLOSE_WAIT") => {
                scan.conns_by_pid.entry(pid).or_default().close_wait += 1;
            }
            // CLOSED / FIN_WAIT_* / SYN_SENT / TIME_WAIT(内核态无 fd) 都不是「服务」
            _ => {}
        }
    };

    for line in text.lines() {
        let Some((tag, rest)) = line.split_at_checked(1) else {
            continue;
        };
        match tag {
            "p" => {
                flush(pid, &comm, &block, &mut scan, &mut conns);
                block = FdBlock::default();
                pid = rest.trim().parse::<i32>().ok();
                comm.clear();
            }
            "c" => comm = rest.to_string(),
            "f" => {
                flush(pid, &comm, &block, &mut scan, &mut conns);
                block = FdBlock::default();
            }
            "P" => {
                block.proto = match rest {
                    "TCP" => Some(Proto::Tcp),
                    "UDP" => Some(Proto::Udp),
                    _ => None,
                }
            }
            "n" => block.name = Some(rest.to_string()),
            "T" => {
                if let Some(st) = rest.strip_prefix("ST=") {
                    block.state = Some(st.to_string());
                }
            }
            _ => {}
        }
    }
    flush(pid, &comm, &block, &mut scan, &mut conns);

    let mut conns: Vec<(u16, u32)> = conns.into_iter().collect();
    conns.sort_unstable();
    scan.conns = conns;
    scan
}

/// 解析 `lsof -F pcn`（UDP：没有 `T` 行，没有连接数）。
pub fn parse_lsof_udp(text: &str) -> Vec<PortBinding> {
    let mut out = Vec::new();

    let mut pid: Option<i32> = None;
    let mut comm = String::new();
    let mut block = FdBlock::default();

    let flush = |pid: Option<i32>, comm: &str, block: &FdBlock, out: &mut Vec<PortBinding>| {
        let Some(pid) = pid else { return };
        if block.proto.is_none() {
            return;
        }
        let Some(name) = block.name.as_deref() else { return };
        let local = name.split("->").next().unwrap_or(name);
        let Some((addr, port)) = split_addr(local) else { return };
        let v6 = local.starts_with('[') || addr.contains(':');
        out.push(PortBinding {
            port,
            proto: Proto::Udp,
            scope: BindScope::from_addr(&addr),
            addr,
            v6,
            pid: Some(pid),
            comm: comm.to_string(),
            start_time: 0,
            project: None,
            project_label: None,
        });
    };

    for line in text.lines() {
        let Some((tag, rest)) = line.split_at_checked(1) else {
            continue;
        };
        match tag {
            "p" => {
                flush(pid, &comm, &block, &mut out);
                block = FdBlock::default();
                pid = rest.trim().parse::<i32>().ok();
                comm.clear();
            }
            "c" => comm = rest.to_string(),
            "f" => {
                flush(pid, &comm, &block, &mut out);
                block = FdBlock::default();
            }
            "P" => {
                block.proto = match rest {
                    "TCP" => Some(Proto::Tcp),
                    "UDP" => Some(Proto::Udp),
                    _ => None,
                }
            }
            "n" => block.name = Some(rest.to_string()),
            _ => {}
        }
    }
    flush(pid, &comm, &block, &mut out);
    out
}

/// `*:58578` → (`*`, 58578)；`[::1]:1430` → (`::1`, 1430)；`*:*` → None。
///
/// 端口解析失败必须返回 None（`*` 无法 parse 成 u16）——这正是过滤
/// 「无绑定端口的通配 fd」的机制，不要在这里兜底成 0。
pub fn split_addr(name: &str) -> Option<(String, u16)> {
    let idx = name.rfind(':')?;
    let (addr, port_str) = (&name[..idx], &name[idx + 1..]);
    let port: u16 = port_str.parse().ok()?;
    if port == 0 {
        return None;
    }
    let addr = addr.trim_start_matches('[').trim_end_matches(']');
    Some((addr.to_string(), port))
}

/// 解析 `netstat -anv -p tcp`：只取 LISTEN 计数与 TIME_WAIT（含端口）。
///
/// 输出列（实测）：`Proto Recv-Q Send-Q Local Foreign (state) ...`
/// 注意地址用 `.` 分隔端口（`192.168.0.115.57589`、`*.5000`、`::1.1430`）。
pub fn parse_netstat(text: &str) -> KernelTcp {
    let mut out = KernelTcp::default();
    let mut tw_ports: std::collections::BTreeSet<u16> = std::collections::BTreeSet::new();

    for line in text.lines() {
        if !line.starts_with("tcp") {
            continue;
        }
        let cols: Vec<&str> = line.split_whitespace().collect();
        // 列不足的行（header 变体）直接跳过，不要靠猜下标对齐
        let (Some(local), Some(state)) = (cols.get(3), cols.get(5)) else {
            continue;
        };
        match *state {
            "LISTEN" => out.listen_total += 1,
            "TIME_WAIT" => {
                out.time_wait += 1;
                if let Some(port) = local.rsplit('.').next().and_then(|p| p.parse::<u16>().ok()) {
                    if tw_ports.len() < 128 {
                        tw_ports.insert(port);
                    }
                }
            }
            _ => {}
        }
    }
    out.time_wait_ports = tw_ports.into_iter().collect();
    out
}

// ---------------------------------------------------------------- 主入口

/// 一轮完整端口采样：TCP + UDP + 内核交叉校验 + 项目归属。
///
/// `proc_names` 用于补齐进程名：macOS 的 lsof 自带 `c` 行，传 None 即可；
/// Windows 的 iphlpapi 只给 pid，由上层（MemState 的全量进程表）提供映射。
///
/// 失败不 panic：lsof 不可用时返回 `unavailable` 快照，`coverage.note` 写清原因——
/// 「无数据」与「没有端口」必须是两种不同的状态。
pub fn scan(proc_names: Option<&std::collections::HashMap<i32, String>>) -> PortSnapshot {
    scan_cached(proc_names, &mut host::ProjectCache::new())
}

/// 同上，但复用跨轮的项目归属缓存（GUI 常驻路径用这个：
/// 冷扫 137 ms → 热扫约 50 ms，差别全在 cwd 查询上）。
pub fn scan_cached(
    proc_names: Option<&std::collections::HashMap<i32, String>>,
    cache: &mut host::ProjectCache,
) -> PortSnapshot {
    let started = std::time::Instant::now();

    // 三个子进程互相独立（内核表 / TCP / UDP），并行跑 —— 串行是 ~90 ms，并行约 40 ms。
    // 实测：三者各 30 ms 左右，绝不值得排队。
    let (kernel, tcp, udp) = std::thread::scope(|s| {
        let k = s.spawn(scan_kernel_tcp);
        let t = s.spawn(scan_tcp);
        let u = s.spawn(scan_udp);
        (
            k.join().unwrap_or(None),
            match t.join() {
                Ok(v) => v,
                Err(_) => Err("端口扫描线程异常退出".to_string()),
            },
            u.join().unwrap_or_else(|_| Ok(Vec::new())),
        )
    });

    let (tcp_scan, tcp_err) = match tcp {
        Ok(v) => (v, None),
        Err(e) => (TcpScan::default(), Some(e)),
    };
    // 原始 LISTEN 条数（**去重前**）：双栈端口在 lsof 里是两条 fd，
    // 用聚合后的条数做分子会误报「有 3 个端口不可见」（实测踩过）。
    let tcp_listen_rows = tcp_scan.listeners.len();
    let udp_bindings = udp.unwrap_or_default();

    // 项目归属：只对监听进程（去重后通常 8–20 个）并发跑 cwd，实测 20 ms/进程
    let mut bindings: Vec<PortBinding> = tcp_scan.listeners.clone();
    bindings.extend(udp_bindings.clone());

    let unique_pids: Vec<i32> = {
        let mut v: Vec<i32> = bindings.iter().filter_map(|b| b.pid).collect();
        v.sort_unstable();
        v.dedup();
        v
    };
    let projects = cache.resolve(&unique_pids, CDW_CONCURRENCY);
    let home = host::home_dir();
    for b in &mut bindings {
        if let Some(dir) = b.pid.and_then(|p| projects.get(&p)) {
            b.project = Some(dir.clone());
            b.project_label = Some(host::label_of(dir, home.as_deref()));
        }
        // Windows 分支只拿到 pid：用进程表补名字（关注度判定依赖 comm）
        if b.comm.is_empty() {
            if let Some(name) = b.pid.and_then(|p| proc_names.and_then(|m| m.get(&p))) {
                b.comm = name.clone();
            }
        }
    }

    let entries = super::group(bindings, &tcp_scan.conns);

    let listen_total = kernel.as_ref().map(|k| k.listen_total).unwrap_or(0);
    let attributed = tcp_listen_rows;

    let note = if let Some(err) = tcp_err {
        Some(format!("端口采集失败：{err}"))
    } else if attributed == 0 && listen_total > 0 {
        Some("无法读取进程的 socket 信息（权限不足）".into())
    } else if listen_total > attributed {
        Some(format!(
            "有 {} 个监听端口无法归属到进程（可能属于其他用户或系统进程）",
            listen_total - attributed
        ))
    } else {
        None
    };

    PortSnapshot {
        entries,
        coverage: Coverage {
            listen_total,
            attributed,
            note,
        },
        time_wait: kernel.as_ref().map(|k| k.time_wait),
        time_wait_ports: kernel.map(|k| k.time_wait_ports).unwrap_or_default(),
        conns_by_pid: tcp_scan.conns_by_pid,
        at: now_secs(),
        scan_ms: started.elapsed().as_millis() as u64,
    }
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 实测片段（rapportd 的双 fd + WeChat 的多端口 + 一条已建立连接）
    const TCP_SAMPLE: &str = "\
p584
crapportd
f8
PTCP
n*:58578
TST=LISTEN
TQR=0
f9
PTCP
n*:58578
TST=LISTEN
p626
cWeChat
f144
PTCP
n127.0.0.1:14023
TST=LISTEN
f150
PTCP
n[::1]:3118->[::1]:51741
TST=ESTABLISHED
f151
PTCP
n[::1]:51741->[::1]:3118
TST=ESTABLISHED
";

    #[test]
    fn parses_listen_and_established() {
        let scan = parse_lsof_tcp(TCP_SAMPLE);
        // 两条 LISTEN（rapportd 双 fd 由 group 去重，解析层如实保留）
        assert_eq!(scan.listeners.len(), 3);
        assert!(scan.listeners.iter().all(|b| b.port != 0));
        // ESTABLISHED 按 local port 统计：3118 有一条、51741 有一条
        let conns: std::collections::HashMap<u16, u32> = scan.conns.iter().copied().collect();
        assert_eq!(conns.get(&3118), Some(&1));
        assert_eq!(conns.get(&51741), Some(&1));
    }

    #[test]
    fn arrow_in_name_does_not_shift_port() {
        // 不加 split("->") 会把 51741 当成服务端口——这是最隐蔽的一个解析 bug
        let scan = parse_lsof_tcp("p1\ncnode\nf5\nPTCP\nn[::1]:3118->[::1]:51741\nTST=LISTEN\n");
        assert_eq!(scan.listeners[0].port, 3118);
        assert_eq!(scan.listeners[0].addr, "::1");
        assert!(scan.listeners[0].v6);
    }

    #[test]
    fn conns_are_counted_per_pid() {
        // M8：同一个 lsof 输出里，WeChat 有 2 条 ESTABLISHED、1 条 CLOSE_WAIT，
        // rapportd 只有 LISTEN——它不该出现在 conns_by_pid 里。
        let text = "\
p584
crapportd
f8
PTCP
n*:58578
TST=LISTEN
p626
cWeChat
f150
PTCP
n[::1]:3118->[::1]:51741
TST=ESTABLISHED
f151
PTCP
n[::1]:51742->[::1]:3118
TST=ESTABLISHED
f152
PTCP
n[::1]:51743->[::1]:443
TST=CLOSE_WAIT
p900
cnode
f9
PTCP
n127.0.0.1:7000->127.0.0.1:51000
TST=ESTABLISHED
";
        let scan = parse_lsof_tcp(text);
        assert_eq!(scan.conns_by_pid.get(&626).map(|c| c.established), Some(2));
        assert_eq!(scan.conns_by_pid.get(&626).map(|c| c.close_wait), Some(1));
        assert_eq!(scan.conns_by_pid.get(&900).map(|c| c.established), Some(1));
        // 只有 LISTEN 的进程不产生连接记录（「没有记录 = 0 条」）
        assert!(!scan.conns_by_pid.contains_key(&584));
        // 端口维度的 conns 只数 ESTABLISHED，CLOSE_WAIT 不进
        let conns: std::collections::HashMap<u16, u32> = scan.conns.iter().copied().collect();
        assert_eq!(conns.get(&51743), None);
    }

    #[test]
    fn file_fds_without_protocol_are_ignored() {
        // 实测：`lsof -p 626 -iTCP`（漏了 -a）会把 cwd/txt 一起吐出来。
        // 这类行没有 P 行，必须被丢弃，否则文件路径会被当 socket 地址。
        let dirty = "\
p626
cWeChat
fcwd
n/Users/xuyi/Library/Containers/com.tencent.xinWeChat/Data
ftxt
n/Applications/WeChat.app/Contents/MacOS/WeChat
f144
PTCP
n127.0.0.1:14023
TST=LISTEN
";
        let scan = parse_lsof_tcp(dirty);
        assert_eq!(scan.listeners.len(), 1);
        assert_eq!(scan.listeners[0].port, 14023);
    }

    #[test]
    fn wildcard_udp_is_dropped() {
        let udp = "p1\nclaunchd\nf3\nPUDP\nn*:*\nf4\nPUDP\nn*:5353\n";
        let out = parse_lsof_udp(udp);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].port, 5353);
    }

    #[test]
    fn addr_splitting() {
        assert_eq!(split_addr("*:58578"), Some(("*".into(), 58578)));
        assert_eq!(split_addr("[::1]:1430"), Some(("::1".into(), 1430)));
        assert_eq!(split_addr("127.0.0.1:7890"), Some(("127.0.0.1".into(), 7890)));
        assert_eq!(split_addr("*:*"), None);
        assert_eq!(split_addr("nonsense"), None);
    }

    /// 实测 `netstat -anv -p tcp` 片段
    const NETSTAT_SAMPLE: &str = "\
Proto Recv-Q Send-Q  Local Address          Foreign Address        (state)
tcp4       0      0  192.168.0.115.57589    35.77.25.170.10843     TIME_WAIT
tcp4       0      0  *.5000                 *.*                    LISTEN
tcp6       0      0  ::1.1430               *.*                    LISTEN
tcp4       0      0  127.0.0.1.7890         *.*                    LISTEN
tcp4       0      0  192.168.0.115.57590    1.1.1.1.443            ESTABLISHED
tcp4       0      0  192.168.0.115.57591    1.1.1.1.443            TIME_WAIT
";

    #[test]
    fn netstat_counts_listen_and_time_wait() {
        let k = parse_netstat(NETSTAT_SAMPLE);
        assert_eq!(k.listen_total, 3);
        assert_eq!(k.time_wait, 2);
        assert_eq!(k.time_wait_ports, vec![57589, 57591]);
    }

    #[test]
    fn netstat_ignores_header_and_short_lines() {
        let k = parse_netstat("Proto Recv-Q\ntcp4 0 0\n");
        assert_eq!(k.listen_total, 0);
        assert_eq!(k.time_wait, 0);
    }
}
