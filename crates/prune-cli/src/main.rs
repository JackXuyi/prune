//! prune-cli：命令行入口（README「CLI」一节）。
//!
//! 单二进制双形态：
//! - 常规子命令：`scan` / `reclaim` / `mem` / `mem --watch`
//! - `prune mcp`：MCP stdio server（AI Agent 查询入口，见 mcp.rs）
//!
//! arg 手写不引 clap（D-C1）：5 条子命令，clap 拖慢编译且无必要。
//! 安全底线（D-C4）：`reclaim` **默认 dry-run**，真执行必须显式 `--yes`，
//! 之下还有 prune-core 的白名单 + 20GB 熔断两道闸。

mod mcp;

use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use prune_core::disk::whitelist::Level;
use prune_core::disk::{self, CacheGroup};
use prune_core::leak;
use prune_core::mem;
use prune_core::net;
use prune_core::reclaim::{self, Limits, ReclaimError};
use prune_core::swap;

// ---------------------------------------------------------------- 参数解析

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Scan {
        json: bool,
    },
    Reclaim {
        yes: bool,
    },
    Mem {
        swapped: bool,
        json: bool,
        watch: Option<u64>,
    },
    /// `prune port` 列关注层；`prune port 3000` 查单个端口；`--all` 含内部 IPC
    Port {
        port: Option<u16>,
        json: bool,
        all: bool,
    },
    /// `prune app <家族名>`：一个应用的五域资源画像（M8）
    App {
        name: String,
        json: bool,
    },
    /// `prune lan [--scan] [--json]`：局域网设备与暴露面（M18，默认只读邻居表）
    Lan {
        /// 全段 TCP connect 探测（催熟 ARP 后回读邻居表，/23 约 6s）
        scan: bool,
        json: bool,
    },
    Mcp,
    Version,
}

pub fn parse_args(args: &[String]) -> Result<Command, String> {
    let mut it = args.iter();
    let sub = it.next().map(|s| s.as_str()).ok_or_else(usage)?;

    match sub {
        "-V" | "--version" | "version" => Ok(Command::Version),
        "-h" | "--help" | "help" => Err(usage()),
        "mcp" => {
            no_more_args(&mut it)?;
            Ok(Command::Mcp)
        }
        "scan" => {
            let mut json = false;
            for a in it {
                match a.as_str() {
                    "--json" => json = true,
                    other => return Err(format!("scan 不认识参数 {other:?}\n{}", usage())),
                }
            }
            Ok(Command::Scan { json })
        }
        "reclaim" => {
            let mut yes = false;
            let mut level = false;
            for a in it {
                match a.as_str() {
                    "--yes" | "-y" => yes = true,
                    "--level" => level = true,
                    "green" if level => level = false,
                    // 默认就是 dry-run，接受以兼容 README 示例
                    "--dry-run" => {}
                    other => return Err(format!("reclaim 不认识参数 {other:?}\n{}", usage())),
                }
            }
            if level {
                return Err("--level 需要一个值（v1 只支持 green）".into());
            }
            Ok(Command::Reclaim { yes })
        }
        "mem" => {
            let mut swapped = false;
            let mut json = false;
            let mut watch: Option<u64> = None;
            let all: Vec<&str> = it.map(|s| s.as_str()).collect();
            let mut i = 0;
            while i < all.len() {
                match all[i] {
                    "--swapped" => swapped = true,
                    "--json" => json = true,
                    "--watch" => watch = Some(10),
                    "--interval" => {
                        i += 1;
                        let v = all
                            .get(i)
                            .ok_or_else(|| "--interval 需要一个秒数".to_string())?;
                        watch = Some(
                            v.parse::<u64>()
                                .map_err(|_| format!("--interval 不是数字: {v:?}"))?,
                        );
                    }
                    other => return Err(format!("mem 不认识参数 {other:?}\n{}", usage())),
                }
                i += 1;
            }
            Ok(Command::Mem {
                swapped,
                json,
                watch,
            })
        }
        "port" => {
            let mut json = false;
            let mut all = false;
            let mut port: Option<u16> = None;
            for a in it {
                match a.as_str() {
                    "--json" => json = true,
                    "--all" | "-a" => all = true,
                    other => match other.parse::<u16>() {
                        Ok(p) if port.is_none() => port = Some(p),
                        Ok(_) => return Err("port 只接受一个端口号".into()),
                        Err(_) => {
                            return Err(format!("port 不认识参数 {other:?}\n{}", usage()));
                        }
                    },
                }
            }
            Ok(Command::Port { port, json, all })
        }
        "app" => {
            let mut json = false;
            let mut name: Option<String> = None;
            for a in it {
                match a.as_str() {
                    "--json" => json = true,
                    other if other.starts_with('-') => {
                        return Err(format!("app 不认识参数 {other:?}\n{}", usage()));
                    }
                    other => match name {
                        None => name = Some(other.to_string()),
                        Some(_) => return Err("app 只接受一个家族名".into()),
                    },
                }
            }
            let Some(name) = name else {
                return Err(format!(
                    "app 需要一个家族名，如 prune app cursor\n{}",
                    usage()
                ));
            };
            Ok(Command::App { name, json })
        }
        "lan" => {
            let mut json = false;
            let mut scan = false;
            for a in it {
                match a.as_str() {
                    "--json" => json = true,
                    "--scan" => scan = true,
                    other => return Err(format!("lan 不认识参数 {other:?}\n{}", usage())),
                }
            }
            Ok(Command::Lan { scan, json })
        }
        other => Err(format!("不认识子命令 {other:?}\n{}", usage())),
    }
}

fn no_more_args(it: &mut std::slice::Iter<String>) -> Result<(), String> {
    if let Some(a) = it.next() {
        return Err(format!("不认识参数 {a:?}\n{}", usage()));
    }
    Ok(())
}

fn usage() -> String {
    "\
用法：
  prune scan [--json]                 全盘扫描，输出可回收清单
  prune reclaim [--level green]       预览零风险清理项（默认 dry-run；--yes 才真删）
  prune mem [--swapped] [--json]      一次性内存读数（footprint 口径）
  prune mem --watch [--interval N]    持续采样并输出泄漏嫌疑判定（默认 10s）
  prune port [PORT] [--json] [--all]  谁占着这个端口（不带 PORT 列关注层；--all 含内部 IPC）
  prune app <家族名> [--json]         一个应用的资源画像：进程/内存/CPU/端口/连接/磁盘
  prune lan [--scan] [--json]         局域网设备与暴露面（默认只读邻居表；--scan 全段探测）
  prune mcp                           启动 MCP stdio server（AI Agent 查询入口）
  prune --version | --help"
        .into()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = match parse_args(&args) {
        Ok(c) => c,
        Err(msg) => {
            eprintln!("{msg}");
            std::process::exit(2);
        }
    };

    let code = match cmd {
        Command::Version => {
            println!("prune v{}", prune_core::version());
            0
        }
        Command::Scan { json } => cmd_scan(json),
        Command::Reclaim { yes } => cmd_reclaim(yes),
        Command::Mem {
            swapped,
            json,
            watch,
        } => match watch {
            Some(interval) => cmd_watch(interval),
            None => cmd_mem(swapped, json),
        },
        Command::Port { port, json, all } => cmd_port(port, json, all),
        Command::App { name, json } => cmd_app(&name, json),
        Command::Lan { scan, json } => cmd_lan(scan, json),
        Command::Mcp => cmd_mcp(),
    };
    std::process::exit(code);
}

fn home_dir() -> PathBuf {
    prune_core::paths::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

// ---------------------------------------------------------------- 通用输出

fn hb(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1024.0 && i < 4 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{bytes} B")
    } else {
        format!("{v:.1} {}", UNITS[i])
    }
}

// ---------------------------------------------------------------- scan

fn cmd_scan(json: bool) -> i32 {
    let started = Instant::now();
    let home = home_dir();
    if !json {
        eprintln!("扫描中（约 15s）…");
    }
    let report = disk::scan(
        &home,
        Some(&|p: disk::ScanProgress| {
            eprint!("\r  [{}/{}] {}", p.done, p.total, p.phase);
            let _ = std::io::stderr().flush();
        }),
    );
    if !json {
        eprintln!(
            "\r扫描完成，用时 {} ms      ",
            started.elapsed().as_millis()
        );
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&report).unwrap());
        return 0;
    }

    let t = &report.totals;
    println!(
        "⚡ 可回收 {}（L1 死重 {} + L2 缓存 {} + L3 项目 {}）",
        hb(t.reclaimable),
        hb(t.deadweight),
        hb(t.l2),
        hb(t.l3)
    );
    if t.l4 > 0 {
        println!("（另有 L4 Docker {}，不自动处理，见文末命令）", hb(t.l4));
    }
    println!();
    println!("{:<34}{:>12}  {:>8}  说明", "分组", "体积", "条目");
    for g in &report.groups {
        println!(
            "{:<34}{:>12}  {:>8}  {}",
            g.label,
            hb(g.total_bytes),
            g.entry_count,
            level_note(g)
        );
    }

    // A1 明细（杀手功能单独展开）
    if let Some(a1) = report.groups.iter().find(|g| g.deadweight.is_some()) {
        if let Some(dw) = &a1.deadweight {
            println!();
            println!(
                "跨平台二进制（{}）：死重 {} · 本机原生 {} · 浪费比 {:.2}:1",
                a1.label,
                hb(dw.bytes),
                hb(dw.native_bytes),
                dw.ratio.unwrap_or(0.0)
            );
            for s in dw.by_platform.iter().take(6) {
                println!(
                    "  {:<10} {:>4} 条  {:>10}",
                    s.platform,
                    s.entry_count,
                    hb(s.bytes)
                );
            }
            println!("  最大的几条：");
            for it in dw.items.iter().take(5) {
                println!("    {:>10}  {}", hb(it.bytes), it.name);
            }
        }
    }

    // A3 多管理器重复
    if let Some(dup) = &report.dup {
        if dup.managers.len() > 1 {
            println!();
            println!(
                "包管理器重复：{} 个并存，合计 {}",
                dup.managers.len(),
                hb(dup.total_bytes)
            );
            for (name, bytes) in &dup.managers {
                println!("  {:<20}{:>12}", name, hb(*bytes));
            }
            println!("  → {}", dup.advice);
        }
    }

    // A6：Docker 只给命令，永不执行
    if report.groups.iter().any(|g| g.key == mcp::L4_KEY) {
        println!();
        println!("Docker（{}，本工具永不执行，请自行评估）：", hb(t.l4));
        for (cmd, desc) in mcp::docker_hints() {
            println!("  {cmd:<36} # {desc}");
        }
    }
    0
}

fn level_note(g: &CacheGroup) -> &'static str {
    match g.level {
        Level::L1 => "🟢 零风险条目",
        Level::L2 => "🟢 整目录可清（移入废纸篓）",
        Level::L3 => {
            if g.cleanable {
                "🟡 可清，需确认"
            } else {
                "🟡 只统计，分散在多个项目目录"
            }
        }
        Level::L4 => "🔴 只提供命令",
    }
}

// ---------------------------------------------------------------- reclaim

fn cmd_reclaim(yes: bool) -> i32 {
    let home = home_dir();
    eprintln!("扫描中…");
    let report = disk::scan(&home, None);

    // v1 绿级 = A1 死重条目（L1，零风险）；整库清理不进 CLI（D-C4）
    let pv = reclaim::preview(&["yarn".into()], &report);
    if pv.items.is_empty() {
        println!("没有可清理的零风险项。");
        return 0;
    }
    println!(
        "将清理 {} 项，合计 {}：",
        pv.items.len(),
        hb(pv.total_bytes)
    );
    for it in &pv.items {
        println!("  {:>10}  {}", hb(it.bytes), it.path);
    }
    for n in &pv.notes {
        println!("  ℹ️  {}", n.text_zh(&report));
    }

    if !yes {
        println!();
        println!("以上为预览（dry-run）。确认无误后追加 --yes 执行（移入废纸篓，可恢复）。");
        return 0;
    }

    // CLI v1 只做 L1（A1 死重条目），静态白名单足够；报告派生白名单留给桌面端 A4/A5
    match reclaim::execute(&pv.items, &home, true, Limits::default(), &[]) {
        Ok(rep) => {
            println!("已移入废纸篓 {} 项，{}。", rep.moved, hb(rep.bytes));
            for f in &rep.failed {
                println!("  ⚠️ 失败 {}：{}", f.path, f.reason);
            }
            0
        }
        Err(ReclaimError::NeedsExplicitConfirm(total)) => {
            // 20GB 熔断：即使 --yes 也要拦（prune-core 护栏）
            eprintln!("总量 {} 超过熔断阈值，即使 --yes 也拒绝执行。", hb(total));
            1
        }
        Err(e) => {
            eprintln!("执行失败：{e}");
            1
        }
    }
}

// ---------------------------------------------------------------- mem

fn cmd_mem(swapped: bool, json: bool) -> i32 {
    eprintln!("采样中（深探 Top 20，约 1-2s）…");
    let procs = mcp::watch_round(20);

    let swap_info = swap::collect(None);
    let pressure = mem::system::read_pressure();

    if json {
        let doc = serde_json::json!({
            "physicalTotal": mem::system::physical_total(),
            "swap": swap_info,
            "pressure": pressure,
            "families": mem::aggregate(&procs),
            "topSwapped": mem::top_swapped(&procs, 5),
        });
        println!("{}", serde_json::to_string_pretty(&doc).unwrap());
        return 0;
    }

    let total = mem::system::physical_total();
    // 没有口径就写「不可用」——不打印 0，避免脚本把 0 当真
    let compressor = mem::system::read()
        .ok()
        .and_then(|s| s.compressor)
        .map(hb)
        .unwrap_or_else(|| "不可用".to_string());
    let pressure_text = pressure
        .map(|p| format!("{:?}", p.level))
        .unwrap_or_else(|| "不可用".to_string());
    println!(
        "物理 {} · 压缩器 {} · swap {}/{}（{:.0}%） · 压力 {}",
        hb(total),
        compressor,
        hb(swap_info.used),
        hb(swap_info.total),
        swap_info.used_ratio() * 100.0,
        pressure_text
    );
    println!();
    let head = if swapped {
        format!(
            "{:<22}{:>11}{:>11}{:>6}  {}",
            "应用家族", "占用", "其中换出", "进程", "口径"
        )
    } else {
        format!("{:<22}{:>11}{:>6}  {}", "应用家族", "占用", "进程", "口径")
    };
    println!("{head}");
    let mut fams = mem::aggregate(&procs);
    fams.sort_by_key(|f| std::cmp::Reverse(f.total()));
    for f in fams.iter().take(15) {
        let sw_col = if swapped {
            format!("{:>11}", f.swapped.map(hb).unwrap_or_else(|| "—".into()))
        } else {
            String::new()
        };
        println!(
            "{:<22}{:>11}{sw_col}{:>6}  {}",
            f.name,
            hb(f.total()),
            f.process_count,
            if f.is_exact() { "footprint" } else { "≈ RSS" }
        );
    }
    println!();
    println!("口径说明：footprint = 物理常驻 + 已压缩 + 已换出；≈ RSS 为未深探进程的下界估算。");
    0
}

// ---------------------------------------------------------------- port

/// `prune port [PORT]`：谁占着这个端口。
///
/// 口径纪律（docs/m7-port-plan.md §3.2）：
/// - 「没查到」≠「端口空闲」：查不到时把内核口径与不可见条数一起说出来；
/// - TIME_WAIT 不是占用：单独一行解释，且没有「结束」建议；
/// - 双栈不重复：内核里 v4/v6 各一条 fd，这里只输出一行。
fn cmd_port(only: Option<u16>, json: bool, all: bool) -> i32 {
    let snap = net::ports::scan(None);

    if json {
        let entries: Vec<&net::PortEntry> = snap
            .entries
            .iter()
            .filter(|e| only.map(|p| e.port == p).unwrap_or(all || e.watched))
            .collect();
        let doc = serde_json::json!({
            "port": only,
            "entries": entries,
            "coverage": snap.coverage,
            "timeWait": snap.time_wait,
            "timeWaitPorts": snap.time_wait_ports,
            "scanMs": snap.scan_ms,
        });
        println!("{}", serde_json::to_string_pretty(&doc).unwrap());
        return 0;
    }

    // 单端口查询：给出完整解释（持有者 + 项目 + 连接 + 暴露面）
    if let Some(port) = only {
        let hits: Vec<&net::PortEntry> = snap.entries.iter().filter(|e| e.port == port).collect();
        if hits.is_empty() {
            println!("端口 {port}：当前用户可见范围内没有进程监听。");
            if let Some(note) = &snap.coverage.note {
                println!("  ⚠️ {note}");
            }
            let invisible = snap.coverage.invisible();
            if invisible > 0 {
                println!(
                    "  内核报告 {} 个监听，其中 {invisible} 个无法归属到进程（可能是别的用户的进程）。",
                    snap.coverage.listen_total
                );
            }
            if snap.time_wait_ports.contains(&port) {
                println!(
                    "  该端口出现在 TIME_WAIT 里（{} 条）：内核保留期内会让新服务拿到 EADDRINUSE，约 2 分钟后消失。",
                    snap.time_wait.unwrap_or(0)
                );
            }
            println!("  自查：sudo lsof -nP -iTCP:{port} -sTCP:LISTEN");
            return 0;
        }
        for e in hits {
            println!(
                "{}/{}  {}",
                e.port,
                proto_cn(e.proto),
                exposure_cn(e.exposure)
            );
            for b in &e.listeners {
                let pid = b
                    .pid
                    .map(|p| p.to_string())
                    .unwrap_or_else(|| "内核持有".into());
                println!("  持有者  {} (pid {})  绑定 {}", b.comm, pid, b.addr);
                if let (Some(label), Some(path)) = (&b.project_label, &b.project) {
                    println!("  项目    {label}\n          {path}");
                }
            }
            if let Some(n) = e.conns {
                println!("  连接    {n} 条 ESTABLISHED");
            }
            if let Some(note) = &e.note {
                println!("  说明    {note}");
            }
        }
        return 0;
    }

    // 列表：关注层在前，折叠层只报条数
    let (watched, folded): (Vec<&net::PortEntry>, Vec<&net::PortEntry>) =
        snap.entries.iter().partition(|e| e.watched);
    println!(
        "{}{}{}{}{}{}",
        pad("端口", 7),
        pad("协议", 7),
        pad("暴露面", 11),
        pad("进程(pid)", 25),
        pad("项目", 27),
        pad("连接", 4)
    );
    let print_rows = |rows: &[&net::PortEntry]| {
        for e in rows {
            let b = e.primary();
            let proc_text = b
                .map(|b| match b.pid {
                    Some(pid) => format!("{} ({})", b.comm, pid),
                    None => format!("{} (内核)", b.comm),
                })
                .unwrap_or_else(|| "—".into());
            let project = b
                .and_then(|b| b.project_label.clone())
                .unwrap_or_else(|| "—".into());
            println!(
                "{}{}{}{}{}{}",
                pad(&e.port.to_string(), 7),
                pad(proto_cn(e.proto), 7),
                pad(exposure_cn(e.exposure), 11),
                pad(&truncate(&proc_text, 23), 25),
                pad(&truncate(&project, 25), 27),
                e.conns.map(|n| n.to_string()).unwrap_or_else(|| "—".into())
            );
        }
    };
    print_rows(&watched);

    if all && !folded.is_empty() {
        println!();
        println!("其他 {} 个端口（内部 IPC / 系统服务）：", folded.len());
        print_rows(&folded);
    } else if !folded.is_empty() {
        println!();
        println!(
            "另有 {} 个内部 IPC / 系统服务端口（--all 展开）。",
            folded.len()
        );
    }

    println!();
    let invisible = snap.coverage.invisible();
    print!(
        "覆盖：内核 {} 个监听 · 可归属 {} 个",
        snap.coverage.listen_total, snap.coverage.attributed
    );
    if invisible > 0 {
        print!("（{invisible} 个不可见，可能是别的用户的进程）");
    }
    println!(" · 扫描 {} ms", snap.scan_ms);
    if let Some(note) = &snap.coverage.note {
        println!("⚠️ {note}");
    }
    if let Some(tw) = snap.time_wait {
        println!(
            "TIME_WAIT {tw} 条（内核保留期，无持有者，不占进程）：{}",
            snap.time_wait_ports
                .iter()
                .take(6)
                .map(|p| p.to_string())
                .collect::<Vec<_>>()
                .join(" / ")
        );
    }
    0
}

// ---------------------------------------------------------------- app（M8）

/// 匹配失败的两种形态：文案不同（「没有这个应用」vs「说的哪一个」）
pub enum AppMatchError {
    /// 一个都没中，候选是当前全部家族
    NotFound(Vec<String>),
    /// 多个家族同时命中，候选就是命中的那几个
    Ambiguous(Vec<String>),
}

/// 一个应用的五域资源画像。CLI 与 MCP 共用（mcp.rs 调 `crate::app_resource_report`）。
///
/// 口径（docs/m8-app-resource-plan.md §3）：
/// - CPU 是单核口径可 >100%，部分成员未知时为下界；
/// - 端口只列关注层（内部 IPC 随机端口是噪音）；
/// - 连接只数 ESTABLISHED / CLOSE_WAIT——连接多 ≠ 流量大；
/// - 磁盘三层按需 du（4 s/层预算），稀疏文件按实占块，可能带 partial 下界。
pub struct AppResource {
    pub family: String,
    pub process_count: usize,
    pub memory_bytes: u64,
    /// false = 未深探，内存为 RSS 下界（CLI 不做深探，深探是 GUI 的 1-2 s 开销）
    pub memory_exact: bool,
    pub cpu: Option<f32>,
    pub cpu_covered: usize,
    pub conns: net::ConnStats,
    /// 关注层端口（端口，暴露面）
    pub ports: Vec<(u16, net::BindScope)>,
    pub disk: prune_core::appdisk::AppDisk,
}

/// 家族名模糊匹配（大小写不敏感子串）。零命中或多个家族同时命中都返回候选，让用户重试。
pub fn app_resource_report(query: &str) -> Result<AppResource, AppMatchError> {
    // CPU 语义（M5）：首见进程一律 None——所以这里扫两轮，中间隔 ~0.8 s，
    // 第二轮才有「近 10 s 均值」可读。代价是命令多等 1 秒，换一个真实读数值得。
    let procs = {
        let mut scanner = prune_core::mem::process::Scanner::new();
        let _ = scanner.scan();
        std::thread::sleep(Duration::from_millis(800));
        scanner.scan().procs
    };
    let q = query.to_lowercase();
    let mut members: Vec<&mem::ProcMem> = procs
        .iter()
        .filter(|p| p.family.to_lowercase().contains(&q))
        .collect();
    members.sort_by_key(|p| std::cmp::Reverse(total_bytes(p)));

    if members.is_empty() {
        // 给候选而不是只报错：家族名的准确拼法用户未必知道
        let mut names: Vec<String> = procs.iter().map(|p| p.family.clone()).collect();
        names.sort();
        names.dedup();
        return Err(AppMatchError::NotFound(names));
    }

    // 多个家族同时命中（如 "code" 同时中 Code / CodeBuddy）→ 报候选让用户重试，
    // 绝不把两个应用的进程合并成一张画像（会把内存和磁盘都算错）
    let mut families: Vec<String> = members.iter().map(|p| p.family.clone()).collect();
    families.sort();
    families.dedup();
    if families.len() > 1 {
        return Err(AppMatchError::Ambiguous(families));
    }
    let family = families.remove(0);
    let pids: Vec<i32> = members.iter().map(|p| p.pid).collect();
    let memory_bytes: u64 = members.iter().map(|p| total_bytes(p)).sum();
    let memory_exact = members.iter().all(|p| p.footprint.is_some());
    let cpu_sum: Option<f32> = {
        let any_known = members.iter().any(|p| p.cpu.is_some());
        any_known.then(|| members.iter().filter_map(|p| p.cpu).sum())
    };
    let cpu_covered = members.iter().filter(|p| p.cpu.is_some()).count();

    // 连接与端口共用 M7 的一次扫描（GUI 同款），不重复跑 lsof
    let snap = net::ports::scan(None);
    let conns = snap.conns_of(&pids);
    let ports: Vec<(u16, net::BindScope)> = snap
        .entries
        .iter()
        .filter(|e| {
            e.watched
                && e.listeners
                    .iter()
                    .any(|b| b.pid.is_some_and(|p| pids.contains(&p)))
        })
        .map(|e| (e.port, e.exposure))
        .collect();

    let exe = members.iter().find_map(|p| p.exe.clone());
    eprintln!("测量磁盘占用（只读，最多 4 秒/层）…");
    let disk = prune_core::appdisk::measure(&family, exe.as_deref(), &home_dir());

    Ok(AppResource {
        family,
        process_count: members.len(),
        memory_bytes,
        memory_exact,
        cpu: cpu_sum,
        cpu_covered,
        conns,
        ports,
        disk,
    })
}

fn total_bytes(p: &mem::ProcMem) -> u64 {
    p.footprint.unwrap_or(p.rss)
}

/// `prune lan`（M18）：默认只读邻居表 + 暴露面，**不发任何探测包**——
/// 默认行为必须无副作用（与 reclaim 默认 dry-run 同一原则）；
/// `--scan` 全段 TCP connect 探测（催熟 ARP 后回读邻居表）。
fn cmd_lan(scan: bool, json: bool) -> i32 {
    // 暴露面：复用 ports::scan 的 BindScope::Any 口径，不重算（spec §10）
    let port_snap = net::ports::scan(None);
    let exposed: Vec<u16> = port_snap
        .entries
        .iter()
        .filter(|e| e.exposure == net::BindScope::Any && e.proto == net::Proto::Tcp)
        .map(|e| e.port)
        .collect();

    let at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let mut snap = if scan {
        // 30s mDNS 窗口期间终端要有心跳，否则像卡死（stderr：stdout 保持纯输出）
        eprintln!(
            "全段扫描中：TCP connect {} 地址 + mDNS（{}s 窗口）+ SSDP（{}s）…",
            net::lan::SCAN_CAP,
            net::lan::MDNS_WINDOW_SECS,
            net::lan::SSDP_WINDOW_SECS
        );
        net::lan::snapshot_scan(at, &exposed)
    } else {
        net::lan::snapshot(at, &exposed)
    };
    // M19 时间线（docs/m19-lan-device-times-spec.md §5）：与 GUI 同一份历史层，
    // 回填 first_seen / last_seen / is_new。不可用快照由 record 内部跳过。
    net::lan_history::record(&mut snap);
    if scan {
        // 与 GUI 同一份缓存（spec §12）：MCP `lan_devices` 读的是它，
        // 所以没有 GUI 的机器上 `prune lan --scan` 之后 MCP 也有数据可读
        if let Some(path) = prune_core::paths::lan_cache_path() {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Ok(text) = serde_json::to_string_pretty(&snap) {
                let tmp = path.with_extension("json.tmp");
                if std::fs::write(&tmp, text).is_ok() {
                    let _ = std::fs::rename(&tmp, &path);
                }
            }
        }
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&snap).unwrap());
        return 0;
    }

    let cidr = snap.cidr.as_deref().unwrap_or("?");
    let iface = snap.iface.as_deref().unwrap_or("?");
    println!(
        "局域网（{iface} · {cidr}）· {} 台设备 · {:.1}s",
        snap.devices.len(),
        snap.scan_ms as f64 / 1000.0
    );
    if snap.scanned > 0 || snap.total > 0 {
        println!("  本轮探测 {} / {} 个地址", snap.scanned, snap.total);
    }
    if snap.exposed_ports.is_empty() {
        println!("  我对同网段开放 0 个端口");
    } else {
        let ports: Vec<String> = snap.exposed_ports.iter().map(|p| p.to_string()).collect();
        println!(
            "  我对同网段开放 {} 个端口：{}",
            ports.len(),
            ports.join(" / ")
        );
    }
    for note in &snap.notes {
        println!("  ⚠️ {note}");
    }
    if snap.devices.is_empty() {
        return 0;
    }
    println!();
    for d in &snap.devices {
        let mac = d.mac.as_deref().unwrap_or("—");
        let vendor = if d.randomized_mac {
            "· 随机 MAC".to_string()
        } else {
            d.vendor.clone().unwrap_or_else(|| "—".into())
        };
        let role = if d.is_self {
            "本机".to_string()
        } else if d.is_gateway {
            "网关".to_string()
        } else {
            kind_label(d.kind).to_string()
        };
        let state = if !d.online {
            "不可达"
        } else if d.state == net::lan::NeighborState::Stale {
            "最近在线"
        } else {
            "在线"
        };
        let inbound = if d.inbound.is_empty() {
            String::new()
        } else {
            let ports: Vec<String> = d.inbound.iter().map(|p| p.to_string()).collect();
            format!("  ← 连了我 {}", ports.join("/"))
        };
        // M19 时间线：新设备标记 + 加入/活跃时长（first_seen == 0 = 旧缓存无记录，不显示）
        let badge = if d.is_new { "[新] " } else { "" };
        let times = if d.first_seen > 0 {
            format!(
                "  加入{} 活跃{}",
                ago_str(at - d.first_seen),
                ago_str(at - d.last_seen)
            )
        } else {
            String::new()
        };
        println!(
            "  {:<15} {:<17} {:<12} {:<6} {}{badge}{times}{inbound}",
            d.ip, mac, vendor, role, state
        );
    }
    0
}

/// 秒差 → CLI 相对时长（CLI 无 i18n，中文硬编码与现有输出一致）
fn ago_str(diff: i64) -> String {
    if diff < 0 {
        return "—".into();
    }
    if diff < 60 {
        return "刚刚".into();
    }
    if diff < 3600 {
        return format!("{} 分钟前", diff / 60);
    }
    if diff < 86_400 {
        return format!("{} 小时前", diff / 3600);
    }
    format!("{} 天前", diff / 86_400)
}

fn kind_label(kind: net::lan::DeviceKind) -> &'static str {
    match kind {
        net::lan::DeviceKind::Router => "路由器",
        net::lan::DeviceKind::Computer => "电脑",
        net::lan::DeviceKind::Phone => "手机",
        net::lan::DeviceKind::Tablet => "平板",
        net::lan::DeviceKind::Tv => "电视",
        net::lan::DeviceKind::Speaker => "音箱",
        net::lan::DeviceKind::Printer => "打印机",
        net::lan::DeviceKind::Nas => "NAS",
        net::lan::DeviceKind::Camera => "摄像头",
        net::lan::DeviceKind::Iot => "IoT",
        net::lan::DeviceKind::Unknown => "未知",
    }
}

fn cmd_app(query: &str, json: bool) -> i32 {
    match app_resource_report(query) {
        Err(AppMatchError::NotFound(candidates)) => {
            eprintln!("没有匹配 {query:?} 的应用。当前在跑的家族：");
            for n in candidates.iter().take(20) {
                eprintln!("  {n}");
            }
            2
        }
        Err(AppMatchError::Ambiguous(candidates)) => {
            eprintln!(
                "{query:?} 同时匹配了 {} 个家族，请用全名重试：",
                candidates.len()
            );
            for n in &candidates {
                eprintln!("  {n}");
            }
            2
        }
        Ok(r) => {
            if json {
                let doc = serde_json::json!({
                    "family": r.family,
                    "processCount": r.process_count,
                    "memoryBytes": r.memory_bytes,
                    "memoryExact": r.memory_exact,
                    "cpu": r.cpu,
                    "cpuCovered": r.cpu_covered,
                    "conns": r.conns,
                    "ports": r.ports.iter().map(|(p, s)| serde_json::json!({
                        "port": p, "exposure": exposure_cn(*s),
                    })).collect::<Vec<_>>(),
                    "disk": r.disk,
                });
                println!("{}", serde_json::to_string_pretty(&doc).unwrap());
                return 0;
            }

            println!("{} · {} 个进程", r.family, r.process_count);
            println!(
                "内存  {}{}",
                hb(r.memory_bytes),
                if r.memory_exact {
                    ""
                } else {
                    "（≈ RSS 下界，未深探）"
                }
            );
            println!(
                "CPU   {}",
                r.cpu
                    .map(|c| {
                        // 先拼百分比再补后缀：直接嵌套 format! 会让 clippy 报
                        // `format!` in `format!` args，也确实白套一层
                        let pct = format!("{c:.1}%");
                        let suffix = if r.cpu_covered < r.process_count {
                            "（下界）"
                        } else {
                            ""
                        };
                        format!("{pct}{suffix}")
                    })
                    .unwrap_or_else(|| "—".into())
            );
            if r.ports.is_empty() {
                println!("端口  —（无关注层端口）");
            } else {
                let list = r
                    .ports
                    .iter()
                    .map(|(p, s)| format!(":{p}（{}）", exposure_cn(*s)))
                    .collect::<Vec<_>>()
                    .join(" ");
                println!("端口  {list}");
            }
            if r.conns.established == 0 && r.conns.close_wait == 0 {
                println!("连接  —");
            } else {
                let mut line = format!("连接  {} 活跃", r.conns.established);
                if r.conns.close_wait > 0 {
                    line.push_str(&format!(
                        " · {} CLOSE_WAIT（可能泄漏，不是结论）",
                        r.conns.close_wait
                    ));
                }
                println!("{line}");
            }
            let leaf = |l: &Option<prune_core::appdisk::DiskLeaf>| match l {
                Some(l) => format!("{}{}", if l.partial { "≥ " } else { "" }, hb(l.bytes)),
                None => "—".into(),
            };
            println!(
                "磁盘  本体 {} · 缓存 {} · 数据 {}",
                leaf(&r.disk.bundle),
                leaf(&r.disk.cache),
                leaf(&r.disk.data)
            );
            if let Some(note) = &r.disk.note {
                println!("      {note}");
            }
            println!();
            println!("口径：内存 footprint 优先；CPU 单核可 >100%；连接多 ≠ 流量大；磁盘按实占块统计（含 APFS 克隆共享）。");
            0
        }
    }
}

fn proto_cn(p: net::Proto) -> &'static str {
    match p {
        net::Proto::Tcp => "tcp",
        net::Proto::Udp => "udp",
    }
}

fn exposure_cn(s: net::BindScope) -> &'static str {
    match s {
        net::BindScope::Any => "全部网卡",
        net::BindScope::Interface => "指定网卡",
        net::BindScope::Loopback => "仅本机",
        net::BindScope::Unknown => "未知",
    }
}

/// 按终端显示宽度左对齐补空格（中文占 2 列，`{:<N}` 只数字符，会把表格撑歪）
fn pad(s: &str, width: usize) -> String {
    let w: usize = s.chars().map(|c| if c.is_ascii() { 1 } else { 2 }).sum();
    let mut out = s.to_string();
    for _ in w..width {
        out.push(' ');
    }
    out
}

/// 终端表格按字符宽度截断（中文 2 列宽，这里只做粗略处理，够用）
fn truncate(s: &str, max: usize) -> String {
    let width: usize = s.chars().map(|c| if c.is_ascii() { 1 } else { 2 }).sum();
    if width <= max {
        return s.to_string();
    }
    let mut out = String::new();
    let mut w = 0;
    for c in s.chars() {
        let cw = if c.is_ascii() { 1 } else { 2 };
        if w + cw > max.saturating_sub(1) {
            out.push('…');
            break;
        }
        out.push(c);
        w += cw;
    }
    out
}

// ---------------------------------------------------------------- watch

fn cmd_watch(interval: u64) -> i32 {
    if interval == 0 {
        eprintln!("--interval 必须 > 0");
        return 2;
    }
    println!(
        "watch 模式：每 {interval}s 采样一轮，每 60s 输出一次泄漏嫌疑判定（序列不落盘，Ctrl-C 退出）。"
    );
    println!("判定需要 30 分钟序列，之前会如实显示「数据不足」。");
    println!();

    let mut acc = mcp::SeriesAccum::new();
    let mut last_verdict = 0i64;
    loop {
        let procs = mcp::watch_round(20);
        let ts = prune_core::swap::now_secs() as i64;
        acc.push(&procs, ts);

        // 按实际时间触发（轮耗时含深探，会大于 interval，按轮数算会迟到）
        if ts - last_verdict >= 60 {
            last_verdict = ts;
            let series = acc.series();
            // watch 会话通常不足 30 min：全量序列交给 detect，让「数据不足」如实说话
            let report = leak::detect(&series, &procs, ts);
            print_watch_verdict(&report);
        }
        std::thread::sleep(Duration::from_secs(interval));
    }
}

fn print_watch_verdict(report: &leak::LeakReport) {
    let t = ts_hms(prune_core::swap::now_secs() as i64);
    if let Some(note) = &report.note {
        println!("[{t}] 无法判定：{note}");
        return;
    }
    if report.items.is_empty() {
        println!(
            "[{t}] 暂无泄漏嫌疑（观测 {} 进程：数据不足 {}、良性排除 {}、未达标 {}）",
            report.observed, report.insufficient, report.benign, report.below
        );
        return;
    }
    println!(
        "[{t}] ⚠️ 疑似内存泄漏 {} 个进程（净增长合计 {}）：",
        report.items.len(),
        hb(report.total_growth())
    );
    for it in &report.items {
        println!(
            "  pid {} {}（{}）+{} / +{:.0}% · 斜率 {:.0} MB/h · R² {:.2} · 单点跃升占比 {:.0}%",
            it.pid,
            it.name,
            it.family,
            hb(it.growth),
            it.growth_ratio * 100.0,
            it.slope_mb_per_hour,
            it.r2,
            it.step_ratio * 100.0
        );
    }
    println!("  → 证据只有增长事实；建议重启该应用后观察是否复现。");
}

fn ts_hms(ts: i64) -> String {
    // 本地时区：libc localtime（prune-core 已依赖 libc，CLI 侧直接复用）
    #[cfg(unix)]
    unsafe {
        let t = ts as libc::time_t;
        let tm = libc::localtime(&t);
        if !tm.is_null() {
            return format!(
                "{:02}:{:02}:{:02}",
                (*tm).tm_hour,
                (*tm).tm_min,
                (*tm).tm_sec
            );
        }
    }
    format!("{ts}")
}

// ---------------------------------------------------------------- mcp

fn cmd_mcp() -> i32 {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        if let Some(resp) = mcp::handle_message(&line, &mcp::run_tool) {
            writeln!(stdout, "{resp}").ok();
            stdout.flush().ok();
        }
    }
    0
}

// ---------------------------------------------------------------- tests

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &str) -> String {
        v.to_string()
    }

    #[test]
    fn parses_all_subcommands() {
        assert_eq!(
            parse_args(&[s("scan"), s("--json")]).unwrap(),
            Command::Scan { json: true }
        );
        assert_eq!(
            parse_args(&[s("reclaim"), s("--level"), s("green")]).unwrap(),
            Command::Reclaim { yes: false }
        );
        assert_eq!(
            parse_args(&[s("reclaim"), s("--yes")]).unwrap(),
            Command::Reclaim { yes: true }
        );
        assert_eq!(
            parse_args(&[s("mem"), s("--swapped")]).unwrap(),
            Command::Mem {
                swapped: true,
                json: false,
                watch: None
            }
        );
        assert_eq!(
            parse_args(&[s("mem"), s("--watch")]).unwrap(),
            Command::Mem {
                swapped: false,
                json: false,
                watch: Some(10)
            }
        );
        assert_eq!(
            parse_args(&[s("mem"), s("--watch"), s("--interval"), s("5")]).unwrap(),
            Command::Mem {
                swapped: false,
                json: false,
                watch: Some(5)
            }
        );
        assert_eq!(parse_args(&[s("mcp")]).unwrap(), Command::Mcp);
        assert_eq!(parse_args(&[s("--version")]).unwrap(), Command::Version);
    }

    #[test]
    fn reclaim_accepts_readme_dry_run_flag() {
        assert!(matches!(
            parse_args(&[s("reclaim"), s("--dry-run")]).unwrap(),
            Command::Reclaim { .. }
        ));
    }

    #[test]
    fn rejects_unknown_and_malformed() {
        assert!(parse_args(&[]).is_err());
        assert!(parse_args(&[s("bogus")]).is_err());
        assert!(parse_args(&[s("scan"), s("--wat")]).is_err());
        assert!(parse_args(&[s("reclaim"), s("--level")]).is_err());
        assert!(parse_args(&[s("mem"), s("--interval")]).is_err());
        assert!(parse_args(&[s("mem"), s("--interval"), s("abc")]).is_err());
    }

    #[test]
    fn help_is_error_exit_path_not_panic() {
        let msg = parse_args(&[s("--help")]).unwrap_err();
        assert!(msg.contains("prune scan"));
    }

    #[test]
    fn parses_port_subcommand() {
        assert_eq!(
            parse_args(&[s("port")]).unwrap(),
            Command::Port {
                port: None,
                json: false,
                all: false
            }
        );
        assert_eq!(
            parse_args(&[s("port"), s("3000")]).unwrap(),
            Command::Port {
                port: Some(3000),
                json: false,
                all: false
            }
        );
        assert_eq!(
            parse_args(&[s("port"), s("3000"), s("--json"), s("--all")]).unwrap(),
            Command::Port {
                port: Some(3000),
                json: true,
                all: true
            }
        );
        // 端口 0 是通配 fd，不是服务：命令行上接受但扫描层会丢掉
        assert!(parse_args(&[s("port"), s("0")]).is_ok());
        // 越界 / 非数字 / 两个端口号都要拒绝，否则会静默查错端口
        assert!(parse_args(&[s("port"), s("70000")]).is_err());
        assert!(parse_args(&[s("port"), s("abc")]).is_err());
        assert!(parse_args(&[s("port"), s("3000"), s("8080")]).is_err());
    }

    #[test]
    fn truncate_counts_cjk_as_two() {
        assert_eq!(truncate("prune", 10), "prune");
        assert_eq!(truncate("meetily/frontend", 8), "meetily…");
        // 中文按 2 列算：4 个汉字 = 8 列 > 6 → 截断并补省略号
        assert_eq!(truncate("全部网卡", 6), "全部…");
    }

    #[test]
    fn pad_aligns_by_display_width() {
        assert_eq!(pad("tcp", 7), "tcp    ");
        // 「仅本机」6 列 → 补到 11 列只需 5 个空格
        assert_eq!(pad("仅本机", 11), "仅本机     ");
    }

    #[test]
    fn hb_formats() {
        assert_eq!(hb(0), "0 B");
        assert_eq!(hb(999), "999 B");
        assert_eq!(hb(1024), "1.0 KB");
        assert_eq!(hb(55 * 1024 * 1024 * 1024 / 10), "5.5 GB");
    }

    #[test]
    #[cfg_attr(target_os = "macos", ignore)] // localtime 在测试进程不可依赖具体时区
    fn ts_hms_wraps() {
        assert_eq!(ts_hms(0), "00:00:00");
        assert_eq!(ts_hms(3661), "01:01:01");
    }
}
