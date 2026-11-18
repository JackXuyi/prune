//! prune-cli：命令行入口（README「CLI」一节）。
//!
//! 单二进制双形态：
//! - 常规子命令：`scan` / `reclaim` / `mem` / `mem --watch`
//! - `prune mcp`：MCP stdio server（AI Agent 查询入口，见 mcp.rs）
//!
//! arg 手写不引 clap（D-C1）：5 条子命令，clap 拖慢编译且无必要。
//! 安全底线（D-C4）：`reclaim` **默认 dry-run**，真执行必须显式 `--yes`，
//! 之下还有 prune-core 的白名单 + 20GB 熔断两道闸。

mod docs_json;
mod doctor;
mod emit_script;
mod mcp;
mod mcp_install;
mod watch;

use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use prune_core::db::Store;
use prune_core::disk::project::ProjectTrend as DiskTrend;
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
        /// `--emit-script`（W3/D-15，DP-4 裁 A）：只打印候选删除命令的
        /// 可审计脚本，永不执行；与 `--yes` 互斥
        emit_script: bool,
    },
    Mem {
        swapped: bool,
        json: bool,
        watch: Option<u64>,
        /// F20：导出内存报告 CSV 到该路径（读序列库历史，不触发扫描）
        csv: Option<String>,
    },
    /// `prune port` 列关注层；`prune port 3000` 查单个端口；`--all` 含内部 IPC
    Port {
        port: Option<u16>,
        json: bool,
        all: bool,
        /// 只看冲突端口（同端口 ≥2 个不同 pid）
        conflicts: bool,
        /// F22：相对可信基线的漂移（新增 / 暴露面变宽 / 消失）
        drift: bool,
        /// F22：以当前快照重建基线（显式重置，不做自动老化）
        rebase: bool,
    },
    /// `prune why 5000`：端口白话解释（F16）——一句话结论 + 证据 + 指路
    Why {
        port: u16,
        json: bool,
    },
    /// `prune files <PATH>`：Open Files 占用反查（F23）——谁占着这块盘
    Files {
        path: String,
        json: bool,
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
    /// `prune watch`：按需守护（cpu / mem / port / disk）
    Watch {
        interval: u64,
        only: Vec<watch::Kind>,
        json: bool,
        once: bool,
        fail_on_warn: bool,
    },
    /// `prune projects`：项目依赖账本（按天趋势）
    Projects {
        days: u32,
        json: bool,
    },
    /// `prune doctor [--json] [--only disk,mem,port,net] [--timeout N]`：
    /// 四域体检（W1/D-04）——结论 + 证据 + 下一步，30s 内出结果
    Doctor {
        json: bool,
        only: Vec<doctor::Domain>,
        timeout: u64,
    },
    Mcp,
    /// `prune mcp install [--client claude|cursor|vscode|all] [--write] [--json] [--force]`
    /// 把 MCP server 写进客户端用户级配置（W1/D-05）。默认 dry-run。
    McpInstall {
        client: mcp_install::ClientSpec,
        write: bool,
        json: bool,
    },
    Version,
}

pub fn parse_args(args: &[String]) -> Result<Command, String> {
    let mut it = args.iter();
    let sub = it.next().map(|s| s.as_str()).ok_or_else(usage)?;

    match sub {
        "-V" | "--version" | "version" => Ok(Command::Version),
        "-h" | "--help" | "help" => Err(usage()),
        "mcp" => {
            // `prune mcp` = stdio server；`prune mcp install …` = 写客户端配置
            let rest: Vec<&str> = it.map(|s| s.as_str()).collect();
            if rest.first() == Some(&"install") {
                let mut client = mcp_install::ClientSpec::All;
                let mut write = false;
                let mut json = false;
                let mut i = 1;
                while i < rest.len() {
                    match rest[i] {
                        "--client" => {
                            i += 1;
                            let v = rest.get(i).ok_or_else(|| {
                                "--client 需要一个值（claude|cursor|vscode|all）".to_string()
                            })?;
                            client = mcp_install::ClientSpec::parse(v).ok_or_else(|| {
                                format!("--client 不认识 {v:?}；合法值：claude|cursor|vscode|all")
                            })?;
                        }
                        "--write" => write = true,
                        "--json" => json = true,
                        // 安全边界：不接受用户传路径（写哪个文件由代码常量决定）
                        "--force" => {
                            eprintln!(
                                "提示：坏 JSON 无论如何都不覆盖；--force 仅用于确认已知风险操作"
                            );
                        }
                        other => {
                            return Err(format!("mcp install 不认识参数 {other:?}\n{}", usage()))
                        }
                    }
                    i += 1;
                }
                return Ok(Command::McpInstall {
                    client,
                    write,
                    json,
                });
            }
            if let Some(a) = rest.first() {
                if *a != "install" {
                    return Err(format!("不认识参数 {a:?}\n{}", usage()));
                }
            }
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
            let mut emit_script = false;
            let mut level = false;
            for a in it {
                match a.as_str() {
                    "--yes" | "-y" => yes = true,
                    "--emit-script" => emit_script = true,
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
            // D-15：语义互斥——一个只打印、一个真执行，同给说明意图混乱，
            // 直接用法错退出 2，不猜
            if yes && emit_script {
                return Err(format!(
                    "--emit-script 与 --yes 互斥：一个只打印命令（人执行），一个真删\n{}",
                    usage()
                ));
            }
            Ok(Command::Reclaim { yes, emit_script })
        }
        "mem" => {
            let mut swapped = false;
            let mut json = false;
            let mut watch: Option<u64> = None;
            let mut csv: Option<String> = None;
            let all: Vec<&str> = it.map(|s| s.as_str()).collect();
            let mut i = 0;
            while i < all.len() {
                match all[i] {
                    "--swapped" => swapped = true,
                    "--json" => json = true,
                    "--watch" => watch = Some(10),
                    "--csv" => {
                        i += 1;
                        csv = Some(
                            all.get(i)
                                .ok_or_else(|| "--csv 需要一个输出路径".to_string())?
                                .to_string(),
                        );
                    }
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
                csv,
            })
        }
        "port" => {
            let mut json = false;
            let mut all = false;
            let mut conflicts = false;
            let mut drift = false;
            let mut rebase = false;
            let mut port: Option<u16> = None;
            for a in it {
                match a.as_str() {
                    "--json" => json = true,
                    "--all" | "-a" => all = true,
                    "--conflicts" | "-c" => conflicts = true,
                    "--drift" => drift = true,
                    "--rebase" => rebase = true,
                    other => match other.parse::<u16>() {
                        Ok(p) if port.is_none() => port = Some(p),
                        Ok(_) => return Err("port 只接受一个端口号".into()),
                        Err(_) => {
                            return Err(format!("port 不认识参数 {other:?}\n{}", usage()));
                        }
                    },
                }
            }
            if rebase && !drift {
                return Err("--rebase 只随 --drift 使用\n{}".to_string() + &usage());
            }
            Ok(Command::Port {
                port,
                json,
                all,
                conflicts,
                drift,
                rebase,
            })
        }
        "why" => {
            let mut json = false;
            let mut port: Option<u16> = None;
            for a in it {
                match a.as_str() {
                    "--json" => json = true,
                    other => match other.parse::<u16>() {
                        Ok(p) if port.is_none() => port = Some(p),
                        Ok(_) => return Err("why 只接受一个端口号".into()),
                        Err(_) => {
                            return Err(format!("why 不认识参数 {other:?}\n{}", usage()));
                        }
                    },
                }
            }
            let Some(port) = port else {
                return Err(format!(
                    "why 需要一个端口号，如 prune why 5000\n{}",
                    usage()
                ));
            };
            Ok(Command::Why { port, json })
        }
        "files" => {
            let mut json = false;
            let mut path: Option<String> = None;
            for a in it {
                match a.as_str() {
                    "--json" => json = true,
                    other if path.is_none() => path = Some(other.to_string()),
                    _ => return Err(format!("files 只接受一个路径\n{}", usage())),
                }
            }
            let Some(path) = path else {
                return Err(format!(
                    "files 需要一个路径，如 prune files /Volumes/Backup\n{}",
                    usage()
                ));
            };
            Ok(Command::Files { path, json })
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
        "watch" => {
            let mut o = watch::Opts {
                interval: watch::DEFAULT_INTERVAL_SEC,
                only: Vec::new(),
                json: false,
                once: false,
                fail_on_warn: false,
            };
            let all: Vec<&str> = it.map(|s| s.as_str()).collect();
            let mut i = 0;
            while i < all.len() {
                match all[i] {
                    "-h" | "--help" => return Err(usage()),
                    "--json" => o.json = true,
                    "--once" => o.once = true,
                    "--fail-on" => {
                        i += 1;
                        match all.get(i).copied() {
                            Some("warn") => o.fail_on_warn = true,
                            Some("none") => o.fail_on_warn = false,
                            Some(other) => {
                                return Err(format!("--fail-on 只接受 warn|none，收到 {other:?}"))
                            }
                            None => return Err("--fail-on 需要一个值（warn|none）".into()),
                        }
                    }
                    "--interval" => {
                        i += 1;
                        let v = all
                            .get(i)
                            .ok_or_else(|| "--interval 需要一个秒数".to_string())?;
                        let raw = v
                            .parse::<u64>()
                            .map_err(|_| format!("--interval 不是数字: {v:?}"))?;
                        // 钳制而不是报错：调用方常把环境变量直接传进来
                        o.interval = raw.clamp(watch::MIN_INTERVAL_SEC, watch::MAX_INTERVAL_SEC);
                        if o.interval != raw {
                            eprintln!("提示：--interval {raw} 超出范围，按 {}s 执行", o.interval);
                        }
                    }
                    "--only" => {
                        i += 1;
                        let v = all.get(i).ok_or_else(|| {
                            "--only 需要逗号分隔的类别（cpu,mem,port,disk）".to_string()
                        })?;
                        for part in v.split(',') {
                            if part.trim().is_empty() {
                                continue;
                            }
                            let k = watch::Kind::parse(part).ok_or_else(|| {
                                format!("--only 不认识 {part:?}；合法值：{}", watch::kinds_hint())
                            })?;
                            if !o.only.contains(&k) {
                                o.only.push(k);
                            }
                        }
                    }
                    other => return Err(format!("watch 不认识参数 {other:?}\n{}", usage())),
                }
                i += 1;
            }
            Ok(Command::Watch {
                interval: o.interval,
                only: o.only,
                json: o.json,
                once: o.once,
                fail_on_warn: o.fail_on_warn,
            })
        }
        "doctor" => {
            let mut json = false;
            let mut only: Vec<doctor::Domain> = Vec::new();
            let mut timeout = doctor::DISK_TIMEOUT_SECS;
            let all: Vec<&str> = it.map(|s| s.as_str()).collect();
            let mut i = 0;
            while i < all.len() {
                match all[i] {
                    "-h" | "--help" => return Err(usage()),
                    "--json" => json = true,
                    "--only" => {
                        i += 1;
                        let v = all.get(i).ok_or_else(|| {
                            "--only 需要逗号分隔的域（disk,mem,port,net）".to_string()
                        })?;
                        for part in v.split(',') {
                            if part.trim().is_empty() {
                                continue;
                            }
                            let d = doctor::Domain::parse(part).ok_or_else(|| {
                                format!("--only 不认识 {part:?}；合法值：disk,mem,port,net")
                            })?;
                            if !only.contains(&d) {
                                only.push(d);
                            }
                        }
                    }
                    "--timeout" => {
                        i += 1;
                        let v = all
                            .get(i)
                            .ok_or_else(|| "--timeout 需要一个秒数".to_string())?;
                        timeout = v
                            .parse::<u64>()
                            .map_err(|_| format!("--timeout 不是数字: {v:?}"))?
                            .clamp(1, doctor::TIMEOUT_CAP_SECS);
                    }
                    other => return Err(format!("doctor 不认识参数 {other:?}\n{}", usage())),
                }
                i += 1;
            }
            Ok(Command::Doctor {
                json,
                only,
                timeout,
            })
        }
        "projects" => {
            let mut days = 30u32;
            let mut json = false;
            let all: Vec<&str> = it.map(|s| s.as_str()).collect();
            let mut i = 0;
            while i < all.len() {
                match all[i] {
                    "-h" | "--help" => return Err(usage()),
                    "--json" => json = true,
                    "--days" => {
                        i += 1;
                        let v = all
                            .get(i)
                            .ok_or_else(|| "--days 需要一个天数".to_string())?;
                        days = v
                            .parse::<u32>()
                            .map_err(|_| format!("--days 不是整数: {v:?}"))?
                            .clamp(1, 400);
                    }
                    other => return Err(format!("projects 不认识参数 {other:?}\n{}", usage())),
                }
                i += 1;
            }
            Ok(Command::Projects { days, json })
        }
        other => Err(format!("不认识子命令 {other:?}\n{}", usage())),
    }
}

// ---------------------------------------------------------------- 退出码

/// CLI 退出码（契约 v1 的一部分；`usage()` 的退出码行由它派生，防再出现
/// 「文档写了、代码没有」——W1-DP-2 裁 A：给 3 一个真实的产生条件）。
///
/// 收敛判据（wave1 方案 §3.3）：
/// - 0：结论已出。包括「读到部分数据 + 在 `coverage.note` 里说明差额」——
///   部分不可见是数据事实，不是进程失败；升级成退出码会让 CI 对每台机器报错；
/// - 2：用法错（parse_args 失败、必填缺失、--only 未知域）；
/// - 3：主输出为空**且**根因是权限（lsof 被拒等）；
/// - 4：主输出为空且根因不是权限（超时、命令不存在、无法解析）；
/// - 10：watch 达 warn（仅 --fail-on）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitCode {
    Ok = 0,
    Usage = 2,
    Permission = 3,
    ProbeFailed = 4,
    WatchWarn = 10,
}

impl ExitCode {
    pub const ALL: &[(ExitCode, &'static str)] = &[
        (ExitCode::Ok, "正常"),
        (ExitCode::Usage, "用法错"),
        (ExitCode::Permission, "权限不足"),
        (ExitCode::ProbeFailed, "探测失败"),
        (ExitCode::WatchWarn, "watch 达 warn（仅 --fail-on）"),
    ];

    pub fn as_i32(self) -> i32 {
        self as i32
    }
}

/// 契约快照的 `exitCodes` 段（code → 机器可读短名）。
pub fn exit_code_map() -> serde_json::Value {
    serde_json::json!({
        "0": "ok",
        "2": "usage",
        "3": "permission",
        "4": "probe_failed",
        "10": "watch_warn",
    })
}

/// `usage()` 的退出码行——从枚举派生而不是手抄，两处永不脱节。
pub fn exit_code_line() -> String {
    let parts: Vec<String> = ExitCode::ALL
        .iter()
        .map(|(c, d)| format!("{} {}", *c as i32, d))
        .collect();
    format!("退出码：{}", parts.join(" / "))
}

fn usage() -> String {
    format!(
        "\
用法：
  prune scan [--json]                 全盘扫描，输出可回收清单
  prune reclaim [--level green] [--emit-script] [--yes]
                                      预览零风险清理项（默认 dry-run）；--emit-script 打印可审计脚本（永不执行）；--yes 才真删
  prune mem [--swapped] [--json]      一次性内存读数（footprint 口径）
  prune mem --csv <PATH>              导出内存报告 CSV（读序列库历史，不触发扫描）
  prune mem --watch [--interval N]    【已弃用】改用 prune watch --only mem
  prune watch [--interval N] [--only cpu,mem,port,disk] [--json] [--once] [--fail-on warn]
                                      按需守护：前台进程，Ctrl-C 即停，不装后台
  prune port [PORT] [--json] [--all] [--conflicts] [--drift [--rebase]]
                                      谁占着这个端口（--conflicts 只看冲突端口；
                                      --drift 看相对基线的漂移，--rebase 重建基线）
  prune why <PORT> [--json]           端口白话解释：一句话说清谁占着、为什么杀不掉、去哪关
  prune files <PATH> [--json]         占用反查（F23）：这块盘/目录被谁开着文件（只读，不解）
  prune projects [--days N] [--json]  项目依赖账本（按天趋势，默认 30 天）
  prune doctor [--json] [--only disk,mem,port,net] [--timeout N]
                                      四域体检：磁盘/内存/端口/网络的结论+证据+下一步
  prune app <家族名> [--json]         一个应用的资源画像：进程/内存/CPU/端口/连接/磁盘
  prune lan [--scan] [--json]         局域网设备与暴露面（默认只读邻居表；--scan 全段探测）
  prune mcp                           启动 MCP stdio server（AI Agent 查询入口）
  prune mcp install [--client claude|cursor|vscode|all] [--write] [--json]
                                      把 MCP server 写进客户端用户级配置（默认 dry-run）
  prune --version | --help

{}",
        exit_code_line()
    )
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
        Command::Reclaim { yes, emit_script } => cmd_reclaim(yes, emit_script),
        Command::Mem {
            swapped,
            json,
            watch,
            csv,
        } => {
            // F20 导出优先：--csv 是读历史的独立路径，与 --watch / 采样正交
            if let Some(path) = csv {
                cmd_mem_csv(&path)
            } else {
                match watch {
                    Some(_) => {
                        // DP-1 裁 A：别名过渡而不是立即 Breaking。已有脚本还能跑，
                        // 但必须让写脚本的人知道正确入口在哪，且给出去除时点。
                        eprintln!(
                            "提示：prune mem --watch 已并入 prune watch --only mem（2027-09 移除本别名）"
                        );
                        cmd_mem_watch_legacy(json)
                    }
                    None => cmd_mem(swapped, json),
                }
            }
        }
        Command::Watch {
            interval,
            only,
            json,
            once,
            fail_on_warn,
        } => watch::run(&watch::Opts {
            interval,
            only,
            json,
            once,
            fail_on_warn,
        }),
        Command::Projects { days, json } => cmd_projects(days, json),
        Command::Doctor {
            json,
            only,
            timeout,
        } => doctor::run(&doctor::Opts {
            only,
            timeout_secs: timeout,
            json,
        }),
        Command::Port {
            port,
            json,
            all,
            conflicts,
            drift,
            rebase,
        } => cmd_port(port, json, all, conflicts, drift, rebase),
        Command::Why { port, json } => cmd_why(port, json),
        Command::Files { path, json } => cmd_files(&path, json),
        Command::App { name, json } => cmd_app(&name, json),
        Command::Lan { scan, json } => cmd_lan(scan, json),
        Command::Mcp => cmd_mcp(),
        Command::McpInstall {
            client,
            write,
            json,
        } => mcp_install::run(client, write, json, swap::now_secs() as i64),
    };
    std::process::exit(code);
}

// ---------------------------------------------------------------- projects

/// `prune projects`：项目依赖账本（W2/D-08）。读的是 GUI/扫描写入的同一张
/// `project_day` 表；CLI 不触发扫描——账本是「历史记录」，不是「现场重算」。
fn cmd_projects(days: u32, json: bool) -> i32 {
    let path = mcp::default_db_path();
    if !path.exists() {
        return projects_unavailable(
            days,
            json,
            &format!(
                "账本库不存在（{}）：先跑一次 prune scan 或打开 GUI",
                path.display()
            ),
        );
    }
    let store = match Store::open(&path) {
        Ok(s) => s,
        Err(e) => return projects_unavailable(days, json, &format!("账本库打开失败: {e}")),
    };
    let now = swap::now_secs() as i64;
    let from = prune_core::localtime::day_string(now - (days as i64) * 86_400);
    let rows = match store.project_day_recent(&from) {
        Ok(r) => r,
        Err(e) => return projects_unavailable(days, json, &format!("读账本失败: {e}")),
    };
    if rows.is_empty() {
        return projects_unavailable(
            days,
            json,
            &format!("{from} 起没有账本记录：账本随每轮全量扫描写入，先扫一轮"),
        );
    }

    let trends = prune_core::disk::project::build_trends(&rows);

    if json {
        let doc = docs_json::projects_doc(&from, days, &trends);
        println!("{}", serde_json::to_string_pretty(&doc).unwrap());
        return 0;
    }

    for line in readout_lines(&trends) {
        println!("{line}");
    }
    println!(
        "
窗口 {from} 起（{} 天）。账本只记依赖目录体积；缺失的日子不补 0。",
        days
    );
    0
}

/// 人读模式最多列这么多行——终端不是趋势图，58 行刷过去等于没读。
/// `--json` 始终是全量，少列几行不会丢事实。
pub(crate) const READOUT_MAX: usize = 20;

/// 人读排序：先看「动了多少」，再看「现在多大」。
///
/// `build_trends` 按当前体积降序（给体检卡用，谁胖谁在前），但日常读数要回答的
/// 是「这两天谁变了」——一个昨天 2 GB、今天 0 B 的项目，按体积排会沉到最后一行，
/// 而它恰恰是唯一的新闻。所以这里按 |变化| 降序，变化不可测的排在有刻度的之后；
/// 体积只作同分时的次序。
pub(crate) fn readout_order(trends: &[DiskTrend]) -> Vec<&DiskTrend> {
    let mut v: Vec<&DiskTrend> = trends.iter().collect();
    v.sort_by_key(|t| {
        let moved: i128 = match t.delta_bytes {
            Some(d) => d.unsigned_abs() as i128,
            None => -1, // 一个点算不出变化：排在所有有刻度的后面
        };
        let size = t.last.as_ref().map(|p| p.deps_bytes).unwrap_or(0) as i128;
        (moved, size)
    });
    v.reverse();
    v
}

pub(crate) fn readout_lines(trends: &[DiskTrend]) -> Vec<String> {
    let ordered = readout_order(trends);
    let mut out = vec![format!(
        "{}{}{}采样",
        pad("项目", 34),
        pad("当前依赖", 12),
        pad("窗口内变化", 18)
    )];
    for t in ordered.iter().take(READOUT_MAX) {
        let cur = t
            .last
            .as_ref()
            .map(|p| hb(p.deps_bytes))
            .unwrap_or_else(|| "—".into());
        // 口径纪律：一个点算不出变化，说「数据不足」而不是 0
        let delta = match t.delta_bytes {
            Some(d) if d > 0 => format!("+{}", hb(d as u64)),
            Some(d) if d < 0 => format!("-{}", hb(d.unsigned_abs())),
            Some(_) => "持平".to_string(),
            None => format!("数据不足（{} 天）", t.samples),
        };
        out.push(format!(
            "{}{}{}{} 天",
            pad(&truncate(&t.name, 32), 34),
            pad(&cur, 12),
            pad(&delta, 18),
            t.samples
        ));
    }
    if ordered.len() > READOUT_MAX {
        out.push(format!(
            "… 另有 {} 个项目未列出（prune projects --json 看全量）",
            ordered.len() - READOUT_MAX
        ));
    }
    out
}

fn projects_unavailable(days: u32, json: bool, note: &str) -> i32 {
    if json {
        let doc = docs_json::projects_unavailable_doc(days, note);
        println!("{}", serde_json::to_string_pretty(&doc).unwrap());
    } else {
        eprintln!("{note}");
    }
    0
}

pub(crate) fn home_dir() -> PathBuf {
    prune_core::paths::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

/// 主输出为空时的退出码收敛（W1-DP-2）：3（权限不足）/ 4（探测失败）/
/// 0（含差额说明或真实「没有」）。
///
/// 判据来自 `Coverage.note` 的三种形态：
/// - `端口采集失败：…` → lsof 不可用 / 执行失败 → 4；
/// - `无法读取进程的 socket 信息（权限不足）` → 全量被拒 → 3；
/// - `有 N 个监听端口无法归属…` → 部分不可见，**数据事实**，仍是 0。
pub(crate) fn exit_code_for_empty(note: Option<&str>) -> i32 {
    match note {
        Some(n) if n.starts_with("端口采集失败") => ExitCode::ProbeFailed.as_i32(),
        Some(n) if n.contains("权限不足") => ExitCode::Permission.as_i32(),
        _ => ExitCode::Ok.as_i32(),
    }
}

// ---------------------------------------------------------------- 通用输出

pub(crate) fn hb(bytes: u64) -> String {
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

    // W2/D-08：全量扫描顺手记一行项目账本。CLI 用户不装 GUI 也能用 `prune projects`；
    // 失败只提示不改变退出码——账本是旁路读数，不是扫描的产出承诺。
    {
        let now = swap::now_secs() as i64;
        let day = prune_core::localtime::day_string(now);
        if let Ok(store) = Store::open(&mcp::default_db_path()) {
            match prune_core::disk::project::record_ledger(&store, &report, &day, now) {
                Ok(n) if n > 0 => {
                    let _ = prune_core::disk::project::prune_ledger(
                        &store,
                        now,
                        prune_core::disk::project::LEDGER_KEEP_DAYS,
                    );
                    if !json {
                        eprintln!("项目账本：{day} 记入 {n} 个项目（prune projects 可看趋势）");
                    }
                }
                Ok(_) => {}
                Err(e) => eprintln!("项目账本落库失败（不影响扫描结果）: {e}"),
            }
        }
    }

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&docs_json::scan_doc(&report)).unwrap()
        );
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

fn cmd_reclaim(yes: bool, emit_script: bool) -> i32 {
    let home = home_dir();
    eprintln!("扫描中…");
    let report = disk::scan(&home, None);

    // v1 绿级 = A1 死重条目（L1，零风险）；整库清理不进 CLI（D-C4）
    let pv = reclaim::preview(&["yarn".into()], &report);

    // D-15（DP-4 裁 A）：--emit-script 只打印候选删除命令的可审计脚本。
    // 此分支提前 return——本命令在 emit 模式下结构上不接触 reclaim::execute，
    // 「无执行路径」由 emit_script.rs 的源码断言测试守门。
    if emit_script {
        return emit_script::run(&pv, &home, &ts_full(prune_core::swap::now_secs() as i64));
    }

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

/// `prune mem --csv <path>`（F20，docs/f20-memory-report-plan.md）：导出内存
/// 报告 CSV。读序列库历史（7 天窗口 = 库的真相边界），**不触发扫描**；
/// 空库输出指路文案并正常退出（0）——不是错误。聚合与 CSV 渲染都在核心层
/// `mem::report` 纯函数，GUI 导出走同一函数（逐字节一致是验收项）。
fn cmd_mem_csv(path: &str) -> i32 {
    let db_path = mcp::default_db_path();
    if !db_path.exists() {
        println!(
            "暂无序列数据（{} 不存在）：先跑一次 GUI 采样或 prune scan，之后导出才有内容。",
            db_path.display()
        );
        return 0;
    }
    let store = match Store::open(&db_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("序列库打开失败: {e}");
            return 4;
        }
    };
    let rows = match store.mem_report_rows(0) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("读序列失败: {e}");
            return 4;
        }
    };
    if rows.is_empty() {
        println!("暂无序列数据（库为空）：先跑一次 GUI 采样，之后导出才有内容。");
        return 0;
    }
    let report = mem::report::aggregate(&rows);
    let now = swap::now_secs() as i64;
    let csv = mem::report::to_csv(&report, now, env!("CARGO_PKG_VERSION"));
    if let Err(e) = std::fs::write(path, csv) {
        eprintln!("写文件失败（{path}）: {e}");
        return 4;
    }
    println!(
        "已导出 {} 个进程 / {} 段空窗 → {path}（窗口 {} → {}）",
        report.procs.len(),
        report.gaps.len(),
        report.from_ts,
        report.to_ts
    );
    0
}

fn cmd_mem(swapped: bool, json: bool) -> i32 {
    eprintln!("采样中（深探 Top 20，约 1-2s）…");
    let procs = mcp::watch_round(20);

    let swap_info = swap::collect(None);
    let pressure = mem::system::read_pressure();

    if json {
        let doc = docs_json::mem_doc(
            &swap_info,
            pressure,
            &mem::aggregate(&procs),
            &mem::top_swapped(&procs, 5),
        );
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
///
/// `prune why <port>`（F16）：白话解释。结论只产自 `net::why::why()` 纯函数，
/// 本函数只负责排版与退出码——文案逻辑不许散落在 CLI 层。
fn cmd_why(port: u16, json: bool) -> i32 {
    let snap = net::ports::scan(None);
    let verdict = net::why::why(port, &snap);
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&docs_json::why_doc(&verdict)).unwrap()
        );
        return exit_code_for_empty(snap.coverage.note.as_deref());
    }
    println!("端口 {}：{}", verdict.port, verdict.headline);
    for line in &verdict.evidence {
        println!("  · {line}");
    }
    if let Some(hint) = &verdict.action_hint {
        println!("  ↳ {hint}");
    }
    // 与 `port --json` 同一退出码纪律：Coverage 差额是数据事实（0），
    // 权限不足（3）/ 采集失败（4）照旧区分
    exit_code_for_empty(snap.coverage.note.as_deref())
}

/// `prune port --drift`（F22，docs/f22-port-baseline-plan.md）：相对可信基线的漂移。
///
/// 基线落在 db `port_baseline` 表，GUI 采样轮与 CLI 共享同一张表——谁先跑谁建基线，
/// 两边口径同源（`baseline::diff/merge` 纯函数）。判据只允许在核心层一处。
/// `--rebase` 显式重建（不做自动老化——基线静默漂移违背「可信」二字）。
fn cmd_port_drift(snap: &net::PortSnapshot, rebase: bool, json: bool) -> i32 {
    let now = snap.at;
    let store = match Store::open(&mcp::default_db_path()) {
        Ok(s) => s,
        Err(e) => {
            let note = format!("基线库打开失败: {e}");
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&docs_json::port_drift_doc(
                        false,
                        rebase,
                        serde_json::json!([]),
                        &[]
                    ))
                    .unwrap()
                );
            } else {
                println!("{note}");
            }
            return 4;
        }
    };

    if rebase {
        let merged = net::baseline::merge(&[], snap, now);
        return match store.port_baseline_replace(&merged) {
            Ok(n) => {
                if json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&docs_json::port_drift_doc(
                            false,
                            true,
                            serde_json::json!([]),
                            &[]
                        ))
                        .unwrap()
                    );
                } else {
                    println!("基线已重建：{n} 个监听记为基线（首次出现时刻 = 现在）。");
                }
                0
            }
            Err(e) => {
                eprintln!("基线落库失败: {e}");
                4
            }
        };
    }

    let baseline = match store.port_baseline_all() {
        Ok(b) => b,
        Err(e) => {
            eprintln!("基线读取失败: {e}");
            return 4;
        }
    };
    let report = net::baseline::diff(&baseline, snap, now);
    if report.seeded {
        // 首扫：全部记为基线，不报漂移（首轮 N 个「漂移」是谎报）
        let merged = net::baseline::merge(&baseline, snap, now);
        if let Err(e) = store.port_baseline_replace(&merged) {
            eprintln!("基线落库失败: {e}");
        }
        if json {
            println!(
                "{}",
                serde_json::to_string_pretty(&docs_json::port_drift_doc(
                    true,
                    false,
                    serde_json::json!([]),
                    &[]
                ))
                .unwrap()
            );
        } else {
            println!(
                "首轮：当前 {} 个监听已记为基线。之后用 prune port --drift 看漂移。",
                snap.entries.len()
            );
        }
        return 0;
    }

    // flagged tuple → 带 comm 的展示条目（core 的 tuple 形态不带 comm，这层翻译）
    let mut drifts: Vec<(u16, String, String, String)> = Vec::new(); // (port, proto, comm, kind)
    for (port, proto, kind) in &report.flagged {
        let comm = snap
            .entries
            .iter()
            .find(|e| e.port == *port && e.proto == *proto)
            .and_then(|e| e.primary())
            .map(|b| b.comm.clone())
            .unwrap_or_default();
        let proto_s = match proto {
            net::Proto::Tcp => "tcp",
            net::Proto::Udp => "udp",
        };
        let kind_s = match kind {
            net::baseline::DriftKind::New => "new",
            net::baseline::DriftKind::Widened => "widened",
        };
        drifts.push((*port, proto_s.into(), comm, kind_s.into()));
    }
    drifts.sort();

    // 比对完成即落基线（last_seen 刷新 / 新行 first_seen / 消失行保留）
    let merged = net::baseline::merge(&baseline, snap, now);
    if let Err(e) = store.port_baseline_replace(&merged) {
        eprintln!("基线落库失败: {e}");
    }

    let drift_json = serde_json::Value::Array(
        drifts
            .iter()
            .map(
                |(p, pr, c, k)| serde_json::json!({ "port": p, "proto": pr, "comm": c, "kind": k }),
            )
            .collect(),
    );
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&docs_json::port_drift_doc(
                false,
                false,
                drift_json,
                &report.removed
            ))
            .unwrap()
        );
        return 0;
    }

    if drifts.is_empty() && report.removed.is_empty() {
        println!("与基线一致：没有新增监听，也没有暴露面变宽。");
        return 0;
    }
    println!(
        "相对基线漂移 {} 项（消失 {} 项）：",
        drifts.len(),
        report.removed.len()
    );
    for (port, proto, comm, kind) in &drifts {
        let kind_cn = match kind.as_str() {
            "widened" => "暴露面变宽",
            _ => "新监听",
        };
        println!("  {kind_cn}  {port}/{proto}  {comm}");
    }
    for r in &report.removed {
        println!("  消失      {r}");
    }
    println!("  重建基线：prune port --drift --rebase");
    0
}

/// `prune files <PATH>`（F23）：Open Files 占用反查——谁占着这块盘。
///
/// 只读纪律：只查不解，解除占用（关 App / 强制弹出）是用户的决定。
/// Coverage 纪律：coverage_note 永远随结果输出——「没有可见的占用者」
/// ≠「没人占用」。文案逻辑不出核心层（`open_files::parse` 纯函数）。
fn cmd_files(path: &str, json: bool) -> i32 {
    if !std::path::Path::new(path).exists() {
        eprintln!("路径不存在：{path}");
        return 2;
    }
    let report = match disk::open_files::scan(path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{e}");
            return 4;
        }
    };
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&docs_json::files_doc(&report)).unwrap()
        );
        return 0;
    }
    println!(
        "{} 的占用者 {} 个（{} ms）：",
        report.path,
        report.holders.len(),
        report.scanned_ms
    );
    for h in &report.holders {
        println!("  {} (pid {})  {} 个文件", h.comm, h.pid, h.files);
        for f in &h.sample_files {
            println!("      {f}");
        }
    }
    if report.holders.is_empty() {
        println!("  没有**可见的**占用者。");
    }
    if let Some(note) = &report.coverage_note {
        println!("  ⚠️ {note}");
    }
    0
}

fn cmd_port(
    only: Option<u16>,
    json: bool,
    all: bool,
    conflicts_only: bool,
    drift: bool,
    rebase: bool,
) -> i32 {
    let snap = net::ports::scan(None);
    if drift {
        return cmd_port_drift(&snap, rebase, json);
    }

    // `--conflicts`：只回答「此刻谁和谁抢端口」。与 `island` 的端口事件同一判据
    // （`owner_count() > 1`），区别是这里给**现状**，那边给**变化**。
    if conflicts_only {
        let entries: Vec<&net::PortEntry> = snap
            .entries
            .iter()
            .filter(|e| only.map(|p| e.port == p).unwrap_or(true))
            .collect();
        let owned: Vec<net::PortEntry> = entries.into_iter().cloned().collect();
        let list = net::ports::conflicts(&owned);
        if json {
            let doc = docs_json::port_conflicts_doc(snap.at, &list, &snap.coverage);
            println!("{}", serde_json::to_string_pretty(&doc).unwrap());
            return 0;
        }
        if list.is_empty() {
            println!("当前没有端口冲突（同一端口 ≥2 个不同 pid 才算）。");
            return 0;
        }
        println!("冲突端口 {} 个：", list.len());
        for c in &list {
            println!(
                "  {}  {} 个持有者  暴露面 {}",
                c.port,
                c.owners,
                exposure_cn(c.exposure)
            );
            for b in &c.listeners {
                let pid = b
                    .pid
                    .map(|p| p.to_string())
                    .unwrap_or_else(|| "内核".into());
                println!(
                    "      {} (pid {})  绑定 {}{}",
                    b.comm,
                    pid,
                    b.addr,
                    b.project_label
                        .as_ref()
                        .map(|l| format!("  · {l}"))
                        .unwrap_or_default()
                );
            }
        }
        return 0;
    }

    if json {
        let entries: Vec<&net::PortEntry> = snap
            .entries
            .iter()
            .filter(|e| only.map(|p| e.port == p).unwrap_or(all || e.watched))
            .collect();
        let owned: Vec<net::PortEntry> = entries.into_iter().cloned().collect();
        let doc = docs_json::port_doc(only, &owned, &snap);
        println!("{}", serde_json::to_string_pretty(&doc).unwrap());
        return exit_code_for_empty(snap.coverage.note.as_deref());
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
            // F21 入站来源：计数是口径，明细 ≤5 条给人看，余量诚实标注
            if let Some(pp) = &e.inbound {
                println!(
                    "  入站    {} 条：本机 {} · 局域网 {} · 公网 {}{}",
                    pp.total(),
                    pp.loopback,
                    pp.lan,
                    pp.public,
                    if pp.unknown > 0 {
                        format!(" · 未知 {}", pp.unknown)
                    } else {
                        String::new()
                    }
                );
                for p in pp.detail.iter().take(5) {
                    println!("      <- {} ({})", p.addr, p.scope.label_cn());
                }
                let more = pp.more();
                if more > 0 {
                    println!("      … 还有 {more} 个对端");
                }
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
        // 信封与缓存分离（wave1 方案 §3.2）：缓存写裸 LanSnapshot（GUI/MCP 读），
        // CLI 打印走 lan_doc 包信封。
        let doc = docs_json::lan_doc(&snap);
        println!("{}", serde_json::to_string_pretty(&doc).unwrap());
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
                let doc = docs_json::app_doc(&r);
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

/// `prune mem --watch` 的旧实现（DP-1 裁 A：保留为别名，2027-09 移除）。
/// 新代码请用 `prune watch --only mem`——那边统一了四类事件与 JSONL 契约。
fn cmd_mem_watch_legacy(json: bool) -> i32 {
    let _ = json;
    cmd_watch_compat(10)
}

fn cmd_watch_compat(interval: u64) -> i32 {
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

pub(crate) fn ts_hms(ts: i64) -> String {
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

/// 本地时间全串（D-15 emit-script 脚本头用）：`YYYY-MM-DD HH:MM:SS`。
/// libc 失败时回退 unix 时间戳——不伪造日期。
fn ts_full(ts: i64) -> String {
    #[cfg(unix)]
    unsafe {
        let t = ts as libc::time_t;
        let tm = libc::localtime(&t);
        if !tm.is_null() {
            return format!(
                "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
                (*tm).tm_year + 1900,
                (*tm).tm_mon + 1,
                (*tm).tm_mday,
                (*tm).tm_hour,
                (*tm).tm_min,
                (*tm).tm_sec
            );
        }
    }
    format!("unix:{ts}")
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
    use prune_core::disk::project::ProjectPoint as DiskProjectPoint;

    /// 退出码 usage 行 ↔ 枚举逐项一致（防再出现「文档写了、代码没有」，
    /// wave1 方案 §3.3；检查 E 的 Rust 侧镜像）。
    #[test]
    fn usage_exit_codes_match_enum() {
        let line = exit_code_line();
        for (code, desc) in ExitCode::ALL {
            assert!(
                line.contains(&format!("{} {}", *code as i32, desc)),
                "usage 退出码行缺 {code:?}（{desc}）：{line}"
            );
        }
        // 5 项之外不允许私货：数字集合必须恰好是枚举成员
        for n in [1, 5, 6, 7, 8, 9] {
            assert!(
                !line.contains(&format!("/ {n} ")) && !line.contains(&format!(" {n} /")),
                "usage 退出码行出现了枚举之外的码 {n}：{line}"
            );
        }
    }

    /// 退出码 3 / 4 的产生判据（W1-DP-2）：「没查到」必须能与
    /// 「没权限」「探不了」区分；差额说明（部分不可见）仍是 0。
    #[test]
    fn exit_code_for_empty_separates_permission_from_probe_failure() {
        assert_eq!(exit_code_for_empty(None), 0);
        assert_eq!(exit_code_for_empty(Some("")), 0);
        // 差额说明是数据事实，不是进程失败
        assert_eq!(
            exit_code_for_empty(Some(
                "有 3 个监听端口无法归属到进程（可能属于其他用户或系统进程）"
            )),
            0
        );
        assert_eq!(
            exit_code_for_empty(Some("无法读取进程的 socket 信息（权限不足）")),
            3
        );
        assert_eq!(
            exit_code_for_empty(Some("端口采集失败：lsof 无法执行：No such file")),
            4
        );
    }

    /// 契约冻结（wave1 方案 §3.4）：`--json` 顶层键集合与
    /// `cli-contract.v1.json` 逐字段相等。改任何 docs_json 函数的键集合
    /// 都会让这里红——「改契约」因此成为一个显式动作。
    #[test]
    fn cli_contract_matches_frozen_snapshot() {
        use serde_json::Value;
        let frozen: Value = serde_json::from_str(include_str!("../cli-contract.v1.json"))
            .expect("cli-contract.v1.json 必须是合法 JSON");
        let actual = docs_json::contract_shape();
        assert_eq!(
            actual, frozen,
            "CLI 契约已变更：同批更新 crates/prune-cli/cli-contract.v1.json（破坏性变更需提升 schemaVersion 并在 CHANGELOG 写明）"
        );
    }

    /// 快照导出工具：`PRUNE_DUMP_CONTRACT=1 cargo test -p prune-cli dump_contract`
    /// 用生产函数重写冻结文件。绝不手写快照。
    #[test]
    fn dump_contract_when_env_set() {
        if std::env::var("PRUNE_DUMP_CONTRACT").is_err() {
            return;
        }
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/cli-contract.v1.json");
        std::fs::write(
            path,
            serde_json::to_string_pretty(&docs_json::contract_shape()).unwrap() + "\n",
        )
        .unwrap();
        eprintln!("已导出契约快照：{path}");
    }

    fn s(v: &str) -> String {
        v.to_string()
    }

    /// 造一个只关心「变化 / 当前体积」的趋势，用来测人读排序
    fn trend(name: &str, size: u64, delta: Option<i64>, samples: u32) -> DiskTrend {
        let point = |bytes: u64| DiskProjectPoint {
            day: "2026-11-16".into(),
            deps_bytes: bytes,
            deps_age_days: 30,
            activity_days: Some(1),
            lockfiles: vec!["pnpm".into()],
            verdict: "heavy".into(),
        };
        DiskTrend {
            root: format!("/w/{name}"),
            name: name.into(),
            points: vec![point(size)],
            first: Some(point(size)),
            last: Some(point(size)),
            delta_bytes: delta,
            peak_bytes: Some(size),
            samples,
        }
    }

    #[test]
    fn readout_puts_the_bigger_movement_first() {
        let big_static = trend("big-static", 20 << 30, Some(0), 2);
        let shrank = trend("shrank", 0, Some(-(2 << 30)), 2);
        let grew = trend("grew", 3 << 30, Some(8 << 30), 2);
        let unmeasured = trend("unmeasured", 40 << 30, None, 1);

        let refs = vec![big_static, shrank, grew, unmeasured];
        let order: Vec<&str> = readout_order(&refs)
            .iter()
            .map(|t| t.name.as_str())
            .collect();
        assert_eq!(
            order,
            vec!["grew", "shrank", "big-static", "unmeasured"],
            "先按 |变化| 排；变化的绝对值相同才看体积；不可测的垫底——\
             否则「昨天 2 GB 今天 0 B」这种唯一有新闻的项目会沉到最后一行"
        );
    }

    #[test]
    fn readout_caps_long_lists_but_says_how_many_were_hidden() {
        let list: Vec<DiskTrend> = (0..25)
            .map(|i| trend(&format!("p{i:02}"), (i as u64 + 1) << 20, Some(i as i64), 2))
            .collect();
        let lines = readout_lines(&list);
        // 表头 1 + 至多 READOUT_MAX 行 + 省略说明 1
        assert_eq!(lines.len(), READOUT_MAX + 2);
        let last = lines.last().unwrap();
        assert!(
            last.contains("另有 5 个项目未列出") && last.contains("--json"),
            "少列必须明说少列了几条，并指路全量：{last}"
        );

        // 不超上限时不出现省略行
        let short: Vec<DiskTrend> = vec![trend("only", 1 << 20, Some(1), 2)];
        let lines = readout_lines(&short);
        assert_eq!(lines.len(), 2);
        assert!(!lines.iter().any(|l| l.contains("未列出")));
    }

    #[test]
    fn readout_states_insufficient_data_instead_of_zero() {
        let lines = readout_lines(&[trend("fresh", 1 << 30, None, 1)]);
        assert!(
            lines[1].contains("数据不足（1 天）"),
            "单点样本不能显示成 0 变化：{}",
            lines[1]
        );
    }

    #[test]
    fn parses_all_subcommands() {
        assert_eq!(
            parse_args(&[s("scan"), s("--json")]).unwrap(),
            Command::Scan { json: true }
        );
        assert_eq!(
            parse_args(&[s("reclaim"), s("--level"), s("green")]).unwrap(),
            Command::Reclaim {
                yes: false,
                emit_script: false
            }
        );
        assert_eq!(
            parse_args(&[s("reclaim"), s("--yes")]).unwrap(),
            Command::Reclaim {
                yes: true,
                emit_script: false
            }
        );
        assert_eq!(
            parse_args(&[s("reclaim"), s("--emit-script")]).unwrap(),
            Command::Reclaim {
                yes: false,
                emit_script: true
            }
        );
        // D-15：打印与执行语义互斥，同给必须用法错
        assert!(parse_args(&[s("reclaim"), s("--emit-script"), s("--yes")]).is_err());
        assert_eq!(
            parse_args(&[s("mem"), s("--swapped")]).unwrap(),
            Command::Mem {
                swapped: true,
                json: false,
                watch: None,
                csv: None,
            }
        );
        assert_eq!(
            parse_args(&[s("mem"), s("--watch")]).unwrap(),
            Command::Mem {
                swapped: false,
                json: false,
                watch: Some(10),
                csv: None,
            }
        );
        assert_eq!(
            parse_args(&[s("mem"), s("--watch"), s("--interval"), s("5")]).unwrap(),
            Command::Mem {
                swapped: false,
                json: false,
                watch: Some(5),
                csv: None,
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
                all: false,
                conflicts: false,
                drift: false,
                rebase: false,
            }
        );
        assert_eq!(
            parse_args(&[s("port"), s("3000")]).unwrap(),
            Command::Port {
                port: Some(3000),
                json: false,
                all: false,
                conflicts: false,
                drift: false,
                rebase: false,
            }
        );
        assert_eq!(
            parse_args(&[s("port"), s("3000"), s("--json"), s("--all")]).unwrap(),
            Command::Port {
                port: Some(3000),
                json: true,
                all: true,
                conflicts: false,
                drift: false,
                rebase: false,
            }
        );
        // 端口 0 是通配 fd，不是服务：命令行上接受但扫描层会丢掉
        assert!(parse_args(&[s("port"), s("0")]).is_ok());
        // 越界 / 非数字 / 两个端口号都要拒绝，否则会静默查错端口
        assert!(parse_args(&[s("port"), s("70000")]).is_err());
        assert!(parse_args(&[s("port"), s("abc")]).is_err());
        assert!(parse_args(&[s("port"), s("3000"), s("8080")]).is_err());
    }

    /// F16：why 子命令解析（端口必填、--json 可选、多余参数拒绝）
    #[test]
    fn parses_why_subcommand() {
        assert_eq!(
            parse_args(&[s("why"), s("5000")]).unwrap(),
            Command::Why {
                port: 5000,
                json: false
            }
        );
        assert_eq!(
            parse_args(&[s("why"), s("3000"), s("--json")]).unwrap(),
            Command::Why {
                port: 3000,
                json: true
            }
        );
        // 缺端口号 → 用法错；多余端口号 → 用法错；非数字 → 用法错
        assert!(parse_args(&[s("why")]).is_err());
        assert!(parse_args(&[s("why"), s("3000"), s("8080")]).is_err());
        assert!(parse_args(&[s("why"), s("abc")]).is_err());
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
