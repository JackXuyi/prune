//! 设置页「体检报告」入口（docs/w1-report-sample-plan.md §10 的界面化）。
//!
//! 复用本机已装的 prune CLI（D-14 产品化前的轻量路径）：探测 → spawn 五条
//! `--json` 命令 → 按样例脚本同一口径组装九段 Markdown（脚注溯源、
//! 「0 ≠ 读不到」、脱敏、违禁词零模板）→ 写入 app_data_dir/reports/。
//!
//! 与 scripts/report-sample 的关系：章节结构、口径说明、底线声明逐字同源；
//! 不做 HTML/PDF（界面入口交付 MD + reveal，由 Finder/编辑器接管）。
//! doctor 在「全域不可读」时会先打印完整 JSON 再退出 3/4——判定顺序必须
//! 先解析 JSON 再看退出码（W1-2 实现教训，见 w1 方案 §10）。

use serde::Serialize;
use serde_json::Value;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager};

/// doctor（D-04）自 0.13 起存在；更老的 CLI 不认识 `doctor --json`。
const MIN_VERSION: (u64, u64) = (0, 13);
const N_A: &str = "该字段本版本不提供";

#[derive(Serialize, Clone)]
pub struct CliInfo {
    pub path: String,
    pub version: String,
}

#[derive(Serialize)]
pub struct ReportOutcome {
    pub path: String,
    pub elapsed_s: u64,
    /// 非 ✓ 的采集项说明（已渲染进报告，这里一并返回给前端提示）
    pub degraded: Vec<String>,
    pub cli_version: String,
}

// ---------------------------------------------------------------- 探测

fn candidate_paths() -> Vec<PathBuf> {
    let mut v = Vec::new();
    // GUI 从 Finder 启动时 PATH 很瘦，固定候选路径才是主力
    for p in ["/opt/homebrew/bin/prune", "/usr/local/bin/prune"] {
        v.push(PathBuf::from(p));
    }
    if let Ok(home) = std::env::var("HOME") {
        v.push(PathBuf::from(home).join(".cargo/bin/prune"));
    }
    if let Ok(path) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path) {
            v.push(dir.join("prune"));
        }
    }
    v
}

fn parse_version(stdout: &str) -> Option<String> {
    for word in stdout.split_whitespace() {
        let w = word.trim_start_matches('v');
        let mut it = w.split('.');
        // 非版本号词（如 "prune"）跳过，不直接判失败
        let maj = match it.next().and_then(|s| s.parse::<u64>().ok()) {
            Some(m) => m,
            None => continue,
        };
        let min = match it.next().and_then(|s| s.parse::<u64>().ok()) {
            Some(m) => m,
            None => continue,
        };
        let patch = it.next().and_then(|s| s.parse::<u64>().ok());
        if maj == 0 && min < MIN_VERSION.1 {
            return None; // 太老，没有 doctor 子命令：跳过这个候选
        }
        return Some(format!(
            "v{maj}.{min}{}",
            patch.map(|p| format!(".{p}")).unwrap_or_default()
        ));
    }
    None
}

pub fn detect_cli() -> Option<CliInfo> {
    for c in candidate_paths() {
        if !c.is_file() {
            continue;
        }
        if let Ok(out) = Command::new(&c).arg("--version").output() {
            if out.status.success() {
                if let Some(v) = parse_version(&String::from_utf8_lossy(&out.stdout)) {
                    return Some(CliInfo {
                        path: c.display().to_string(),
                        version: v,
                    });
                }
            }
        }
    }
    None
}

#[tauri::command]
pub fn report_cli_detect() -> Option<CliInfo> {
    detect_cli()
}

// ---------------------------------------------------------------- 采集

struct Collected {
    key: &'static str,
    cmd: &'static str,
    /// 契约 JSON 解析成功（数据可用）
    ok: bool,
    /// 退出码非 0 但 JSON 可用（读不到的域已标注在 JSON 内）
    degraded: bool,
    /// 采集失败时渲染进报告的原因
    note: String,
    ms: u64,
    data: Option<Value>,
}

impl Collected {
    fn fail(key: &'static str, cmd: &'static str, ms: u64, note: String) -> Self {
        Self {
            key,
            cmd,
            ok: false,
            degraded: false,
            note,
            ms,
            data: None,
        }
    }
}

/// 带超时的 spawn。超时后子进程不可直接 kill（handle 在子线程里），
/// 但 CLI 全部只读且自身有域级超时，最坏情况是本次报告失败、进程自然结束。
fn run_with_timeout(bin: &str, args: &[&str], timeout: Duration) -> Result<(String, i32), String> {
    let (tx, rx) = mpsc::channel();
    let bin = bin.to_string();
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    thread::spawn(move || {
        let res = Command::new(&bin)
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output();
        let _ = tx.send(res);
    });
    match rx.recv_timeout(timeout) {
        Ok(Ok(out)) => Ok((
            String::from_utf8_lossy(&out.stdout).into_owned(),
            out.status.code().unwrap_or(-1),
        )),
        Ok(Err(e)) => Err(e.to_string()),
        Err(_) => Err("执行超时".into()),
    }
}

fn collect_one(
    bin: &str,
    key: &'static str,
    cmd: &'static str,
    args: &[&str],
    timeout: Duration,
) -> Collected {
    let t0 = Instant::now();
    let (stdout, code) = match run_with_timeout(bin, args, timeout) {
        Ok(v) => v,
        Err(e) => {
            return Collected::fail(
                key,
                cmd,
                t0.elapsed().as_millis() as u64,
                format!("{cmd} 未采集成功：{e}"),
            )
        }
    };
    let ms = t0.elapsed().as_millis() as u64;
    let parsed: Value = match serde_json::from_str(stdout.trim()) {
        Ok(v) => v,
        Err(_) => {
            return Collected::fail(
                key,
                cmd,
                ms,
                format!("{cmd} 未采集成功：输出不是契约 JSON（退出码 {code}）"),
            );
        }
    };
    // 先看契约，再看退出码——doctor 全域不可读时先打完整 JSON 再退出 3/4
    if parsed.get("schemaVersion").and_then(|x| x.as_u64()) != Some(1) {
        return Collected::fail(
            key,
            cmd,
            ms,
            format!("{cmd} 未采集成功：schemaVersion 不是 1"),
        );
    }
    if code == 0 {
        Collected {
            key,
            cmd,
            ok: true,
            degraded: false,
            note: String::new(),
            ms,
            data: Some(parsed),
        }
    } else {
        Collected {
            key,
            cmd,
            ok: true,
            degraded: true,
            note: format!("退出码 {code}：部分域读不到，原因已标注在对应条目内"),
            ms,
            data: Some(parsed),
        }
    }
}

fn collect_all(bin: &str) -> Vec<Collected> {
    // 两波（W1-2 教训）：doctor 与 scan 同时遍历磁盘会互相挤爆超时
    let wave1: Vec<Collected> = thread::scope(|s| {
        let h_mem = s.spawn(|| {
            collect_one(
                bin,
                "mem",
                "prune mem",
                &["mem", "--json"],
                Duration::from_secs(20),
            )
        });
        let h_port = s.spawn(|| {
            collect_one(
                bin,
                "port",
                "prune port --conflicts",
                &["port", "--conflicts", "--json"],
                Duration::from_secs(20),
            )
        });
        let h_proj = s.spawn(|| {
            collect_one(
                bin,
                "projects",
                "prune projects --days 90",
                &["projects", "--days", "90", "--json"],
                Duration::from_secs(20),
            )
        });
        let h_scan = s.spawn(|| {
            collect_one(
                bin,
                "scan",
                "prune scan",
                &["scan", "--json"],
                Duration::from_secs(90),
            )
        });
        vec![
            h_mem.join().unwrap(),
            h_port.join().unwrap(),
            h_proj.join().unwrap(),
            h_scan.join().unwrap(),
        ]
    });
    let doctor = collect_one(
        bin,
        "doctor",
        "prune doctor",
        &["doctor", "--json"],
        Duration::from_secs(120),
    );
    let mut all = wave1;
    all.push(doctor);
    all
}

// ---------------------------------------------------------------- 组装

struct Ledger {
    n: usize,
    rows: Vec<(String, String)>,
}

impl Ledger {
    fn mark(&mut self, cmd: &str, path: &str) -> String {
        self.n += 1;
        self.rows.push((cmd.to_string(), path.to_string()));
        format!("[^s{}]", self.n)
    }
}

fn fmt_bytes(b: u64) -> String {
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    let v = b as f64;
    if v >= GB {
        format!("{:.1} GB", v / GB)
    } else if v >= MB {
        format!("{:.1} MB", v / MB)
    } else {
        format!("{b} B")
    }
}

fn sev_mark(s: &str) -> &'static str {
    match s {
        "bad" => "🔴",
        "warn" => "🟡",
        "ok" => "🟢",
        _ => "⚪",
    }
}

fn sev_rank(s: &str) -> u8 {
    match s {
        "bad" => 0,
        "warn" => 1,
        "unknown" => 2,
        _ => 3,
    }
}

fn finding<'a>(doc: &'a Value, id: &str) -> Option<&'a Value> {
    doc.get("findings")?
        .as_array()?
        .iter()
        .find(|f| f.get("id").and_then(|x| x.as_str()) == Some(id))
}

fn get_u64(v: &Value, path: &[&str]) -> Option<u64> {
    let mut cur = v;
    for k in path {
        cur = cur.get(k)?;
    }
    cur.as_u64()
}

fn get_str<'a>(v: &'a Value, path: &[&str]) -> Option<&'a str> {
    let mut cur = v;
    for k in path {
        cur = cur.get(k)?;
    }
    cur.as_str()
}

/// 与 SystemTime 无关的本地时间串：只做 civil-from-days（UTC 近似，报告标注采集时刻即可）
fn fmt_timestamp(secs: u64) -> String {
    let days = (secs / 86400) as i64;
    let rem = secs % 86400;
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    // Howard Hinnant 的 civil_from_days
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mth = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mth <= 2 { y + 1 } else { y };
    format!("{y:04}-{mth:02}-{d:02} {h:02}:{m:02}:{s:02} UTC")
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 脱敏：用户名/主目录/主机名 → 代称；私网 IP 掩码后两段。
/// 只处理我们渲染出的受控字段 + 防御性全文替换（不引入 regex 依赖）。
fn redact(md: &str) -> String {
    let mut out = md.to_string();
    if let Ok(home) = std::env::var("HOME") {
        if !home.is_empty() {
            out = out.replace(&home, "~");
        }
    }
    if let Ok(user) = std::env::var("USER") {
        if !user.is_empty() {
            out = out.replace(&user, "用户");
        }
    }
    if let Ok(out_h) = Command::new("hostname").output() {
        let h = String::from_utf8_lossy(&out_h.stdout).trim().to_string();
        if !h.is_empty() {
            out = out.replace(&h, "开发机 #1");
        }
    }
    mask_private_ips_safe(&out)
}

/// 扫描 d.d.d.d 形态的 IPv4，私网段（10.x / 192.168.x / 172.16-31.x）掩码后两段。
/// UTF-8 安全：只在 ASCII 数字/点边界切片，多字节字符按字符长度整体复制。
fn mask_private_ips_safe(md: &str) -> String {
    let bytes = md.as_bytes();
    let mut out = String::with_capacity(md.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            let start = i;
            let mut octets = [0u32; 4];
            let mut j = i;
            let mut valid = true;
            for oi in 0..4 {
                let mut val = 0u32;
                let mut digits = 0;
                while j < bytes.len() && bytes[j].is_ascii_digit() && digits < 3 {
                    val = val * 10 + (bytes[j] - b'0') as u32;
                    j += 1;
                    digits += 1;
                }
                if digits == 0 {
                    valid = false;
                    break;
                }
                octets[oi] = val;
                if oi < 3 {
                    if j < bytes.len() && bytes[j] == b'.' {
                        j += 1;
                    } else {
                        valid = false;
                        break;
                    }
                }
            }
            let boundary_ok = j >= bytes.len() || (!bytes[j].is_ascii_digit() && bytes[j] != b'.');
            if valid && boundary_ok {
                let private = octets[0] == 10
                    || (octets[0] == 192 && octets[1] == 168)
                    || (octets[0] == 172 && (16..=31).contains(&octets[1]));
                if private {
                    // 掩码只保留网络号：10.x → 10.x.x.x；192.168 → 192.168.x.x；172.16-31 → 172.{n}.x.x
                    let masked = if octets[0] == 10 {
                        "10.x.x.x".to_string()
                    } else {
                        format!("{}.{}.x.x", octets[0], octets[1])
                    };
                    out.push_str(&masked);
                    i = j;
                    continue;
                }
            }
            let mut k = start;
            while k < bytes.len() && (bytes[k].is_ascii_digit() || bytes[k] == b'.') {
                k += 1;
            }
            out.push_str(&md[start..k]);
            i = k;
        } else {
            // 非 ASCII：整字符复制（UTF-8 多字节首字节不是数字，安全推进到字符边界）
            let ch_len = utf8_len(bytes[i]);
            let end = (i + ch_len).min(bytes.len());
            out.push_str(&md[i..end]);
            i = end;
        }
    }
    out
}

fn utf8_len(first: u8) -> usize {
    match first {
        0x00..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        _ => 4,
    }
}

fn meta_section(
    items: &[Collected],
    cli_version: &str,
    l: &mut Ledger,
    doc: &Value,
    mem: Option<&Value>,
    scan: Option<&Value>,
) -> String {
    let at = doc
        .get("at")
        .and_then(|x| x.as_u64())
        .map(fmt_timestamp)
        .unwrap_or_else(|| fmt_timestamp(now_secs()));
    let status: Vec<String> = items
        .iter()
        .map(|c| {
            let name = c.cmd.split_whitespace().nth(1).unwrap_or(c.key);
            let mark = if !c.ok {
                "✗"
            } else if c.degraded {
                "⚠"
            } else {
                "✓"
            };
            let note = if c.ok && !c.note.is_empty() {
                format!("（{}）", c.note)
            } else {
                String::new()
            };
            format!("{name} {mark} {:.1}s{note}", c.ms as f64 / 1000.0)
        })
        .collect();
    let gaps: Vec<String> = doc
        .get("coverage")
        .and_then(|x| x.as_object())
        .map(|m| {
            m.iter()
                .filter(|(_, v)| v.get("state").and_then(|s| s.as_str()) != Some("ok"))
                .map(|(k, v)| {
                    let note = v
                        .get("note")
                        .and_then(|n| n.as_str())
                        .unwrap_or("原因未提供");
                    format!("{k}：{note}")
                })
                .collect()
        })
        .unwrap_or_default();
    let machine = match (
        mem.and_then(|m| get_u64(m, &["physicalTotal"])),
        scan.and_then(|s| get_u64(s, &["volume", "totalBytes"])),
        scan.and_then(|s| get_u64(s, &["volume", "availableBytes"])),
    ) {
        (Some(phys), Some(total), Some(avail)) => format!(
            "物理内存 {}{}，磁盘总容量 {}{}，当前可用 {}{}",
            fmt_bytes(phys),
            l.mark("prune mem", "physicalTotal"),
            fmt_bytes(total),
            l.mark("prune scan", "volume.totalBytes"),
            fmt_bytes(avail),
            l.mark("prune scan", "volume.availableBytes"),
        ),
        _ => N_A.to_string(),
    };
    format!(
        "## 1. 元信息\n\n| 项 | 值 |\n|---|---|\n| 生成时间 | {at} |\n| Prune 版本 | prune {cli_version} |\n| 契约版本 schemaVersion | v1 |\n| 采集方式 | 分两波：scan / mem / port / projects 并行，doctor 独占跑（避免两轮磁盘遍历互相挤压触发超时） |\n| 采集状态 | {status} |\n| 机器形态 | {machine} |\n| 未覆盖域 | {gaps} |",
        status = status.join(" · "),
        gaps = if gaps.is_empty() { "无".to_string() } else { gaps.join("；") },
    )
}

fn conclusion_section(
    scan: Option<&Collected>,
    findings: &[&Value],
    cmd_of: &dyn Fn(&Value) -> String,
    l: &mut Ledger,
) -> String {
    let reclaimable =
        scan.and_then(|s| get_u64(s.data.as_ref().unwrap(), &["totals", "reclaimable"]));
    let head = match reclaimable {
        Some(b) => format!(
            "磁盘上可回收空间合计约 **{}**{}。",
            fmt_bytes(b),
            l.mark("prune scan", "totals.reclaimable")
        ),
        None => "磁盘总量本机未采集成功，无法给出可回收合计。".to_string(),
    };
    // 结论卡只列非 🟢 结论；disk.reclaimable_high 与首句重复，让位
    let mut pool: Vec<&&Value> = findings
        .iter()
        .filter(|f| {
            f.get("severity").and_then(|s| s.as_str()) != Some("ok")
                && f.get("id").and_then(|s| s.as_str()) != Some("disk.reclaimable_high")
        })
        .collect();
    pool.sort_by_key(|f| {
        sev_rank(
            f.get("severity")
                .and_then(|s| s.as_str())
                .unwrap_or("unknown"),
        )
    });
    let body = if pool.is_empty() {
        "体检未产出结论（doctor 未采集成功，原因见第 1 节采集状态）。".to_string()
    } else {
        pool.iter()
            .take(3)
            .map(|f| {
                let cmd = cmd_of(f);
                format!(
                    "{} {}{}",
                    sev_mark(
                        f.get("severity")
                            .and_then(|s| s.as_str())
                            .unwrap_or("unknown")
                    ),
                    f.get("title").and_then(|t| t.as_str()).unwrap_or(N_A),
                    l.mark(
                        &cmd,
                        &format!(
                            "findings[id={}].title",
                            f.get("id").and_then(|s| s.as_str()).unwrap_or("?")
                        )
                    )
                )
            })
            .collect::<Vec<_>>()
            .join("；")
            + "。"
    };
    format!("## 2. 结论卡\n\n{head}\n{body}")
}

fn disk_section(it: &Collected, proj: &Collected, l: &mut Ledger) -> String {
    if !it.ok {
        return format!("## 3. 磁盘面\n\n- ⚪ 磁盘面读不到：{}。", it.note);
    }
    let scan = it.data.as_ref().unwrap();
    let mut lines = Vec::new();
    let l2 = get_u64(scan, &["totals", "l2"]);
    let l3 = get_u64(scan, &["totals", "l3"]);
    let l4 = get_u64(scan, &["totals", "l4"]);
    let rec = get_u64(scan, &["totals", "reclaimable"]);
    if let (Some(a), Some(b), Some(c), Some(r)) = (l2, l3, l4, rec) {
        lines.push(format!(
            "- 分级合计：🟢 L2 {}{}、🟡 L3 {}{}、🔴 L4 {}{}；可回收合计 {}{}。",
            fmt_bytes(a),
            l.mark("prune scan", "totals.l2"),
            fmt_bytes(b),
            l.mark("prune scan", "totals.l3"),
            fmt_bytes(c),
            l.mark("prune scan", "totals.l4"),
            fmt_bytes(r),
            l.mark("prune scan", "totals.reclaimable"),
        ));
        lines.push("  - 分级含义：L2 = 可再生缓存（清理后由工具自动重建）；L3 = 需判断项（清理代价高，逐项确认）；L4 = 只给命令、不自动执行。可回收合计只含 L2 + L3，L4 不计入。".to_string());
    } else {
        lines.push(format!("- ⚪ 分级合计读不到：{}。", it.note));
    }
    // 体积最大的 5 组（cleanable，按体积降序）
    let mut groups: Vec<&Value> = scan
        .get("groups")
        .and_then(|g| g.as_array())
        .map(|a| {
            a.iter()
                .filter(|g| {
                    g.get("cleanable")
                        .and_then(|c| c.as_bool())
                        .unwrap_or(false)
                })
                .collect()
        })
        .unwrap_or_default();
    groups.sort_by_key(|g| std::cmp::Reverse(get_u64(g, &["totalBytes"]).unwrap_or(0)));
    if !groups.is_empty() {
        let top: Vec<String> = groups
            .iter()
            .take(5)
            .map(|g| {
                let key = get_str(g, &["key"]).unwrap_or("?");
                let label = get_str(g, &["label"]).unwrap_or(key);
                let level = get_str(g, &["level"]).unwrap_or("?");
                let b = get_u64(g, &["totalBytes"]).unwrap_or(0);
                format!(
                    "{label} {}（{}）{}",
                    fmt_bytes(b),
                    level.to_uppercase(),
                    l.mark("prune scan", &format!("groups[key={key}].totalBytes"))
                )
            })
            .collect();
        lines.push(format!(
            "  - 体积最大的 {} 组：{}。",
            top.len(),
            top.join("、")
        ));
    }
    // 重复存储
    if let Some(dup) = scan.get("dup") {
        let total = get_u64(dup, &["totalBytes"]).unwrap_or(0);
        if total > 0 {
            let advice = get_str(dup, &["advice"])
                .unwrap_or("")
                .trim_end_matches(['。', '；'])
                .to_string();
            lines.push(format!(
                "  - 重复存储：{advice}；合计 {}{}（与上方分组重叠，不相加）。",
                fmt_bytes(total),
                l.mark("prune scan", "dup.totalBytes"),
            ));
        }
    }
    // 项目账本
    if proj.ok {
        let p = proj.data.as_ref().unwrap();
        let arr = p
            .get("projects")
            .and_then(|x| x.as_array())
            .map(|a| a.as_slice())
            .unwrap_or(&[]);
        let days = get_u64(p, &["days"]).unwrap_or(90);
        let flat = arr
            .iter()
            .filter(|x| x.get("deltaBytes").and_then(|d| d.as_u64()) == Some(0))
            .count();
        let insufficient = arr
            .iter()
            .filter(|x| {
                x.get("deltaBytes").and_then(|d| d.as_u64()).is_none()
                    && x.get("deltaBytes").and_then(|d| d.as_f64()).is_none()
            })
            .count();
        let mut ledger_line = format!(
            "- 项目账本（{days} 天）：{} 个项目{}。",
            arr.len(),
            l.mark("prune projects --days 90", "projects")
        );
        let mut moving: Vec<(&Value, i64)> = arr
            .iter()
            .filter_map(|x| {
                let d = x.get("deltaBytes")?;
                if d.is_i64() || d.is_u64() {
                    let v = d.as_i64().unwrap_or(d.as_u64().unwrap_or(0) as i64);
                    if v != 0 {
                        Some((x, v))
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect();
        if moving.is_empty() {
            ledger_line.push_str(&format!(
                "本期均无可判定变化（持平 {flat} 个，采样不足 {insufficient} 个）。"
            ));
        } else {
            moving.sort_by_key(|(_, v)| std::cmp::Reverse(v.abs()));
            let tops: Vec<String> = moving
                .iter()
                .take(3)
                .map(|(x, v)| {
                    let name = get_str(x, &["name"]).unwrap_or("?");
                    let sign = if *v > 0 { "+" } else { "" };
                    format!("{name} {sign}{}", fmt_bytes(v.unsigned_abs()))
                })
                .collect();
            ledger_line.push_str(&format!(
                "有变动的 {} 个（{}）；其余持平 {} 个、采样不足 {} 个。",
                moving.len(),
                tops.join("、"),
                flat,
                insufficient
            ));
        }
        lines.push(ledger_line);
        lines.push(
            "  - 账本口径：只记两次采样之间的体积差；采样不足的项目显示\"数据不足\"，不记 0。"
                .to_string(),
        );
    } else {
        lines.push(format!("- ⚪ 项目账本读不到：{}。", proj.note));
    }
    format!("## 3. 磁盘面\n\n{}", lines.join("\n"))
}

fn mem_section(it: &Collected, doc: &Value, l: &mut Ledger) -> String {
    if !it.ok {
        return format!("## 4. 内存面\n\n- ⚪ 内存面读不到：{}。", it.note);
    }
    let mem = it.data.as_ref().unwrap();
    let mut lines = Vec::new();
    let phys = get_u64(mem, &["physicalTotal"]);
    let sw_used = get_u64(mem, &["swap", "used"]);
    let sw_total = get_u64(mem, &["swap", "total"]);
    // swap 占比单源化：取 doctor 的判定（W1-2 教训：doctor 与 mem 读取时刻相差数十秒）
    let swap_ratio_src = finding(doc, "mem.swap_high").map(|f| {
        (
            f.get("title")
                .and_then(|t| t.as_str())
                .unwrap_or(N_A)
                .to_string(),
            "findings[id=mem.swap_high].title",
        )
    });
    let mut first = String::from("- ");
    if let Some(p) = phys {
        first.push_str(&format!(
            "物理内存 {}{}；",
            fmt_bytes(p),
            l.mark("prune mem", "physicalTotal")
        ));
    }
    if let (Some(u), Some(t)) = (sw_used, sw_total) {
        match &swap_ratio_src {
            Some((title, path)) => first.push_str(&format!(
                "swap 已用 {}{} / 共 {}{}，占比 {}{}；",
                fmt_bytes(u),
                l.mark("prune mem", "swap.used"),
                fmt_bytes(t),
                l.mark("prune mem", "swap.total"),
                title,
                l.mark("prune doctor", path)
            )),
            None => {
                let ratio = if t > 0 { u * 100 / t } else { 0 };
                first.push_str(&format!(
                    "swap 已用 {}{} / 共 {}{}，占比 {ratio}%{}；",
                    fmt_bytes(u),
                    l.mark("prune mem", "swap.used"),
                    fmt_bytes(t),
                    l.mark("prune mem", "swap.total"),
                    l.mark("prune mem", "swap")
                ));
            }
        }
    }
    if let Some(level) = get_str(mem, &["pressure", "level"]) {
        let metric = get_str(mem, &["pressure", "metric"]).unwrap_or("kernel");
        first.push_str(&format!(
            "内存压力 {}{}（口径 {metric}）。",
            level,
            l.mark("prune mem", "pressure.level")
        ));
    }
    lines.push(first);
    // 占用最大的 3 个进程族
    let mut fams: Vec<&Value> = mem
        .get("families")
        .and_then(|f| f.as_array())
        .map(|a| a.iter().collect())
        .unwrap_or_default();
    fams.sort_by_key(|f| std::cmp::Reverse(get_u64(f, &["footprint"]).unwrap_or(0)));
    if !fams.is_empty() {
        let top: Vec<String> = fams
            .iter()
            .take(3)
            .map(|f| {
                let name = get_str(f, &["name"]).unwrap_or("?");
                let fp = get_u64(f, &["footprint"]).unwrap_or(0);
                format!(
                    "{name} {}{}",
                    fmt_bytes(fp),
                    l.mark("prune mem", &format!("families[name={name}].footprint"))
                )
            })
            .collect();
        lines.push(format!(
            "- 占用最大的 {} 个进程族：{}。",
            top.len(),
            top.join("、")
        ));
        // 被换出最多的 3 个进程
        let mut tops: Vec<&Value> = mem
            .get("topSwapped")
            .and_then(|t| t.as_array())
            .map(|a| a.iter().collect())
            .unwrap_or_default();
        tops.sort_by_key(|t| std::cmp::Reverse(get_u64(t, &["swapped"]).unwrap_or(0)));
        if !tops.is_empty() {
            let parts: Vec<String> = tops
                .iter()
                .take(3)
                .map(|t| {
                    let pid = get_u64(t, &["pid"]).unwrap_or(0);
                    let name = get_str(t, &["name"]).unwrap_or("?");
                    let family = get_str(t, &["family"]).unwrap_or("?");
                    let sw = get_u64(t, &["swapped"]).unwrap_or(0);
                    format!(
                        "{name}（{family}，pid {pid}）已换出 {}{}",
                        fmt_bytes(sw),
                        l.mark("prune mem", &format!("topSwapped[pid={pid}].swapped"))
                    )
                })
                .collect();
            lines.push(format!(
                "  - 其中被换出到 swap 最多的：{}。",
                parts.join("、")
            ));
        }
        lines.push(
            "  - 进程族名取可执行文件的家族名，不含路径；单个进程的路径不在报告中出现。"
                .to_string(),
        );
    }
    format!("## 4. 内存面\n\n{}", lines.join("\n"))
}

fn port_section(it: &Collected, l: &mut Ledger) -> String {
    if !it.ok {
        return format!("## 5. 端口面\n\n- ⚪ 端口面读不到：{}。", it.note);
    }
    let p = it.data.as_ref().unwrap();
    let mut lines = Vec::new();
    let conflicts = p
        .get("conflicts")
        .and_then(|c| c.as_array())
        .cloned()
        .unwrap_or_default();
    if conflicts.is_empty() {
        lines.push(format!(
            "- 🟢 冲突端口：未发现{}。",
            l.mark("prune port --conflicts", "conflicts")
        ));
    } else {
        let parts: Vec<String> = conflicts
            .iter()
            .enumerate()
            .map(|(i, x)| {
                let port = x
                    .get("port")
                    .map(|v| v.to_string().trim_matches('"').to_string())
                    .unwrap_or_else(|| "?".into());
                let owners = x
                    .get("owners")
                    .and_then(|o| o.as_array())
                    .map(|os| {
                        os.iter()
                            .filter_map(|o| o.get("name").and_then(|n| n.as_str()))
                            .collect::<Vec<_>>()
                            .join(" / ")
                    })
                    .unwrap_or_else(|| "归属未读到".into());
                format!(
                    "{port}（{owners}）{}",
                    l.mark("prune port --conflicts", &format!("conflicts[{i}]"))
                )
            })
            .collect();
        lines.push(format!(
            "- 🔴 冲突端口 {} 个：{}。",
            conflicts.len(),
            parts.join("、")
        ));
    }
    let attributed = get_u64(p, &["coverage", "attributed"]);
    let total = get_u64(p, &["coverage", "listenTotal"]);
    let note = get_str(p, &["coverage", "note"]).unwrap_or("");
    lines.push(format!(
        "- 监听可见性：可归属 {}{} / 共 {}{} 个监听{note}。差值即当前用户无权限看到的监听——它们不在本报告的任何结论里。",
        attributed.map(|v| v.to_string()).unwrap_or_else(|| N_A.into()),
        l.mark("prune port --conflicts", "coverage.attributed"),
        total.map(|v| v.to_string()).unwrap_or_else(|| N_A.into()),
        l.mark("prune port --conflicts", "coverage.listenTotal"),
        note = if note.is_empty() { String::new() } else { format!("（{note}）") },
    ));
    format!("## 5. 端口面\n\n{}", lines.join("\n"))
}

fn net_section(doc: Option<&Value>, l: &mut Ledger) -> String {
    let Some(doc) = doc else {
        return "## 6. 网络面\n\n- ⚪ 网络面读不到：doctor 未采集成功。".to_string();
    };
    let Some(f) = finding(doc, "net.verdict") else {
        return "## 6. 网络面\n\n- ⚪ 网络面读不到：doctor 未产出 net.verdict 结论。".to_string();
    };
    let title = f.get("title").and_then(|t| t.as_str()).unwrap_or(N_A);
    let ev = f
        .get("evidence")
        .and_then(|e| e.as_array())
        .cloned()
        .unwrap_or_default();
    let parts: Vec<String> = ev
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let label = e.get("label").and_then(|x| x.as_str()).unwrap_or("?");
            let value = e.get("value").and_then(|x| x.as_str()).unwrap_or("?");
            format!(
                "{label} {value}{}",
                l.mark(
                    "prune doctor",
                    &format!("findings[id=net.verdict].evidence[{i}]")
                )
            )
        })
        .collect();
    let sev = f
        .get("severity")
        .and_then(|s| s.as_str())
        .unwrap_or("unknown");
    format!(
        "## 6. 网络面\n\n- {} **{title}**：{}。\n  - 网络域只测\"能不能出去、出去多快\"，不扫描局域网设备，也不记录公网 IP。",
        sev_mark(sev),
        parts.join("，"),
    )
}

/// 第 8/9 节：与样例脚本逐字同源（scripts/report-sample/assemble.mjs）
fn caliber_section() -> String {
    "## 8. 口径说明\n\n\
     - **内存数字用的是 phys_footprint，不是 RSS。** phys_footprint 是进程占用的物理内存页合计，比\"实际内存\"（活动监视器口径）更大也更诚实——被压缩或换出的页都算在内。两套口径没有对错，但混用会得出错误结论。\n\
     - **0 ≠ 读不到。** 某项显示 0 或\"无\"时，如果当时没有读取权限，报告会明确写\"读不到 + 原因\"，绝不把\"没读到\"渲染成\"没有问题\"。\n\
     - **怀疑不下判决。** 体检只报告\"读到什么、依据是什么、下一步可以查什么\"，不自动删除任何东西，也不把猜测写成结论。\n\
     - **同一指标可能来自两条命令。** swap 使用率这类实时量在两次读取之间会变动（doctor 与 mem 的读取时刻相差数十秒）；每个数字都带自己的来源脚注，以脚注指向的命令为准，报告不合并、不取平均、不做四舍五入对齐。\n\
     - **🟢 正常 / 🟡 注意 / 🔴 严重 / ⚪ 读不到。** ⚪ 不是正常：它表示这一项在本机没有读到数据，原因附在条目里。"
        .to_string()
}

fn baseline_section() -> String {
    "## 9. 底线声明\n\n\
     - 本报告**完全在本机生成**，不连接网络服务，不上传任何数据。\n\
     - Prune **不会自动删除**任何文件；报告中所有\"可回收\"均指\"确认后可手动移入废纸篓\"。\n\
     - 报告默认开启脱敏：主机名、用户名、内网地址均已替换；局域网设备清单不在采集范围内。"
        .to_string()
}

fn assemble(items: &[Collected], cli_version: &str) -> String {
    let l = &mut Ledger {
        n: 0,
        rows: Vec::new(),
    };
    let get = |k: &str| items.iter().find(|c| c.key == k);
    let doc = get("doctor")
        .filter(|c| c.ok)
        .map(|c| c.data.as_ref().unwrap());
    let mem = get("mem")
        .filter(|c| c.ok)
        .map(|c| c.data.as_ref().unwrap());
    let scan = get("scan")
        .filter(|c| c.ok)
        .map(|c| c.data.as_ref().unwrap());

    let findings: Vec<&Value> = doc
        .map(|d| {
            d.get("findings")
                .and_then(|f| f.as_array())
                .map(|a| a.iter().collect())
                .unwrap_or_default()
        })
        .unwrap_or_default();
    let doctor_cmd = "prune doctor".to_string();
    let cmd_of = |_f: &Value| -> String { doctor_cmd.clone() };

    let mut parts: Vec<String> = Vec::new();
    parts.push("# 开发机体检报告\n\n> 本报告由 Prune 在本机生成，未上传任何数据。结论口径见第 8 节，每个数字的来源见第 7 节。\n".to_string());
    parts.push(meta_section(
        items,
        cli_version,
        l,
        doc.unwrap_or(&Value::Null),
        mem,
        scan,
    ));
    parts.push(conclusion_section(
        get("scan").filter(|c| c.ok),
        &findings,
        &cmd_of,
        l,
    ));
    // §3 固定渲染（A7：换机/降级结构一致）；项目账本的失败原因在 disk_section 内处理
    match get("scan") {
        Some(s) if s.ok => parts.push(disk_section(s, get("projects").unwrap(), l)),
        _ => {
            let reason = get("scan")
                .map(|c| c.note.clone())
                .unwrap_or_else(|| "未采集".to_string());
            parts.push(format!("## 3. 磁盘面\n\n- ⚪ 磁盘面读不到：{reason}。"));
        }
    }
    match get("mem") {
        Some(m) => parts.push(mem_section(m, doc.unwrap_or(&Value::Null), l)),
        None => parts.push("## 4. 内存面\n\n- ⚪ 内存面读不到：未采集。".to_string()),
    }
    match get("port") {
        Some(p) if p.ok => parts.push(port_section(p, l)),
        _ => {
            let reason = get("port")
                .map(|c| c.note.clone())
                .unwrap_or_else(|| "未采集".to_string());
            parts.push(format!("## 5. 端口面\n\n- ⚪ 端口面读不到：{reason}。"));
        }
    }
    parts.push(net_section(doc, l));
    parts.push(
        "## 7. 来源附录（每个数字可复核）\n\n| 脚注 | 命令 | JSON 路径 |\n|---|---|---|\n"
            .to_string()
            + &l.rows
                .iter()
                .enumerate()
                .map(|(i, (cmd, path))| format!("| S{} | `{cmd}` | `{path}` |", i + 1))
                .collect::<Vec<_>>()
                .join("\n"),
    );
    parts.push(caliber_section());
    parts.push(baseline_section());
    let md = parts.join("\n\n---\n\n") + "\n";
    redact(&md)
}

// ---------------------------------------------------------------- 入口命令

#[tauri::command]
pub async fn report_generate(app: AppHandle) -> Result<ReportOutcome, String> {
    let Some(cli) = detect_cli() else {
        return Err("未检测到可用的 prune CLI（需要 ≥ v0.13）".into());
    };
    let t0 = Instant::now();
    let bin = cli.path.clone();
    let items = tauri::async_runtime::spawn_blocking(move || collect_all(&bin))
        .await
        .map_err(|e| format!("采集任务失败：{e}"))?;

    let degraded: Vec<String> = items
        .iter()
        .filter(|c| !c.ok || c.degraded)
        .map(|c| {
            if c.ok {
                format!("{}：{}", c.cmd, c.note)
            } else {
                c.note.clone()
            }
        })
        .collect();

    let md = assemble(&items, &cli.version);
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("无法定位应用数据目录：{e}"))?
        .join("reports");
    std::fs::create_dir_all(&dir).map_err(|e| format!("无法创建报告目录：{e}"))?;
    let ts = now_secs();
    let path = dir.join(format!("report-{ts}.md"));
    std::fs::write(&path, &md).map_err(|e| format!("无法写入报告：{e}"))?;

    Ok(ReportOutcome {
        path: path.display().to_string(),
        elapsed_s: t0.elapsed().as_secs(),
        degraded,
        cli_version: cli.version,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 用 raw fixture（scripts/report-sample 产物）走一遍完整组装
    fn fixture_collected(key: &'static str, cmd: &'static str, raw: &str) -> Collected {
        Collected {
            key,
            cmd,
            ok: true,
            degraded: false,
            note: String::new(),
            ms: 1000,
            data: Some(serde_json::from_str(raw).unwrap()),
        }
    }

    fn fixture_items() -> Vec<Collected> {
        vec![
            fixture_collected(
                "mem",
                "prune mem",
                include_str!("../../output/report-sample/raw/mem.json"),
            ),
            fixture_collected(
                "port",
                "prune port --conflicts",
                include_str!("../../output/report-sample/raw/port.json"),
            ),
            fixture_collected(
                "projects",
                "prune projects --days 90",
                include_str!("../../output/report-sample/raw/projects.json"),
            ),
            fixture_collected(
                "scan",
                "prune scan",
                include_str!("../../output/report-sample/raw/scan.json"),
            ),
            fixture_collected(
                "doctor",
                "prune doctor",
                include_str!("../../output/report-sample/raw/doctor.json"),
            ),
        ]
    }

    #[test]
    fn assemble_full_structure() {
        let md = assemble(&fixture_items(), "v0.13.1");
        for section in [
            "## 1. 元信息",
            "## 2. 结论卡",
            "## 3. 磁盘面",
            "## 4. 内存面",
            "## 5. 端口面",
            "## 6. 网络面",
            "## 7. 来源附录",
            "## 8. 口径说明",
            "## 9. 底线声明",
        ] {
            assert!(md.contains(section), "缺章节：{section}");
        }
        // 脚注双向一致：正文每个 [^sN] 都有附录行 SN，附录每行都被正文用到
        let marks: Vec<usize> = md
            .match_indices("[^s")
            .map(|(i, _)| {
                let s = &md[i + 3..];
                let n: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
                n.parse().unwrap()
            })
            .collect();
        let rows: Vec<usize> = md
            .match_indices("| S")
            .map(|(i, _)| {
                let s = &md[i + 3..];
                let n: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
                n.parse().unwrap()
            })
            .collect();
        assert!(!marks.is_empty(), "没有任何溯源脚注");
        for n in &marks {
            assert!(rows.contains(n), "脚注 [^s{n}] 没有附录行");
        }
        for n in &rows {
            assert!(marks.contains(n), "附录行 S{n} 没有被正文引用");
        }
        // 不允许渲染出未处理的空值痕迹
        for bad in ["undefined", "NaN", "null", "[object"] {
            assert!(!md.contains(bad), "报告出现 {bad}");
        }
    }

    /// doctor 全域不可读（退出码 3 但打印了完整 JSON）+ 其余命令失败：
    /// 九段结构必须完整保留，「读不到 + 原因」必须出现（W1-2 A3 的 Rust 侧对应）
    #[test]
    fn assemble_degraded_keeps_structure() {
        let doctor = json!({
            "schemaVersion": 1, "at": 1794892843, "elapsedMs": 120,
            "coverage": { "disk": { "state": "unknown", "note": "无法读取磁盘卷信息（权限不足）" } },
            "findings": [ { "id": "disk.volume_low", "domain": "disk", "severity": "unknown",
                "title": "磁盘可用空间读不到", "evidence": [],
                "unreadable": "无法读取磁盘卷信息（权限不足）", "next": ["重试"] } ]
        });
        let items = vec![
            Collected::fail(
                "mem",
                "prune mem",
                10,
                "prune mem 未采集成功：输出不是契约 JSON（退出码 3）".to_string(),
            ),
            Collected::fail(
                "port",
                "prune port --conflicts",
                10,
                "prune port --conflicts 未采集成功：执行超时".to_string(),
            ),
            Collected::fail(
                "projects",
                "prune projects --days 90",
                10,
                "prune projects --days 90 未采集成功：执行超时".to_string(),
            ),
            Collected::fail(
                "scan",
                "prune scan",
                10,
                "prune scan 未采集成功：执行超时".to_string(),
            ),
            Collected {
                key: "doctor",
                cmd: "prune doctor",
                ok: true,
                degraded: true,
                note: "退出码 3：部分域读不到，原因已标注在对应条目内".into(),
                ms: 120,
                data: Some(doctor),
            },
        ];
        let md = assemble(&items, "v0.13.1");
        for section in [
            "## 1. 元信息",
            "## 2. 结论卡",
            "## 3. 磁盘面",
            "## 4. 内存面",
            "## 5. 端口面",
            "## 6. 网络面",
            "## 7. 来源附录",
            "## 8. 口径说明",
            "## 9. 底线声明",
        ] {
            assert!(md.contains(section), "降级时缺章节：{section}");
        }
        assert!(md.contains("读不到"), "降级时未渲染「读不到」");
        assert!(md.contains("权限不足"), "降级原因未渲染");
        assert!(md.contains("⚠"), "degraded 状态未在采集状态里标注");
    }

    #[test]
    fn redact_masks_private_ips_only() {
        assert_eq!(
            mask_private_ips_safe("网关 192.168.1.23 ok"),
            "网关 192.168.x.x ok"
        );
        assert_eq!(mask_private_ips_safe("10.0.0.5"), "10.x.x.x");
        assert_eq!(mask_private_ips_safe("172.16.5.4"), "172.16.x.x");
        assert_eq!(mask_private_ips_safe("172.15.0.1"), "172.15.0.1"); // 非私网段
        assert_eq!(mask_private_ips_safe("8.8.8.8"), "8.8.8.8"); // 公网不掩码
        assert_eq!(mask_private_ips_safe("版本 0.13.1"), "版本 0.13.1"); // 版本号不是 IP
                                                                         // UTF-8 多字节不能被切坏
        assert_eq!(
            mask_private_ips_safe("中文192.168.1.1中文"),
            "中文192.168.x.x中文"
        );
    }

    #[test]
    fn parse_version_rejects_old_cli() {
        assert_eq!(parse_version("prune v0.13.1\n").as_deref(), Some("v0.13.1"));
        assert_eq!(parse_version("prune 0.14.0\n").as_deref(), Some("v0.14.0"));
        assert_eq!(parse_version("prune v0.11.0\n"), None); // 没有 doctor
    }
}
