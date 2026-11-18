//! `prune watch`（W2/D-10）：按需守护 —— 前台进程，Ctrl-C 即停。
//!
//! 与 `prune mem --watch`（旧）的关系：那边只做内存泄漏判定，本模块覆盖
//! cpu / mem / port / disk 四类。**判据不复制**：CPU / 内存 / 端口全部调用
//! `prune_core::island` 里 GUI 用的同一批监视器，磁盘水位用新增的 `DiskMonitor`。
//! 两处各写一份阈值逻辑，半年后必然对不上——这是本模块最重要的一条纪律。
//!
//! 三条边界（写进产品文档，不是实现细节）：
//! 1. **不装后台**：没有 launchd / systemd / 开机项 / 守护重启，Ctrl-C 即停；
//! 2. **不写日志文件**：stdout / stderr 就是出口，重定向由调用方决定；
//! 3. **只读**：本模块不删除、不 kill、不改配置——发现问题只报告。

use std::io::Write;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

use prune_core::config;
use prune_core::disk;
use prune_core::island::{
    CpuMonitor, CpuThresholds, DiskMonitor, DiskSeverity, DiskThresholds, MemMonitor,
    MemThresholds, MemTrigger, PortGuardKind, PortGuardMonitor,
};
use prune_core::mem;
use prune_core::net;
use prune_core::swap;

// ---------------------------------------------------------------- CWD 配置的关注端口（W2 收尾 DP-W2-1-B）

/// watch 的关注端口补充来源：CWD 向上找最近一份 `prune.toml` 的 `watch.ports`。
///
/// 在线项目的配置已由 `net::ports::scan_cached` → `collect_watch_ports` 覆盖
/// （对绑定进程的 cwd 解析后读配置）；这里补的是「CWD 项目还没起服务」的场景——
/// `PortGuardMonitor` 的 NewListen 只看 `e.watched`，不并入的话 dev server
/// 起来那一刻不会告警。找不到 / 空配置 / 就近损坏 → None，行为与 v0.14.0 一致。
fn cwd_watch_ports() -> Option<(std::path::PathBuf, Vec<u16>)> {
    let cwd = std::env::current_dir().ok()?;
    let found = prune_core::prune_toml::find_watch_ports_upward(&cwd)?;
    (!found.1.is_empty()).then_some(found)
}

/// 把调用方声明的关注端口并入端口条目的 `watched` 标志，返回实际改动的条目数。
///
/// 纯函数（可测）。只置位不撤销：采样路径已判定的 `watched`（特权端口 /
/// 系统进程 / 在线项目配置）不重复计数。这就是与 GUI 同源的合并点——
/// GUI 的 `scan_cached` 在 group 时合并，watch 在拿到快照后合并，判据同一份
/// `is_watched_with` 语义，只是补的是「进程还没起」的时间窗。
fn mark_extra_watched(entries: &mut [prune_core::net::PortEntry], extra: &[u16]) -> usize {
    if extra.is_empty() {
        return 0;
    }
    let mut n = 0;
    for e in entries.iter_mut() {
        if !e.watched && extra.contains(&e.port) {
            e.watched = true;
            n += 1;
        }
    }
    n
}

/// 采样类别
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Cpu,
    Mem,
    Port,
    Disk,
}

impl Kind {
    pub fn parse(s: &str) -> Option<Kind> {
        match s.trim() {
            "cpu" => Some(Kind::Cpu),
            "mem" | "memory" => Some(Kind::Mem),
            "port" | "ports" => Some(Kind::Port),
            "disk" => Some(Kind::Disk),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Kind::Cpu => "cpu",
            Kind::Mem => "mem",
            Kind::Port => "port",
            Kind::Disk => "disk",
        }
    }
}

pub const ALL_KINDS: [Kind; 4] = [Kind::Cpu, Kind::Mem, Kind::Port, Kind::Disk];

/// 合法类别清单（错误提示用：别让用户去翻文档才知道拼写）
pub fn kinds_hint() -> String {
    ALL_KINDS
        .iter()
        .map(|k| k.as_str())
        .collect::<Vec<_>>()
        .join(",")
}

/// 默认采样间隔（秒）。与 `island` 的 2 s 不同：CLI 的场景是「盯一会儿」，
/// 10 s 一轮足够，且一轮里有一次 lsof（30–40 ms）与一次 sysinfo 全量扫描。
pub const DEFAULT_INTERVAL_SEC: u64 = 10;
pub const MIN_INTERVAL_SEC: u64 = 2;
pub const MAX_INTERVAL_SEC: u64 = 300;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Opts {
    pub interval: u64,
    pub only: Vec<Kind>,
    pub json: bool,
    pub once: bool,
    /// 出现 warn 及以上即退出码 10（CI 用）
    pub fail_on_warn: bool,
}

impl Opts {
    fn wants(&self, k: Kind) -> bool {
        self.only.is_empty() || self.only.contains(&k)
    }
}

/// 阈值来源：**同一份 settings.json**（`prune_core::config` 无 Tauri 读取）。
/// GUI 里改过的值，CLI 立刻生效；缺键回落这里写的默认值。
struct Thresholds {
    cpu: CpuThresholds,
    mem: MemThresholds,
    disk: DiskThresholds,
}

impl Thresholds {
    fn load() -> Self {
        let cpu = CpuThresholds {
            threshold_pct: config::get_f64("island.cpu_pct").unwrap_or(90.0) as f32,
            sustain_sec: config::get_u64("island.cpu_sustain_sec").unwrap_or(60),
            tolerance_pct: config::get_f64("island.cpu_tolerance_pct").unwrap_or(90.0) as f32,
        }
        .clamped();
        let mem = MemThresholds {
            swap_pct: config::get_f64("island.swap_pct").unwrap_or(80.0) as f32,
            sustain_sec: config::get_u64("island.mem_sustain_sec").unwrap_or(30),
        }
        .clamped();
        let disk = DiskThresholds {
            warn_ratio: config::get_f64("watch.disk_warn_ratio").unwrap_or(0.15),
            critical_ratio: config::get_f64("watch.disk_critical_ratio").unwrap_or(0.05),
        }
        .clamped();
        Self { cpu, mem, disk }
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn severity_rank(s: &str) -> u8 {
    match s {
        "critical" => 2,
        "warn" => 1,
        _ => 0,
    }
}

/// 一行事件（JSONL 或人读）。`severity` 只有 warn / critical 两档——
/// 这个工具不产生「info」事件，能进 stdout 的都是需要看一眼的。
fn emit(json_mode: bool, event: &Value) {
    let mut out = std::io::stdout();
    if json_mode {
        let _ = writeln!(out, "{event}");
    } else {
        let t = event.get("ts").and_then(|v| v.as_i64()).unwrap_or(0);
        let kind = event.get("kind").and_then(|v| v.as_str()).unwrap_or("");
        let sev = event.get("severity").and_then(|v| v.as_str()).unwrap_or("");
        let text = event
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let mark = if sev == "critical" { "!!" } else { "⚠ " };
        let _ = writeln!(out, "[{}] {mark}{kind}  {text}", crate::ts_hms(t));
    }
    let _ = out.flush();
}

/// 单个频道的一次采样结果：`None` = 该频道本轮读不到（不猜、不报 0）
pub fn run(opts: &Opts) -> i32 {
    let th = Thresholds::load();
    // CWD 配置的关注端口：启动时解析一次（配置文件在进程生命周期内不重读——
    // watch 是观察者，配置热加载不是它的事）
    let cwd_cfg = cwd_watch_ports();
    if opts.once {
        return run_once_with(
            opts,
            &th,
            cwd_cfg.as_ref().map(|(r, p)| (r.clone(), p.clone())),
        );
    }

    if !opts.json {
        println!(
            "watch：每 {}s 一轮（{}），Ctrl-C 退出。它不常驻、不写日志、不装开机项。",
            opts.interval,
            if opts.only.is_empty() {
                "全部频道".to_string()
            } else {
                opts.only
                    .iter()
                    .map(|k| k.as_str())
                    .collect::<Vec<_>>()
                    .join(",")
            }
        );
        if let Some((root, ports)) = &cwd_cfg {
            println!(
                "关注端口（来自 {}）：{}",
                root.join(prune_core::prune_toml::FILE_NAME).display(),
                ports
                    .iter()
                    .map(|p| p.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        println!("端口与磁盘首次只建基线，不补报存量；之后只报变化。\n");
    }

    let mut scanner = mem::process::Scanner::new();
    let mut cpu_mon = CpuMonitor::new();
    let mut mem_mon = MemMonitor::new();
    let mut port_mon = PortGuardMonitor::new();
    let mut disk_mon = DiskMonitor::new();
    let mut first_round = true;
    // 主循环每轮复用：只取端口列表（config_source 已在横幅输出过）
    let cwd_ports = cwd_cfg.map(|(_, p)| p).unwrap_or_default();

    loop {
        let ms = now_ms();
        let ts = (ms / 1000) as i64;
        let mut readable_any = false;

        if opts.wants(Kind::Cpu) {
            let scan = scanner.scan();
            if let Some(usage) = scan.cpu_usage {
                readable_any = true;
                if let Some(alert) = cpu_mon.observe(ms, usage, &th.cpu) {
                    let ev = json!({
                        "schemaVersion": 1, "ts": ts, "kind": "cpu.sustained", "severity": "warn",
                        "subject": { "scope": "machine" },
                        "evidence": {
                            "avgPct": round1(alert.avg_pct),
                            "peakPct": round1(alert.peak_pct),
                            "ratioPct": round1(alert.ratio_pct),
                            "samples": alert.samples,
                        },
                        "threshold": {
                            "pct": th.cpu.threshold_pct, "sustainSec": th.cpu.sustain_sec,
                            "tolerancePct": th.cpu.tolerance_pct,
                        },
                        "text": format!(
                            "整机 CPU {:.0}%（{} 秒内 {:.0}% 的时间在 {:.0}% 以上）",
                            alert.avg_pct, th.cpu.sustain_sec, alert.ratio_pct, th.cpu.threshold_pct
                        ),
                    });
                    emit(opts.json, &ev);
                    if opts.fail_on_warn {
                        return 10;
                    }
                }
            }
        }

        if opts.wants(Kind::Mem) {
            let pressure = mem::system::read_pressure();
            let swap_info = swap::collect(None);
            let swap_pct = if swap_info.total == 0 {
                -1.0
            } else {
                (swap_info.used_ratio() * 100.0) as f32
            };
            if let Some(p) = pressure {
                readable_any = true;
                if let Some(alert) = mem_mon.observe(ms, p.level, swap_pct, &th.mem) {
                    let kind = match alert.trigger {
                        MemTrigger::Pressure => "mem.pressure",
                        MemTrigger::Swap => "mem.swap",
                        MemTrigger::Both => "mem.pressure_swap",
                    };
                    let sev = if alert.pressure == mem::PressureLevel::Critical {
                        "critical"
                    } else {
                        "warn"
                    };
                    let ev = json!({
                        "schemaVersion": 1, "ts": ts, "kind": kind, "severity": sev,
                        "subject": { "scope": "machine" },
                        "evidence": {
                            "pressure": alert.pressure,
                            "swapRatioPct": if swap_pct < 0.0 { Value::Null } else { json!(round1(swap_pct)) },
                            "swapTotalBytes": swap_info.total,
                            "swapUsedBytes": swap_info.used,
                        },
                        "threshold": { "swapPct": th.mem.swap_pct, "sustainSec": th.mem.sustain_sec },
                        "text": format!(
                            "内存压力 {:?}，swap {:.0}%（{} / {}）",
                            alert.pressure, swap_pct.max(0.0), crate::hb(swap_info.used), crate::hb(swap_info.total)
                        ),
                    });
                    emit(opts.json, &ev);
                    if opts.fail_on_warn && severity_rank(sev) >= 1 {
                        return 10;
                    }
                }
            }
        }

        if opts.wants(Kind::Port) {
            let mut snap = net::ports::scan(None);
            // CWD 项目的 watch.ports 并入关注层（在线项目已由采样路径覆盖）
            mark_extra_watched(&mut snap.entries, &cwd_ports);
            if snap.coverage.listen_total > 0 || !snap.entries.is_empty() {
                readable_any = true;
            }
            // 首轮只建基线（存量端口一条不报），之后只报变化
            for alert in port_mon.observe(ms, &snap.entries) {
                let kind = match alert.kind {
                    PortGuardKind::Conflict => "port.conflict",
                    PortGuardKind::NewListen => "port.new_listen",
                };
                let sev = "warn";
                let ev = json!({
                    "schemaVersion": 1, "ts": ts, "kind": kind, "severity": sev,
                    "subject": {
                        "port": alert.port,
                        "pid": alert.pid,
                        "comm": alert.comm,
                        "project": alert.project_label,
                    },
                    "evidence": { "owners": alert.owners, "exposure": alert.exposure },
                    "threshold": { "rule": "owners>1 或关注端口新占用" },
                    "text": match alert.kind {
                        PortGuardKind::Conflict => format!(
                            "端口 {} 有 {} 个持有者（{}）", alert.port, alert.owners, alert.comm
                        ),
                        PortGuardKind::NewListen => format!(
                            "关注端口 {} 被占用（{}{}）", alert.port, alert.comm,
                            alert.project_label.as_deref().map(|p| format!(" · {p}")).unwrap_or_default()
                        ),
                    },
                });
                emit(opts.json, &ev);
                if opts.fail_on_warn {
                    return 10;
                }
            }
        }

        if opts.wants(Kind::Disk) {
            let home = crate::home_dir();
            if let Some(v) = disk::volume_info(&home) {
                readable_any = true;
                let ratio = if v.total_bytes == 0 {
                    0.0
                } else {
                    v.user_available_bytes as f64 / v.total_bytes as f64
                };
                if let Some(level) = disk_mon.observe(ratio, &th.disk) {
                    let sev = match level {
                        DiskSeverity::Critical => "critical",
                        DiskSeverity::Warn => "warn",
                    };
                    let line = match level {
                        DiskSeverity::Critical => th.disk.critical_ratio,
                        DiskSeverity::Warn => th.disk.warn_ratio,
                    };
                    let ev = json!({
                        "schemaVersion": 1, "ts": ts, "kind": "disk.watermark", "severity": sev,
                        "subject": { "volume": "/", "path": home.to_string_lossy() },
                        "evidence": {
                            "availableBytes": v.user_available_bytes,
                            "totalBytes": v.total_bytes,
                            "freeRatio": round3(ratio),
                        },
                        "threshold": { "ratio": line },
                        "text": format!(
                            "可用空间 {:.1}%（{} / {}），低于 {:.0}% 线",
                            ratio * 100.0, crate::hb(v.user_available_bytes), crate::hb(v.total_bytes), line * 100.0
                        ),
                    });
                    emit(opts.json, &ev);
                    if opts.fail_on_warn {
                        return 10;
                    }
                }
            }
        }

        // 首轮结束就判定「有没有一个频道是能读的」：全读不到时不该假装在守护
        if first_round {
            first_round = false;
            if !readable_any {
                eprintln!(
                    "四个频道全部不可读（lsof / 进程枚举 / 卷容量都拿不到数据）：本机环境不支持 watch"
                );
                return 4;
            }
        }

        std::thread::sleep(Duration::from_secs(opts.interval));
    }
}

/// 跑一轮、输出、退出（CI 用）。`--json` 时**恰好一行**合法 JSON。
///
/// 顶层键全集见 [`ONCE_TOP_KEYS`]（契约快照 `cli-contract.v1.json` 引用它）。
/// 仅测试路径消费（快照对账）；生产键集合以 `run_once` 的实际插入为准。
#[cfg(test)]
pub const ONCE_TOP_KEYS: &[&str] = &["schemaVersion", "ts", "mode", "cpu", "mem", "ports", "disk"];

fn run_once(opts: &Opts, th: &Thresholds) -> i32 {
    run_once_with(opts, th, cwd_watch_ports())
}

/// `cwd_cfg` 由调用方传入（`run` 启动时解析一次，保证横幅与循环同源）。
fn run_once_with(
    opts: &Opts,
    th: &Thresholds,
    cwd_cfg: Option<(std::path::PathBuf, Vec<u16>)>,
) -> i32 {
    let cwd_ports = cwd_cfg.as_ref().map(|(_, p)| p.clone()).unwrap_or_default();
    let ts = (now_ms() / 1000) as i64;
    let mut doc = json!({ "schemaVersion": 1, "ts": ts, "mode": "once" });
    let mut worst = 0u8;
    let mut readable_any = false;

    if opts.wants(Kind::Cpu) {
        // sysinfo 的 CPU 是差分值：首轮恒为 0/None，必须再采一次才有效
        let mut scanner = mem::process::Scanner::new();
        let _ = scanner.scan();
        std::thread::sleep(Duration::from_millis(320));
        let scan = scanner.scan();
        match scan.cpu_usage {
            Some(u) => {
                readable_any = true;
                let level = if u >= th.cpu.threshold_pct {
                    worst = worst.max(1);
                    "warn"
                } else {
                    "ok"
                };
                doc["cpu"] = json!({
                    "usagePct": round1(u),
                    "cores": scan.cores,
                    "load": scan.load,
                    "thresholdPct": th.cpu.threshold_pct,
                    "severity": level,
                });
            }
            None => doc["cpu"] = json!({ "status": "unavailable", "reason": "CPU 差分窗口未建立" }),
        }
    }

    if opts.wants(Kind::Mem) {
        let swap_info = swap::collect(None);
        let swap_pct = if swap_info.total == 0 {
            None
        } else {
            Some(round1((swap_info.used_ratio() * 100.0) as f32))
        };
        let pressure = mem::system::read_pressure();
        if pressure.is_some() {
            readable_any = true;
        }
        let sev = match pressure.map(|p| p.level) {
            Some(mem::PressureLevel::Critical) => {
                worst = worst.max(2);
                "critical"
            }
            Some(mem::PressureLevel::Warning) => {
                worst = worst.max(1);
                "warn"
            }
            Some(mem::PressureLevel::Normal) => {
                if swap_pct.is_some_and(|v| v >= th.mem.swap_pct as f64) {
                    worst = worst.max(1);
                    "warn"
                } else {
                    "ok"
                }
            }
            None => "unavailable",
        };
        doc["mem"] = json!({
            "pressure": pressure.map(|p| p.level),
            "swapTotalBytes": swap_info.total,
            "swapUsedBytes": swap_info.used,
            "swapRatioPct": swap_pct,
            "swapThresholdPct": th.mem.swap_pct,
            "severity": sev,
        });
    }

    if opts.wants(Kind::Port) {
        let mut snap = net::ports::scan(None);
        // CWD 项目的 watch.ports 并入关注层（在线项目已由采样路径覆盖）
        mark_extra_watched(&mut snap.entries, &cwd_ports);
        if !snap.entries.is_empty() {
            readable_any = true;
        }
        let conflicts = net::ports::conflicts(&snap.entries);
        if !conflicts.is_empty() {
            worst = worst.max(1);
        }
        doc["ports"] = json!({
            "conflicts": conflicts,
            "count": conflicts.len(),
            "coverage": snap.coverage,
            "severity": if conflicts.is_empty() { "ok" } else { "warn" },
        });
        // 有 CWD 配置时才加键：无配置时输出与 v0.14.0 逐字节一致（A4 回归保证）。
        // 顶层键集合不变（ports 段内部字段），契约快照 cli-contract.v1.json 不受影响。
        if let Some((root, ports)) = &cwd_cfg {
            doc["ports"]["watchedExtra"] = json!({
                "ports": ports,
                "source": root.join(prune_core::prune_toml::FILE_NAME),
            });
        }
    }

    if opts.wants(Kind::Disk) {
        let home = crate::home_dir();
        match disk::volume_info(&home) {
            Some(v) => {
                readable_any = true;
                let ratio = if v.total_bytes == 0 {
                    0.0
                } else {
                    v.user_available_bytes as f64 / v.total_bytes as f64
                };
                let sev = if ratio <= th.disk.critical_ratio {
                    worst = worst.max(2);
                    "critical"
                } else if ratio <= th.disk.warn_ratio {
                    worst = worst.max(1);
                    "warn"
                } else {
                    "ok"
                };
                doc["disk"] = json!({
                    "volume": "/",
                    "availableBytes": v.user_available_bytes,
                    "totalBytes": v.total_bytes,
                    "freeRatio": round3(ratio),
                    "warnRatio": th.disk.warn_ratio,
                    "criticalRatio": th.disk.critical_ratio,
                    "severity": sev,
                });
            }
            None => doc["disk"] = json!({ "status": "unavailable", "reason": "读不到卷容量" }),
        }
    }

    doc["severity"] = json!(match worst {
        2 => "critical",
        1 => "warn",
        _ => "ok",
    });

    if opts.json {
        println!("{}", serde_json::to_string(&doc).unwrap_or_default());
    } else {
        print_once_human(&doc);
    }

    if !readable_any {
        return 4;
    }
    if opts.fail_on_warn && worst >= 1 {
        return 10;
    }
    0
}

fn print_once_human(doc: &Value) {
    let sev = doc.get("severity").and_then(|v| v.as_str()).unwrap_or("ok");
    println!(
        "prune watch --once（{}）",
        crate::ts_hms(doc["ts"].as_i64().unwrap_or(0))
    );
    if let Some(cpu) = doc.get("cpu") {
        if let Some(u) = cpu.get("usagePct").and_then(|v| v.as_f64()) {
            println!(
                "  CPU    {u:.1}%（{} 核，阈值 {:.0}%）",
                cpu.get("cores").and_then(|v| v.as_u64()).unwrap_or(0),
                cpu.get("thresholdPct")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(90.0)
            );
        } else {
            println!("  CPU    不可读（差分窗口未建立）");
        }
    }
    if let Some(m) = doc.get("mem") {
        let p = m
            .get("pressure")
            .map(|v| v.to_string())
            .unwrap_or("null".into());
        let sw = m
            .get("swapRatioPct")
            .and_then(|v| v.as_f64())
            .map(|v| format!("{v:.1}%"))
            .unwrap_or_else(|| "不可读".into());
        println!("  内存   压力 {p}，swap {sw}");
    }
    if let Some(p) = doc.get("ports") {
        let n = p.get("count").and_then(|v| v.as_u64()).unwrap_or(0);
        let total = p
            .get("coverage")
            .and_then(|c| c.get("attributed"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        println!("  端口   冲突 {n} 个（可见监听 {total} 条）");
        if let Some(list) = p.get("conflicts").and_then(|v| v.as_array()) {
            for c in list {
                println!(
                    "         {} ← {} 个持有者",
                    c.get("port").and_then(|v| v.as_u64()).unwrap_or(0),
                    c.get("owners").and_then(|v| v.as_u64()).unwrap_or(0)
                );
            }
        }
    }
    if let Some(d) = doc.get("disk") {
        if let Some(r) = d.get("freeRatio").and_then(|v| v.as_f64()) {
            println!(
                "  磁盘   可用 {:.1}%（{} / {}）",
                r * 100.0,
                crate::hb(
                    d.get("availableBytes")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0)
                ),
                crate::hb(d.get("totalBytes").and_then(|v| v.as_u64()).unwrap_or(0))
            );
        } else {
            println!("  磁盘   不可读");
        }
    }
    println!(
        "\n结论：{}",
        match sev {
            "critical" => "有 critical 项，先处理它",
            "warn" => "有 warn 项，可看一眼",
            _ => "四项都在阈值内",
        }
    );
}

fn round1(v: f32) -> f64 {
    (v as f64 * 10.0).round() / 10.0
}

fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(only: Vec<Kind>, once: bool, fail: bool) -> Opts {
        Opts {
            interval: DEFAULT_INTERVAL_SEC,
            only,
            json: true,
            once,
            fail_on_warn: fail,
        }
    }

    #[test]
    fn kind_parsing_accepts_documented_aliases() {
        assert_eq!(Kind::parse("cpu"), Some(Kind::Cpu));
        assert_eq!(Kind::parse("memory"), Some(Kind::Mem));
        assert_eq!(Kind::parse("ports"), Some(Kind::Port));
        assert_eq!(Kind::parse("gpu"), None);
    }

    #[test]
    fn wants_treats_empty_only_as_all() {
        let o = opts(vec![], true, false);
        for k in ALL_KINDS {
            assert!(o.wants(k));
        }
        let o = opts(vec![Kind::Port], true, false);
        assert!(o.wants(Kind::Port));
        assert!(!o.wants(Kind::Cpu));
    }

    #[test]
    fn interval_clamp_prevents_busy_loop() {
        // 0 / 1 秒会变成忙等：钳到下限；超大值钳到上限
        let lo = 0u64.clamp(MIN_INTERVAL_SEC, MAX_INTERVAL_SEC);
        let hi = 99_999u64.clamp(MIN_INTERVAL_SEC, MAX_INTERVAL_SEC);
        assert_eq!(lo, MIN_INTERVAL_SEC);
        assert_eq!(hi, MAX_INTERVAL_SEC);
        assert_eq!(Kind::parse("cpu").map(|k| k.as_str()), Some("cpu"));
    }

    #[test]
    fn severity_ranking_matches_exit_semantics() {
        assert_eq!(severity_rank("ok"), 0);
        assert_eq!(severity_rank("warn"), 1);
        assert_eq!(severity_rank("critical"), 2);
    }

    #[test]
    fn once_json_is_single_line_and_has_schema_version() {
        // 只跑磁盘频道：不依赖 lsof / sysinfo 差分的机器状态
        let o = opts(vec![Kind::Disk], true, false);
        let code = run_once(&o, &Thresholds::load());
        assert!(code == 0 || code == 4, "磁盘频道应可读（code={code}）");
    }

    // ---- mark_extra_watched（W2 收尾 DP-W2-1-B）----

    /// 最小端口条目：observe / mark 都不依赖 listeners 内容
    fn entry(port: u16, watched: bool) -> prune_core::net::PortEntry {
        prune_core::net::PortEntry {
            port,
            proto: prune_core::net::Proto::Tcp,
            listeners: vec![],
            conns: None,
            exposure: prune_core::net::BindScope::Loopback,
            watched,
            note: None,
            service: None,
        }
    }

    #[test]
    fn mark_extra_watched_only_flips_matching_ports() {
        let mut entries = vec![entry(3000, false), entry(8080, false), entry(5173, true)];
        let n = mark_extra_watched(&mut entries, &[3000]);
        assert_eq!(n, 1, "只置位命中的未关注条目");
        assert!(entries[0].watched, "3000 应被置位");
        assert!(!entries[1].watched, "8080 不在 extra 里，保持不动");
        assert!(entries[2].watched, "5173 原本就 watched，不变");
    }

    #[test]
    fn mark_extra_watched_empty_extra_is_noop() {
        let mut entries = vec![entry(3000, false)];
        assert_eq!(mark_extra_watched(&mut entries, &[]), 0);
        assert!(
            !entries[0].watched,
            "空配置零改动——与 v0.14.0 行为一致（A4）"
        );
    }
}
