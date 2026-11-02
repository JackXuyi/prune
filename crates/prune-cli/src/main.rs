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

use prune_core::disk::{self, CacheGroup};
use prune_core::disk::whitelist::Level;
use prune_core::leak;
use prune_core::mem;
use prune_core::reclaim::{self, Limits, ReclaimError};
use prune_core::swap;

// ---------------------------------------------------------------- 参数解析

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Scan { json: bool },
    Reclaim { yes: bool },
    Mem { swapped: bool, json: bool, watch: Option<u64> },
    Mcp,
    Version,
}

pub fn parse_args(args: &[String]) -> Result<Command, String> {
    let mut it = args.iter();
    let sub = it.next().map(|s| s.as_str()).ok_or_else(|| usage())?;

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
                        let v = all.get(i).ok_or_else(|| "--interval 需要一个秒数".to_string())?;
                        watch = Some(
                            v.parse::<u64>()
                                .map_err(|_| format!("--interval 不是数字: {v:?}"))?,
                        );
                    }
                    other => return Err(format!("mem 不认识参数 {other:?}\n{}", usage())),
                }
                i += 1;
            }
            Ok(Command::Mem { swapped, json, watch })
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
        Command::Mem { swapped, json, watch } => match watch {
            Some(interval) => cmd_watch(interval),
            None => cmd_mem(swapped, json),
        },
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
        println!(
            "（另有 L4 Docker {}，不自动处理，见文末命令）",
            hb(t.l4)
        );
    }
    println!();
    println!("{:<34}{:>12}  {:>8}  {}", "分组", "体积", "条目", "说明");
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
                println!("  {:<10} {:>4} 条  {:>10}", s.platform, s.entry_count, hb(s.bytes));
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
    println!("将清理 {} 项，合计 {}：", pv.items.len(), hb(pv.total_bytes));
    for it in &pv.items {
        println!("  {:>10}  {}", hb(it.bytes), it.path);
    }
    for n in &pv.notes {
        println!("  ℹ️  {n}");
    }

    if !yes {
        println!();
        println!("以上为预览（dry-run）。确认无误后追加 --yes 执行（移入废纸篓，可恢复）。");
        return 0;
    }

    // CLI v1 只做 L1（A1 死重条目），静态白名单足够；报告派生白名单留给桌面端 A4/A5
    match reclaim::execute(&pv.items, &home, true, Limits::default(), &[]) {
        Ok(rep) => {
            println!(
                "已移入废纸篓 {} 项，{}。",
                rep.moved,
                hb(rep.bytes)
            );
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
    println!(
        "物理 {} · 压缩器 {} · swap {}/{}（{:.0}%） · 压力 {:?}",
        hb(total),
        hb(mem::system::read().map(|s| s.compressor_bytes()).unwrap_or(0)),
        hb(swap_info.used),
        hb(swap_info.total),
        swap_info.used_ratio() * 100.0,
        pressure.level
    );
    println!();
    let head = if swapped {
        format!("{:<22}{:>11}{:>11}{:>6}  {}", "应用家族", "占用", "其中换出", "进程", "口径")
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
            return format!("{:02}:{:02}:{:02}", (*tm).tm_hour, (*tm).tm_min, (*tm).tm_sec);
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
            Command::Mem { swapped: true, json: false, watch: None }
        );
        assert_eq!(
            parse_args(&[s("mem"), s("--watch")]).unwrap(),
            Command::Mem { swapped: false, json: false, watch: Some(10) }
        );
        assert_eq!(
            parse_args(&[s("mem"), s("--watch"), s("--interval"), s("5")]).unwrap(),
            Command::Mem { swapped: false, json: false, watch: Some(5) }
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
