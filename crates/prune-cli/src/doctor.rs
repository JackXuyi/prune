//! doctor.rs：`prune doctor` —— 四域体检（wave1 方案 §4，D-04）。
//!
//! 一句话：给出「这台机器现在哪里不对劲」的**结论 + 证据 + 下一步**，
//! 而不是再抛一遍原始读数。磁盘/内存/端口/网络四域，总预算 30s。
//!
//! 判据全部复用既有口径，不新造阈值（方案 §4.6）：
//! - 磁盘水位：`island::DISK_WARN_RATIO`（15%）/ `DISK_CRITICAL_RATIO`（5%），
//!   与菜单栏 island 事件同一判据；
//! - 内存压力：`mem::system::read_pressure()` 的级别；swap > 70% warn；
//! - 端口冲突：`net::ports::conflicts()`（与 GUI/island 同一判据）；
//! - 网络：`NetDiag.status` / `verdict` / `captive`。
//!
//! 硬约束：任一域读不到 → 该域 `unknown` + 原因，**绝不显示 0**
//! （diag.rs「读不到就 None，不下假结论」的纪律原样继承）。
//!
//! 数据源决策（W1-DP-3 裁 B）：磁盘默认**实时**扫一次白名单（~15s），
//! 不新增「最近报告缓存」状态源——CLI 侧本来没有，doctor 是低频命令。
//!
//! 配置边界（W2 收尾 DP-W2-3）：**doctor 不读 `prune.toml`**。doctor 是机器级
//! 体检，阈值是机器级常量（上面的 15% / 5% / 70%）；`prune.toml` 是项目级收窄。
//! 若 doctor 吃项目阈值，同一台机器换个目录结论就变，「结论卡可复核」被
//! 「在哪个目录跑」破坏——两者口径不同源是设计，不是遗漏。

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use prune_core::disk::{self, DiskReport};
use prune_core::island::{DISK_CRITICAL_RATIO, DISK_WARN_RATIO};
use prune_core::mem;
use prune_core::net;
use prune_core::swap;

/// 可回收量达到这个数 → warn【估算，待真机校准】
pub const RECLAIM_WARN_BYTES: u64 = 20 << 30;
/// swap 使用超过这个比例 → warn【估算】
pub const SWAP_WARN_RATIO: f64 = 0.70;
/// 磁盘域单域超时默认值（全量白名单扫描实测 ~15s）
pub const DISK_TIMEOUT_SECS: u64 = 20;
/// `--timeout` 上限（总预算 30s 的硬约束）
pub const TIMEOUT_CAP_SECS: u64 = 30;

// ---------------------------------------------------------------- 域

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Domain {
    Disk,
    Mem,
    Port,
    Net,
}

impl Domain {
    pub fn parse(s: &str) -> Option<Domain> {
        match s {
            "disk" => Some(Domain::Disk),
            "mem" => Some(Domain::Mem),
            "port" => Some(Domain::Port),
            "net" => Some(Domain::Net),
            _ => None,
        }
    }

    /// 输出与 `--only` 的固定顺序（磁盘最慢放最前，超时不拖累别域的结论已出）
    pub const ALL: [Domain; 4] = [Domain::Disk, Domain::Mem, Domain::Port, Domain::Net];

    pub fn name(self) -> &'static str {
        match self {
            Domain::Disk => "disk",
            Domain::Mem => "mem",
            Domain::Port => "port",
            Domain::Net => "net",
        }
    }

    pub fn label_cn(self) -> &'static str {
        match self {
            Domain::Disk => "磁盘",
            Domain::Mem => "内存",
            Domain::Port => "端口",
            Domain::Net => "网络",
        }
    }
}

// ---------------------------------------------------------------- 模型

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Ok,
    Warn,
    Bad,
    /// 只在 coverage.state != ok 时出现——缺数据不能被读成正常
    Unknown,
}

impl Severity {
    fn mark(self) -> &'static str {
        match self {
            Severity::Ok => "✓",
            Severity::Warn => "⚠",
            Severity::Bad => "✗",
            Severity::Unknown => "⊘",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Evidence {
    pub label: String,
    pub value: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    pub id: &'static str,
    pub domain: &'static str,
    pub severity: Severity,
    pub title: String,
    pub evidence: Vec<Evidence>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub next: Vec<String>,
    /// 非空 = 该结论建立在「读不到」上；reason 必须是为什么读不到
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unreadable: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DomainCoverage {
    /// ok | warn | bad | unknown | timeout | skipped
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invisible: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elapsed_ms: Option<u64>,
}

impl DomainCoverage {
    /// 测试与契约快照也用：给定初始 state（其余键缺省）。
    pub fn new(state: &str) -> DomainCoverage {
        DomainCoverage {
            state: state.into(),
            note: None,
            invisible: None,
            elapsed_ms: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Opts {
    pub only: Vec<Domain>,
    pub timeout_secs: u64,
    pub json: bool,
}

// ---------------------------------------------------------------- 运行

pub fn run(opts: &Opts) -> i32 {
    let started = Instant::now();
    let at = swap::now_secs() as i64;
    let mut findings: Vec<Finding> = Vec::new();
    let mut coverage: BTreeMap<&'static str, DomainCoverage> = Domain::ALL
        .iter()
        .map(|d| {
            (
                d.name(),
                if opts.only.is_empty() || opts.only.contains(d) {
                    DomainCoverage::new("ok")
                } else {
                    DomainCoverage::new("skipped")
                },
            )
        })
        .collect();
    let mut domain_ms: BTreeMap<&'static str, u64> = BTreeMap::new();

    for domain in Domain::ALL {
        if !opts.only.is_empty() && !opts.only.contains(&domain) {
            continue;
        }
        let t0 = Instant::now();
        let out = match domain {
            Domain::Disk => run_disk(opts, &mut findings),
            Domain::Mem => run_mem(&mut findings),
            Domain::Port => run_port(&mut findings),
            Domain::Net => run_net(&mut findings),
        };
        let ms = t0.elapsed().as_millis() as u64;
        domain_ms.insert(domain.name(), ms);
        let cov = coverage.get_mut(domain.name()).unwrap();
        *cov = out;
        cov.elapsed_ms = Some(ms);
    }

    let elapsed_ms = started.elapsed().as_millis() as u64;

    if opts.json {
        let doc = crate::docs_json::doctor_doc(at, elapsed_ms, &findings, &coverage);
        println!("{}", serde_json::to_string_pretty(&doc).unwrap());
    } else {
        print_human(at, elapsed_ms, &domain_ms, &findings, &coverage);
    }

    exit_for(&opts.only, &coverage)
}

/// 退出码（方案 §4.5）：0 结论已出（含 warn/bad）；2 用法错（parse 层）；
/// **全部**选中域都不可读时按根因给 3（权限）/ 4（探测失败）。
fn exit_for(only: &[Domain], coverage: &BTreeMap<&'static str, DomainCoverage>) -> i32 {
    let selected: Vec<&DomainCoverage> = if only.is_empty() {
        Domain::ALL
            .iter()
            .filter_map(|d| coverage.get(d.name()))
            .collect()
    } else {
        only.iter().filter_map(|d| coverage.get(d.name())).collect()
    };
    if selected
        .iter()
        .all(|c| c.state == "unknown" || c.state == "timeout")
    {
        if selected
            .iter()
            .any(|c| c.note.as_deref().is_some_and(|n| n.contains("权限")))
        {
            return crate::ExitCode::Permission.as_i32();
        }
        return crate::ExitCode::ProbeFailed.as_i32();
    }
    crate::ExitCode::Ok.as_i32()
}

// ---------------------------------------------------------------- 磁盘域

/// 磁盘域：实时全量扫描（在独立线程跑，`--timeout` 超时就放弃等待并如实
/// 标 timeout——扫描线程随进程退出被回收，CLI 不需要 join）。
fn run_disk(opts: &Opts, findings: &mut Vec<Finding>) -> DomainCoverage {
    let timeout = opts.timeout_secs.min(TIMEOUT_CAP_SECS);
    let (tx, rx) = std::sync::mpsc::channel();
    let home = crate::home_dir();
    std::thread::spawn(move || {
        let report = disk::scan(&home, None);
        tx.send(report).ok();
    });
    let report: DiskReport = match rx.recv_timeout(Duration::from_secs(timeout)) {
        Ok(r) => r,
        Err(_) => {
            return DomainCoverage {
                state: "timeout".into(),
                note: Some(format!(
                    "扫描超过 {timeout}s 未完成：--only mem,port,net 可跳过磁盘域"
                )),
                invisible: None,
                elapsed_ms: None,
            };
        }
    };

    // disk.volume_low：可用比例复用 island 的 15%/5% 判据
    match report.volume {
        None => findings.push(Finding {
            id: "disk.volume_low",
            domain: "disk",
            severity: Severity::Unknown,
            title: "卷容量读不到".into(),
            evidence: Vec::new(),
            next: vec!["df -h 手动确认卷状态".into()],
            unreadable: Some("statvfs 失败：拿不到容量口径，不猜一个 0".into()),
        }),
        Some(v) => {
            let ratio = if v.total_bytes == 0 {
                None
            } else {
                Some(v.user_available_bytes as f64 / v.total_bytes as f64)
            };
            let (sev, title) = match ratio {
                None => (Severity::Unknown, "卷容量为 0（异常口径）".to_string()),
                Some(r) if r < DISK_CRITICAL_RATIO => (Severity::Bad, format!("可用 {}", pct(r))),
                Some(r) if r < DISK_WARN_RATIO => (Severity::Warn, format!("可用 {}", pct(r))),
                Some(r) => (Severity::Ok, format!("可用 {}", pct(r))),
            };
            let mut evidence = vec![
                Evidence {
                    label: "available".into(),
                    value: Value::from(crate::hb(v.user_available_bytes)),
                },
                Evidence {
                    label: "total".into(),
                    value: Value::from(crate::hb(v.total_bytes)),
                },
            ];
            if let Some(r) = ratio {
                evidence.push(Evidence {
                    label: "ratio".into(),
                    value: Value::from(pct(r)),
                });
            }
            findings.push(Finding {
                id: "disk.volume_low",
                domain: "disk",
                severity: sev,
                title,
                evidence,
                next: if sev >= Severity::Warn {
                    vec!["prune reclaim --level green 预览 → GUI 存储页确认清理".into()]
                } else {
                    Vec::new()
                },
                unreadable: None,
            });
        }
    }

    // disk.reclaimable_high：≥ 20 GB 才出现（低于阈值不占屏，scan 里看得到数字）
    if report.totals.reclaimable >= RECLAIM_WARN_BYTES {
        findings.push(Finding {
            id: "disk.reclaimable_high",
            domain: "disk",
            severity: Severity::Warn,
            title: format!(
                "可回收 {}，超过 20 GB",
                crate::hb(report.totals.reclaimable)
            ),
            evidence: vec![Evidence {
                label: "reclaimableBytes".into(),
                value: Value::from(report.totals.reclaimable),
            }],
            next: vec!["prune scan 看清单 → prune reclaim --level green 预览".into()],
            unreadable: None,
        });
    }

    DomainCoverage::new("ok")
}

// ---------------------------------------------------------------- 内存域

fn run_mem(findings: &mut Vec<Finding>) -> DomainCoverage {
    let pressure = mem::system::read_pressure();
    let swap_info = swap::collect(None);

    // mem.pressure：读不到就 unknown + 原因，绝不把「未知」画成「正常」
    let (sev, title, unreadable) = match &pressure {
        None => (
            Severity::Unknown,
            "内存压力读不到".into(),
            Some("系统没有暴露压力口径（Linux 无此语义 / sysctl 被拒）".to_string()),
        ),
        Some(p) => match p.level {
            mem::PressureLevel::Critical => (Severity::Bad, "内存压力 critical".into(), None),
            mem::PressureLevel::Warning => (Severity::Warn, "内存压力 warning".into(), None),
            mem::PressureLevel::Normal => (Severity::Ok, "内存压力正常".into(), None),
        },
    };
    let mut evidence = vec![Evidence {
        label: "swapUsed".into(),
        value: Value::from(crate::hb(swap_info.used)),
    }];
    if let Some(p) = &pressure {
        evidence.push(Evidence {
            label: "pressure".into(),
            value: Value::from(format!("{:?}", p.level)),
        });
    }
    findings.push(Finding {
        id: "mem.pressure",
        domain: "mem",
        severity: sev,
        title,
        evidence,
        next: Vec::new(),
        unreadable,
    });

    // mem.swap_high：swap 总量为 0 = 没配 swap（真实的 0，不是缺数据）
    if swap_info.total > 0 {
        let r = swap_info.used_ratio();
        let sev = if r > SWAP_WARN_RATIO {
            Severity::Warn
        } else {
            Severity::Ok
        };
        findings.push(Finding {
            id: "mem.swap_high",
            domain: "mem",
            severity: sev,
            title: format!("swap 使用 {}", pct(r)),
            evidence: vec![
                Evidence {
                    label: "used".into(),
                    value: Value::from(crate::hb(swap_info.used)),
                },
                Evidence {
                    label: "total".into(),
                    value: Value::from(crate::hb(swap_info.total)),
                },
            ],
            next: Vec::new(),
            unreadable: None,
        });
    }

    let mut cov = DomainCoverage::new("ok");
    if pressure.is_none() {
        cov.state = "unknown".into();
        cov.note = Some("内存压力口径不可用".into());
    }
    cov
}

// ---------------------------------------------------------------- 端口域

fn run_port(findings: &mut Vec<Finding>) -> DomainCoverage {
    let snap = net::ports::scan(None);
    let conflicts = net::ports::conflicts(&snap.entries);

    let mut cov = DomainCoverage::new("ok");

    // 域不可读：采集失败 / 全量权限被拒 → unknown + 原因（不显示 0）
    if let Some(note) = &snap.coverage.note {
        if note.starts_with("端口采集失败") {
            cov.state = "unknown".into();
            cov.note = Some(note.clone());
            findings.push(Finding {
                id: "port.conflict",
                domain: "port",
                severity: Severity::Unknown,
                title: "端口读不到".into(),
                evidence: Vec::new(),
                next: Vec::new(),
                unreadable: Some(note.clone()),
            });
            return cov;
        }
        if snap.coverage.attributed == 0 && snap.coverage.listen_total > 0 {
            cov.state = "unknown".into();
            cov.note = Some(note.clone());
            findings.push(Finding {
                id: "port.conflict",
                domain: "port",
                severity: Severity::Unknown,
                title: "端口读不到".into(),
                evidence: Vec::new(),
                next: Vec::new(),
                unreadable: Some(note.clone()),
            });
            return cov;
        }
    }

    // port.conflict：与 GUI/island 同一判据（owner_count > 1）
    if conflicts.is_empty() {
        findings.push(Finding {
            id: "port.conflict",
            domain: "port",
            severity: Severity::Ok,
            title: "无端口冲突".into(),
            evidence: Vec::new(),
            next: Vec::new(),
            unreadable: None,
        });
    } else {
        for c in &conflicts {
            let owners: Vec<String> = c
                .listeners
                .iter()
                .map(|b| match b.pid {
                    Some(p) => format!("{}({})", b.comm, p),
                    None => format!("{}(内核)", b.comm),
                })
                .collect();
            findings.push(Finding {
                id: "port.conflict",
                domain: "port",
                severity: Severity::Warn,
                title: format!(":{} 被 {} 同时监听", c.port, owners.join(" 与 ")),
                evidence: vec![
                    Evidence {
                        label: "port".into(),
                        value: Value::from(c.port),
                    },
                    Evidence {
                        label: "owners".into(),
                        value: Value::from(owners.join(", ")),
                    },
                ],
                next: vec![format!("prune port {} 看持有者", c.port)],
                unreadable: None,
            });
        }
    }

    // port.invisible：如实信息项（部分不可见是数据事实，不报警）
    let invisible = snap.coverage.invisible();
    if invisible > 0 {
        findings.push(Finding {
            id: "port.invisible",
            domain: "port",
            severity: Severity::Ok,
            title: format!(
                "{invisible} 个监听无法归属到进程（内核口径 {} / 已归属 {}）",
                snap.coverage.listen_total, snap.coverage.attributed
            ),
            evidence: vec![
                Evidence {
                    label: "invisible".into(),
                    value: Value::from(invisible),
                },
                Evidence {
                    label: "total".into(),
                    value: Value::from(snap.coverage.listen_total),
                },
            ],
            next: Vec::new(),
            unreadable: None,
        });
        cov.invisible = Some(invisible);
    }
    if let Some(note) = &snap.coverage.note {
        cov.note = Some(note.clone());
    }
    cov
}

// ---------------------------------------------------------------- 网络域

fn run_net(findings: &mut Vec<Finding>) -> DomainCoverage {
    let diag = net::diag::run_diag(swap::now_secs() as i64, prune_core::locale::Locale::Zh);
    let mut cov = DomainCoverage::new("ok");

    if diag.status == net::diag::DiagStatus::Failed {
        cov.state = "unknown".into();
        let note = diag.note.clone().unwrap_or_else(|| "探测机制失败".into());
        cov.note = Some(note.clone());
        findings.push(Finding {
            id: "net.verdict",
            domain: "net",
            severity: Severity::Unknown,
            title: "网络读不到".into(),
            evidence: Vec::new(),
            next: Vec::new(),
            unreadable: Some(note),
        });
        return cov;
    }

    let mut evidence: Vec<Evidence> = Vec::new();
    for (label, layer) in [("dns", &diag.dns), ("lan", &diag.lan), ("wan", &diag.wan)] {
        if let Some(l) = layer {
            if let Some(ms) = l.median_ms {
                evidence.push(Evidence {
                    label: label.into(),
                    value: Value::from(format!("{ms:.1} ms")),
                });
            }
        }
    }
    if let Some(j) = diag.jitter_ms {
        evidence.push(Evidence {
            label: "jitter".into(),
            value: Value::from(format!("{j:.1} ms")),
        });
    }
    if let Some(gw) = &diag.gateway {
        evidence.push(Evidence {
            label: "gateway".into(),
            value: Value::from(gw.as_str()),
        });
    }

    // net.captive：登录页劫持单独一条
    if diag.captive == Some(net::diag::CaptiveState::Suspected) {
        findings.push(Finding {
            id: "net.captive",
            domain: "net",
            severity: Severity::Warn,
            title: "疑似强制登录页（酒店 / 机场网络）".into(),
            evidence: Vec::new(),
            next: vec!["浏览器打开任意网页完成认证".into()],
            unreadable: None,
        });
    }

    let (sev, title, next) = match diag.verdict.as_ref().and_then(|v| v.kind) {
        Some(net::diag::VerdictKind::AllGood) => (Severity::Ok, "出口正常".into(), Vec::new()),
        None if evidence.is_empty() => {
            (Severity::Unknown, "网络结论读不到".to_string(), Vec::new())
        }
        None => (Severity::Ok, "出口正常".into(), Vec::new()),
        Some(kind) => {
            let v = diag
                .verdict
                .as_ref()
                .map(|v| v.label.clone())
                .unwrap_or_default();
            let sev = match kind {
                net::diag::VerdictKind::LanDown
                | net::diag::VerdictKind::DnsFail
                | net::diag::VerdictKind::WanGap => Severity::Bad,
                _ => Severity::Warn,
            };
            let next = if sev == Severity::Bad {
                vec!["prune net 看分层探测明细".into()]
            } else {
                Vec::new()
            };
            (sev, v, next)
        }
    };
    let unreadable = if sev == Severity::Unknown {
        Some("分层探测都没有产出：不下结论".to_string())
    } else {
        None
    };
    findings.push(Finding {
        id: "net.verdict",
        domain: "net",
        severity: sev,
        title,
        evidence,
        next,
        unreadable,
    });
    if sev == Severity::Unknown {
        cov.state = "unknown".into();
        cov.note = Some("网络分层探测无产出".into());
    }
    cov
}

// ---------------------------------------------------------------- 人读输出

fn pct(r: f64) -> String {
    format!("{:.0}%", r * 100.0)
}

fn print_human(
    at: i64,
    elapsed_ms: u64,
    domain_ms: &BTreeMap<&'static str, u64>,
    findings: &[Finding],
    coverage: &BTreeMap<&'static str, DomainCoverage>,
) {
    let clock = crate::ts_hms(at);
    let parts: Vec<String> = Domain::ALL
        .iter()
        .map(|d| {
            // 未运行的域不显示 0.0s——「没跑」不是「瞬间完成」
            if coverage.get(d.name()).is_some_and(|c| c.state == "skipped") {
                return format!("{} 未运行", d.label_cn());
            }
            let ms = domain_ms.get(d.name()).copied().unwrap_or(0);
            format!("{} {:.1}s", d.label_cn(), ms as f64 / 1000.0)
        })
        .collect();
    println!(
        "Prune 体检 · {clock} · 用时 {:.1}s（{}）",
        elapsed_ms as f64 / 1000.0,
        parts.join(" / ")
    );

    for domain in Domain::ALL {
        let cov = &coverage[domain.name()];
        println!();
        if cov.state == "skipped" {
            println!("{}  ⊘ 未运行（--only 未包含）", domain.label_cn());
            continue;
        }
        let items: Vec<&Finding> = findings
            .iter()
            .filter(|f| f.domain == domain.name())
            .collect();
        if items.is_empty() {
            println!("{}  ⊘ 不可读：无产出", domain.label_cn());
            continue;
        }
        // 域标题 = 最严重那条的结论
        let worst = items
            .iter()
            .max_by_key(|f| match f.severity {
                Severity::Ok => 0,
                Severity::Unknown => 1,
                Severity::Warn => 2,
                Severity::Bad => 3,
            })
            .copied()
            .unwrap();
        println!(
            "{}  {} {}",
            domain.label_cn(),
            worst.severity.mark(),
            worst.title
        );
        for f in &items {
            if f.evidence.is_empty() && f.unreadable.is_none() {
                continue;
            }
            if let Some(reason) = &f.unreadable {
                println!("      ⊘ 不可读：{reason}");
            }
            let ev: Vec<String> = f
                .evidence
                .iter()
                .map(|e| match &e.value {
                    Value::String(s) => format!("{} {}", e.label, s),
                    other => format!("{} {}", e.label, other),
                })
                .collect();
            if !ev.is_empty() {
                println!("      证据：{}", ev.join(" · "));
            }
            for n in &f.next {
                println!("      下一步：{n}");
            }
        }
        if let Some(note) = &cov.note {
            println!("      ⚠️ {note}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_parse_covers_hint_and_rejects_unknown() {
        for d in Domain::ALL {
            assert_eq!(Domain::parse(d.name()), Some(d));
        }
        assert_eq!(Domain::parse("cpu"), None);
        assert_eq!(Domain::parse(""), None);
    }

    #[test]
    fn severity_serializes_to_fixed_vocabulary() {
        // 词表固定四个值（方案 §4.3）：Agent 与 CI 按词表消费，不允许多余词
        for (s, want) in [
            (Severity::Ok, "\"ok\""),
            (Severity::Warn, "\"warn\""),
            (Severity::Bad, "\"bad\""),
            (Severity::Unknown, "\"unknown\""),
        ] {
            assert_eq!(serde_json::to_string(&s).unwrap(), want);
        }
    }

    #[test]
    fn exit_only_when_every_selected_domain_unreadable() {
        let mut cov = BTreeMap::new();
        for d in Domain::ALL {
            cov.insert(d.name(), DomainCoverage::new("ok"));
        }
        assert_eq!(exit_for(&[], &cov), 0);

        // 全部 unknown 且根因含权限 → 3
        let mut cov2 = cov.clone();
        for d in Domain::ALL {
            let mut c = DomainCoverage::new("unknown");
            c.note = Some("无法读取进程的 socket 信息（权限不足）".into());
            cov2.insert(d.name(), c);
        }
        assert_eq!(exit_for(&[], &cov2), 3);

        // 全部 unknown 但根因不是权限 → 4
        let mut cov3 = cov.clone();
        for d in Domain::ALL {
            let mut c = DomainCoverage::new("unknown");
            c.note = Some("探测超时".into());
            cov3.insert(d.name(), c);
        }
        assert_eq!(exit_for(&[], &cov3), 4);

        // 有一个域可读就还是 0：部分不可见不是进程失败
        let mut cov4 = cov3.clone();
        cov4.insert("mem", DomainCoverage::new("ok"));
        assert_eq!(exit_for(&[], &cov4), 0);

        // --only 选中范围外的 unknown 不参与判定：mem 可读 → 0
        assert_eq!(exit_for(&[Domain::Mem], &cov4), 0);
    }

    #[test]
    fn mem_findings_never_fake_zero_when_unreadable() {
        // read_pressure 在测试进程里可能拿得到也可能拿不到；不管哪种，
        // 走 run_mem 都不该产生「pressure=Normal 却无解释」的假正常。
        let mut findings = Vec::new();
        let cov = run_mem(&mut findings);
        let p = findings.iter().find(|f| f.id == "mem.pressure").unwrap();
        if cov.state == "unknown" {
            assert_eq!(p.severity, Severity::Unknown);
            assert!(p.unreadable.is_some(), "缺数据必须带原因");
        }
    }
}
