//! M10 应用网速：`nettop` 累计字节差分。
//!
//! ## 为什么是「轮询 + 差分」而不是流式订阅
//!
//! 实测（2026-11-05，M2/16G）：
//! - `nettop -P -x -l 1 -t external -J bytes_in,bytes_out` 单次 **14.5 ms**（10 次 0.145 s）；
//! - 流式 `-L 0 -s 1` 在**管道/文件**上是 4 KB 块缓冲，实测 4.5 s 才落盘第一批，
//!   必须用 `script` 包一层 pty 才成行缓冲——为一个读数引入 pty 包装不值。
//!
//! 所以：每秒跑一次一次性采样，拿**累计字节**做差分。子进程随起随灭，
//! 不存在「子进程跑飞了要 kill」的生命周期问题。
//!
//! ## 两条不能含糊的口径
//!
//! 1. **只按 pid 匹配，绝不按名匹配**：nettop 把名字截断成 15 字符
//!    （`Google Chrome H.1281`），还可能含非 ASCII；名字唯一的作用是给人看。
//! 2. **进程不在样本里 ≠ 0 B/s**：nettop 只列出「有流量的进程」。当前样本里没有
//!    这个名字，只能说明这一秒没有它的记录——差分语义上按 0 处理，但 UI 的
//!    「不可读」状态与「0」必须严格区分（见 `NetSpeedStatus`）。

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// 累计字节（进程启动至今，nettop 的 `bytes_in` / `bytes_out`）
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RawBytes {
    pub down: u64,
    pub up: u64,
}

/// 差分后的速率（字节/秒）
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Speed {
    pub down_bps: f64,
    pub up_bps: f64,
}

impl Speed {
    pub fn is_zero(&self) -> bool {
        self.down_bps < 0.5 && self.up_bps < 0.5
    }
}

/// 从 `name.pid` 里取 pid。
///
/// 名字本身可能含空格（`Google Chrome H.1281`）、点（`com.apple.WebKit.1234`）
/// 或非 ASCII（`小鸡快跑-….2468`），所以只认**最后一个点**之后的部分。
pub fn pid_of(field: &str) -> Option<i32> {
    let tail = field.rsplit('.').next()?;
    if tail.is_empty() || !tail.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    tail.parse().ok()
}

/// 解析一行。
///
/// 两种形态都要吃（取决于调用方式，实测都有）：
/// - 一次性 `-l 1`：**空格对齐** `Google Chrome H.1281   380098   1619112`
/// - 流式 `-L 0`：**CSV** `Google Chrome H.1281,380098,1619112,`
///
/// 返回 `(pid, 累计字节)`。表头行（`bytes_in/bytes_out`）与格式违规行一律跳过——
/// 私有 CLI 没有格式契约，解析器必须假设「这行可能是垃圾」。
pub fn parse_row(line: &str) -> Option<(i32, RawBytes)> {
    // pty 包装会带 Ctrl-D 之类的控制字符，先清掉
    let line = line.trim_matches(|c: char| c.is_control() || c.is_whitespace());

    if line.is_empty() || line.starts_with("time") || line.contains("bytes_in") {
        return None;
    }

    // CSV 形态：至少有 3 列，且第 2、3 列是纯数字
    if line.contains(',') {
        let cols: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
        if cols.len() < 3 {
            return None;
        }
        let (down, up) = (cols[1].parse::<u64>().ok()?, cols[2].parse::<u64>().ok()?);
        return Some((pid_of(cols[0])?, RawBytes { down, up }));
    }

    // 空格对齐形态：末尾两列是数字，其余全部归名字（名字里可以有空格）
    let toks: Vec<&str> = line.split_whitespace().collect();
    if toks.len() < 3 {
        return None;
    }
    let up = toks[toks.len() - 1].parse::<u64>().ok()?;
    let down = toks[toks.len() - 2].parse::<u64>().ok()?;
    let field = toks[..toks.len() - 2].join(" ");
    Some((pid_of(&field)?, RawBytes { down, up }))
}

/// 一份样本：同一 pid 的多行（不同 socket / 接口 / 形态）先求和。
///
/// nettop 会为同一个进程输出多行（v4/v6、多接口），不求和就会把一条连接的
/// 字节数当成整个进程的。
pub fn parse_sample(text: &str) -> HashMap<i32, RawBytes> {
    let mut out: HashMap<i32, RawBytes> = HashMap::new();
    for line in text.lines() {
        if let Some((pid, bytes)) = parse_row(line) {
            let slot = out.entry(pid).or_default();
            slot.down = slot.down.saturating_add(bytes.down);
            slot.up = slot.up.saturating_add(bytes.up);
        }
    }
    out
}

/// 累计值差分 → 速率。
///
/// - 上一轮没有该 pid（本轮新出现）：给 0。它确实有流量，但我们没有起点，
///   编一个数就是假事实；下一秒就有真实基线了。
/// - 计数器变小（进程重启 / pid 复用）：按 0 处理，不能算负数后回绕成天文数字。
/// - `secs <= 0`（时钟异常）：全部 0。
pub fn diff(prev: &HashMap<i32, RawBytes>, cur: &HashMap<i32, RawBytes>, secs: f64) -> HashMap<i32, Speed> {
    let mut out = HashMap::with_capacity(cur.len());
    if secs <= 0.0 {
        return out;
    }
    for (pid, now) in cur {
        let speed = match prev.get(pid) {
            Some(before) => Speed {
                down_bps: now.down.saturating_sub(before.down) as f64 / secs,
                up_bps: now.up.saturating_sub(before.up) as f64 / secs,
            },
            None => Speed::default(),
        };
        out.insert(*pid, speed);
    }
    out
}

/// 一组 pid 的速率之和（应用维度的家族口径）。
pub fn sum_of(per_pid: &HashMap<i32, Speed>, pids: &[i32]) -> Speed {
    let mut out = Speed::default();
    for pid in pids {
        if let Some(s) = per_pid.get(pid) {
            out.down_bps += s.down_bps;
            out.up_bps += s.up_bps;
        }
    }
    out
}

/// 采样状态。UI 必须能区分「还在攒基线」与「根本读不到」——
/// 两者都显示 0 KB/s 是同一个谎（M8 的教训）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NetSpeedStatus {
    /// 正常出数
    Ok,
    /// 只有第一轮样本，还没有差分基线
    Warmup,
    /// nettop 缺失 / 失败 / 连续解析不出
    Unavailable,
}

/// 一次网速采样的完整产出（IPC 载荷）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetSpeedSample {
    pub at: i64,
    pub status: NetSpeedStatus,
    /// 非空 = 必须显示给用户的原因
    pub note: Option<String>,
    /// pid → 速率。**只含本样本里出现的进程**；前端对缺失 pid 按 0 处理
    pub per_pid: HashMap<i32, Speed>,
    pub total_down_bps: f64,
    pub total_up_bps: f64,
    /// 与上一轮的实际间隔（毫秒），让前端能解释速率的窗口
    pub interval_ms: u64,
}

impl NetSpeedSample {
    pub fn unavailable(note: &str) -> Self {
        Self {
            at: 0,
            status: NetSpeedStatus::Unavailable,
            note: Some(note.to_string()),
            per_pid: HashMap::new(),
            total_down_bps: 0.0,
            total_up_bps: 0.0,
            interval_ms: 0,
        }
    }

    pub fn warmup(interval_ms: u64) -> Self {
        Self {
            at: 0,
            status: NetSpeedStatus::Warmup,
            note: None,
            per_pid: HashMap::new(),
            total_down_bps: 0.0,
            total_up_bps: 0.0,
            interval_ms,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_aligned_output_with_spaces_in_name() {
        // 实测形态：名字被截断成 15 字符，且含空格
        let line = "Google Chrome H.1281                                  890012         60012";
        let (pid, b) = parse_row(line).expect("应解析成功");
        assert_eq!(pid, 1281);
        assert_eq!(b.down, 890012);
        assert_eq!(b.up, 60012);
    }

    #[test]
    fn parses_csv_output() {
        let (pid, b) = parse_row("WeChat.626,54059,34186,").unwrap();
        assert_eq!(pid, 626);
        assert_eq!((b.down, b.up), (54059, 34186));
    }

    #[test]
    fn skips_headers_and_garbage() {
        assert!(parse_row("                    bytes_in       bytes_out").is_none());
        assert!(parse_row(",bytes_in,bytes_out,").is_none());
        assert!(parse_row("").is_none());
        assert!(parse_row("not a nettop line").is_none());
        // 只有名字没有数字
        assert!(parse_row("apsd.364").is_none());
        // pid 不是数字
        assert!(parse_row("weird.name,10,20,").is_none());
    }

    #[test]
    fn sums_multiple_rows_for_same_pid() {
        // 同一进程的 v4/v6 两行必须求和，否则只统计到一半流量
        let text = "\
node.5120,1000,100,
node.5120,500,50,
WeChat.626,10,20,
";
        let sample = parse_sample(text);
        assert_eq!(sample[&5120].down, 1500);
        assert_eq!(sample[&5120].up, 150);
        assert_eq!(sample[&626].down, 10);
    }

    #[test]
    fn diff_computes_rate() {
        let mut prev = HashMap::new();
        prev.insert(1, RawBytes { down: 1000, up: 0 });
        let mut cur = HashMap::new();
        cur.insert(1, RawBytes { down: 3000, up: 500 });
        let s = diff(&prev, &cur, 2.0);
        assert_eq!(s[&1].down_bps, 1000.0);
        assert_eq!(s[&1].up_bps, 250.0);
    }

    #[test]
    fn new_pid_gets_zero_not_invented_rate() {
        // 新进程没有基线：编一个数就是假事实
        let prev = HashMap::new();
        let mut cur = HashMap::new();
        cur.insert(7, RawBytes { down: 9_999_999, up: 1 });
        let s = diff(&prev, &cur, 1.0);
        assert_eq!(s[&7].down_bps, 0.0);
    }

    #[test]
    fn counter_reset_does_not_wrap() {
        // 进程重启后累计值变小：不能算成 2^64 级的负数回绕
        let mut prev = HashMap::new();
        prev.insert(3, RawBytes { down: 10_000, up: 10_000 });
        let mut cur = HashMap::new();
        cur.insert(3, RawBytes { down: 100, up: 100 });
        let s = diff(&prev, &cur, 1.0);
        assert_eq!(s[&3].down_bps, 0.0);
        assert_eq!(s[&3].up_bps, 0.0);
    }

    #[test]
    fn zero_interval_yields_nothing() {
        let mut prev = HashMap::new();
        prev.insert(1, RawBytes { down: 0, up: 0 });
        let mut cur = HashMap::new();
        cur.insert(1, RawBytes { down: 10, up: 10 });
        assert!(diff(&prev, &cur, 0.0).is_empty());
    }

    #[test]
    fn family_sum_ignores_unknown_pids() {
        let mut per = HashMap::new();
        per.insert(1, Speed { down_bps: 100.0, up_bps: 10.0 });
        per.insert(2, Speed { down_bps: 50.0, up_bps: 5.0 });
        let s = sum_of(&per, &[1, 2, 999]);
        assert_eq!(s.down_bps, 150.0);
        assert_eq!(s.up_bps, 15.0);
    }

    /// 拿本机真实 nettop 输出跑一遍：私有 CLI 没有格式契约，光靠手写用例不够。
    /// 默认 ignore（CI 机器可能没有 nettop）；本地用 `cargo test -- --ignored` 跑。
    #[test]
    #[ignore = "依赖本机 nettop"]
    fn parses_real_nettop_output() {
        let out = std::process::Command::new("nettop")
            .args([
                "-P", "-x", "-l", "1", "-t", "external", "-J", "bytes_in,bytes_out",
            ])
            .output()
            .expect("本机应有 nettop");
        assert!(out.status.success(), "nettop 退出码异常");
        let text = String::from_utf8_lossy(&out.stdout);
        let sample = parse_sample(&text);
        assert!(
            !sample.is_empty(),
            "真实输出一条都没解析出来，格式可能变了：\n{text}"
        );
        // 累计值必须是非零量级（本机任何时刻都有系统进程在收发）
        assert!(sample.values().any(|b| b.down + b.up > 0));
    }
}
